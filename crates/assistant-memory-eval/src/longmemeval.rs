use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::Instant;

use assistant_memory::Memory;
use assistant_memory::brief::estimate_brief_tokens;
use assistant_memory::rusqlite::params;
use assistant_memory::tools::search_thoughts_tool;
use chrono::{Duration, SecondsFormat, TimeZone, Utc};
use serde::de::{Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::manifest::{CaseResult, relevant_ranks};
use crate::metrics::score_pr_ranking;

#[derive(Debug, Error)]
pub enum LongMemEvalError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] assistant_memory::rusqlite::Error),
    #[error(transparent)]
    Memory(#[from] assistant_memory::Error),
    #[error(transparent)]
    Metric(#[from] crate::metrics::MetricError),
    #[error("unsupported LongMemEval manifest schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("dataset license is not approved: {0}")]
    UnapprovedLicense(String),
    #[error("dataset revision must be a 40-character lowercase hexadecimal commit SHA")]
    UnpinnedRevision,
    #[error("dataset source must embed the pinned revision")]
    UnpinnedSource,
    #[error("dataset checksum must be a 64-character lowercase hexadecimal SHA-256")]
    InvalidChecksum,
    #[error("duplicate selected question ID: {0}")]
    DuplicateQuestion(String),
    #[error("selected question has no pinned evidence: {0}")]
    EmptyEvidence(String),
    #[error("dataset checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("selected question is missing from the dataset: {0}")]
    MissingQuestion(String),
    #[error("question {question_id} has inconsistent session arrays")]
    InconsistentSessions { question_id: String },
    #[error("question {question_id} type differs from the pinned manifest")]
    QuestionTypeDrift { question_id: String },
    #[error("question {question_id} evidence differs from the pinned manifest")]
    EvidenceDrift { question_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongMemEvalManifest {
    pub schema_version: u32,
    pub suite_version: String,
    pub selection_method: String,
    pub dataset: Dataset,
    pub cases: Vec<PinnedCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    pub name: String,
    pub source: String,
    pub revision: String,
    pub sha256: String,
    pub license: String,
    pub license_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedCase {
    pub question_id: String,
    pub question_type: String,
    pub evidence_session_ids: Vec<String>,
    pub evidence_turn_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LongMemEvalEntry {
    pub question_id: String,
    pub question_type: String,
    pub question: String,
    pub haystack_session_ids: Vec<String>,
    pub haystack_dates: Vec<String>,
    pub haystack_sessions: Vec<Vec<Turn>>,
    pub answer_session_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Turn {
    pub role: String,
    pub content: String,
    pub has_answer: Option<bool>,
}

#[derive(Debug)]
pub struct LongMemEvalSuiteResult {
    pub suite_version: String,
    pub cases: Vec<CaseResult>,
    pub case_duration_us: BTreeMap<String, u64>,
}

pub fn load_manifest(input: &str) -> Result<LongMemEvalManifest, LongMemEvalError> {
    let manifest: LongMemEvalManifest = serde_json::from_str(input)?;
    if manifest.schema_version != 1 {
        return Err(LongMemEvalError::UnsupportedSchema(manifest.schema_version));
    }
    if !is_lower_hex(&manifest.dataset.revision, 40) {
        return Err(LongMemEvalError::UnpinnedRevision);
    }
    if !manifest.dataset.source.contains(&manifest.dataset.revision) {
        return Err(LongMemEvalError::UnpinnedSource);
    }
    if !is_lower_hex(&manifest.dataset.sha256, 64) {
        return Err(LongMemEvalError::InvalidChecksum);
    }
    let mut seen = BTreeSet::new();
    for case in &manifest.cases {
        if !seen.insert(case.question_id.as_str()) {
            return Err(LongMemEvalError::DuplicateQuestion(
                case.question_id.clone(),
            ));
        }
        if case.evidence_session_ids.is_empty() || case.evidence_turn_ids.is_empty() {
            return Err(LongMemEvalError::EmptyEvidence(case.question_id.clone()));
        }
    }
    Ok(manifest)
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn verify_cache(manifest: &LongMemEvalManifest, path: &Path) -> Result<(), LongMemEvalError> {
    if manifest.dataset.license != "MIT" {
        return Err(LongMemEvalError::UnapprovedLicense(
            manifest.dataset.license.clone(),
        ));
    }
    let actual = sha256_file(path)?;
    if actual != manifest.dataset.sha256 {
        return Err(LongMemEvalError::ChecksumMismatch {
            expected: manifest.dataset.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

pub fn load_selected_dataset(
    manifest: &LongMemEvalManifest,
    path: &Path,
) -> Result<Vec<LongMemEvalEntry>, LongMemEvalError> {
    verify_cache(manifest, path)?;
    let wanted: BTreeSet<String> = manifest
        .cases
        .iter()
        .map(|case| case.question_id.clone())
        .collect();
    let file = File::open(path)?;
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    let selected = deserializer.deserialize_seq(SelectedVisitor { wanted: &wanted })?;
    let mut by_id: BTreeMap<String, LongMemEvalEntry> = selected
        .into_iter()
        .map(|entry| (entry.question_id.clone(), entry))
        .collect();
    let mut ordered = Vec::with_capacity(manifest.cases.len());
    for pinned in &manifest.cases {
        let entry = by_id
            .remove(&pinned.question_id)
            .ok_or_else(|| LongMemEvalError::MissingQuestion(pinned.question_id.clone()))?;
        validate_entry(pinned, &entry)?;
        ordered.push(entry);
    }
    Ok(ordered)
}

pub fn run_longmemeval_suite(
    manifest: &LongMemEvalManifest,
    path: &Path,
) -> Result<LongMemEvalSuiteResult, LongMemEvalError> {
    let entries = load_selected_dataset(manifest, path)?;
    let mut cases = Vec::with_capacity(entries.len());
    let mut case_duration_us = BTreeMap::new();
    for (pinned, entry) in manifest.cases.iter().zip(entries) {
        let started = Instant::now();
        cases.push(run_case(pinned, &entry)?);
        case_duration_us.insert(
            pinned.question_id.clone(),
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
    }
    Ok(LongMemEvalSuiteResult {
        suite_version: manifest.suite_version.clone(),
        cases,
        case_duration_us,
    })
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

struct SelectedVisitor<'a> {
    wanted: &'a BTreeSet<String>,
}

impl<'de> Visitor<'de> for SelectedVisitor<'_> {
    type Value = Vec<LongMemEvalEntry>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a LongMemEval JSON array")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut selected = Vec::with_capacity(self.wanted.len());
        while let Some(entry) = sequence.next_element::<LongMemEvalEntry>()? {
            if self.wanted.contains(&entry.question_id) {
                selected.push(entry);
            }
        }
        Ok(selected)
    }
}

fn validate_entry(pinned: &PinnedCase, entry: &LongMemEvalEntry) -> Result<(), LongMemEvalError> {
    if entry.haystack_session_ids.len() != entry.haystack_sessions.len()
        || entry.haystack_dates.len() != entry.haystack_sessions.len()
    {
        return Err(LongMemEvalError::InconsistentSessions {
            question_id: entry.question_id.clone(),
        });
    }
    if entry.question_type != pinned.question_type {
        return Err(LongMemEvalError::QuestionTypeDrift {
            question_id: entry.question_id.clone(),
        });
    }
    let evidence_turn_ids = evidence_turn_ids(entry);
    if entry.answer_session_ids != pinned.evidence_session_ids
        || evidence_turn_ids != pinned.evidence_turn_ids
    {
        return Err(LongMemEvalError::EvidenceDrift {
            question_id: entry.question_id.clone(),
        });
    }
    Ok(())
}

fn evidence_turn_ids(entry: &LongMemEvalEntry) -> Vec<String> {
    let mut ids = Vec::new();
    for (session_index, session) in entry.haystack_sessions.iter().enumerate() {
        for (turn_index, turn) in session.iter().enumerate() {
            if turn.has_answer == Some(true) {
                ids.push(format!(
                    "{}_{}",
                    entry.haystack_session_ids[session_index],
                    turn_index + 1
                ));
            }
        }
    }
    ids
}

fn run_case(pinned: &PinnedCase, entry: &LongMemEvalEntry) -> Result<CaseResult, LongMemEvalError> {
    let memory = Memory::open_in_memory()?;
    let start = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    let mut sequence = 0i64;
    let mut evidence_keys = BTreeMap::new();
    for (session_index, session) in entry.haystack_sessions.iter().enumerate() {
        for (turn_index, turn) in session.iter().enumerate() {
            let turn_id = format!(
                "{}_{}",
                entry.haystack_session_ids[session_index],
                turn_index + 1
            );
            let id = format!("{}:{session_index}:{turn_index}", entry.question_id);
            evidence_keys.insert(
                id.clone(),
                (entry.haystack_session_ids[session_index].clone(), turn_id),
            );
            let content = format!(
                "{}\n{}: {}",
                entry.haystack_dates[session_index], turn.role, turn.content
            );
            let summary: String = content.chars().take(500).collect();
            let timestamp =
                (start + Duration::seconds(sequence)).to_rfc3339_opts(SecondsFormat::Secs, true);
            sequence += 1;
            memory.conn.execute(
                "INSERT INTO thoughts (id, content, summary, type, source, trust_level, visibility, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'note', 'longmemeval', 'trusted', 'personal', ?4, ?4)",
                params![id, content, summary, timestamp],
            )?;
        }
    }

    let result = search_thoughts_tool(
        &memory,
        &json!({
            "query": entry.question,
            "limit": 10,
            "all_projects": true,
        }),
    );
    let tool_output = result.json();
    let ranked_storage_ids: Vec<String> = tool_output["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    let mut ranked_turn_ids = Vec::new();
    let mut ranked_session_ids = Vec::new();
    for storage_id in &ranked_storage_ids {
        let Some((session_id, turn_id)) = evidence_keys.get(storage_id) else {
            continue;
        };
        if !ranked_turn_ids.contains(turn_id) {
            ranked_turn_ids.push(turn_id.clone());
        }
        if !ranked_session_ids.iter().any(|id| id == session_id) {
            ranked_session_ids.push(session_id.clone());
        }
    }
    let relevant: BTreeSet<String> = pinned.evidence_session_ids.iter().cloned().collect();
    let metrics = score_pr_ranking(&ranked_session_ids, &relevant)?;
    let failures = result
        .is_error
        .then(|| format!("search_thoughts returned an error: {}", result.text))
        .into_iter()
        .collect::<Vec<_>>();

    Ok(CaseResult {
        id: pinned.question_id.clone(),
        suite: "longmemeval-pr".into(),
        category: pinned.question_type.clone(),
        passed: failures.is_empty(),
        ranked_ids: ranked_session_ids.clone(),
        relevant_ids: pinned.evidence_session_ids.clone(),
        relevant_ranks: relevant_ranks(&ranked_session_ids, &pinned.evidence_session_ids),
        forbidden_ids: Vec::new(),
        metrics: Some(metrics),
        estimated_tokens: estimate_brief_tokens(&result.text).max(0) as u64,
        serialized_bytes: result.text.len() as u64,
        failures,
        output: json!({
            "mode": tool_output["mode"],
            "ranked_session_ids": ranked_session_ids,
            "ranked_turn_ids": ranked_turn_ids,
            "evidence_session_ids": pinned.evidence_session_ids,
            "evidence_turn_ids": pinned.evidence_turn_ids,
        }),
    })
}

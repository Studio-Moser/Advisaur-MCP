use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use assistant_memory::brief::estimate_brief_tokens;
use assistant_memory::projects::{ProjectSeed, upsert_project};
use assistant_memory::rusqlite::params;
use assistant_memory::tools::{get_brief_tool, search_thoughts_tool, select_context_tool};
use assistant_memory::vectors::embedding_to_bytes;
use assistant_memory::{Memory, identity::derive_existing_project_id};
use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::manifest::{CaseResult, relevant_ranks};
use crate::metrics::score_pr_ranking;

/// The project-owned suites, beside the public datasets (ADR-0012 renamed them from
/// `shelby-contract` and `shelby-hard-confuser`).
pub const CONTRACT_SUITE: &str = "contract";
pub const HARD_CONFUSER_SUITE: &str = "hard-confuser";

/// Whether a case belongs to a project-owned suite rather than a public dataset.
pub fn is_project_suite(suite: &str) -> bool {
    suite == CONTRACT_SUITE || suite == HARD_CONFUSER_SUITE
}

#[derive(Debug, Error)]
pub enum ContractError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Memory(#[from] assistant_memory::Error),
    #[error(transparent)]
    Sqlite(#[from] assistant_memory::rusqlite::Error),
    #[error(transparent)]
    Metric(#[from] crate::metrics::MetricError),
    #[error("unsupported contract schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("duplicate project slug: {0}")]
    DuplicateProject(String),
    #[error("duplicate thought ID: {0}")]
    DuplicateThought(String),
    #[error("duplicate edge ID: {0}")]
    DuplicateEdge(String),
    #[error("duplicate case ID: {0}")]
    DuplicateCase(String),
    #[error("unknown project slug on thought {thought_id}: {project}")]
    UnknownProject { thought_id: String, project: String },
    #[error("edge {edge_id} refers to unknown thought: {thought_id}")]
    UnknownEdgeThought { edge_id: String, thought_id: String },
    #[error("thought {0} has an invalid trust level")]
    InvalidTrust(String),
    #[error("invalid timestamp on {entity}: {value}")]
    InvalidTimestamp { entity: String, value: String },
    #[error("case {case_id} expects unknown thought ID: {thought_id}")]
    UnknownExpectedThought { case_id: String, thought_id: String },
    #[error("hard-confuser case {0} contains an empty required field")]
    EmptyHardConfuser(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractSuite {
    schema_version: u32,
    suite_version: String,
    projects: Vec<String>,
    thoughts: Vec<FixtureThought>,
    #[serde(default)]
    edges: Vec<FixtureEdge>,
    #[serde(default)]
    hard_confusers: Vec<HardConfuserCase>,
    cases: Vec<ContractCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HardConfuserCase {
    id: String,
    scenario: HardConfuserScenario,
    query: String,
    relevant_content: String,
    confuser_content: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HardConfuserScenario {
    SameTopicWrongProject,
    SamePersonWrongEvent,
    StaleFactCurrentFact,
    NearDuplicateDecision,
}

impl HardConfuserScenario {
    fn category(self) -> &'static str {
        match self {
            Self::SameTopicWrongProject => "same-topic-wrong-project",
            Self::SamePersonWrongEvent => "same-person-wrong-event",
            Self::StaleFactCurrentFact => "stale-fact-current-fact",
            Self::NearDuplicateDecision => "near-duplicate-decision",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureThought {
    id: String,
    content: String,
    summary: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    source: String,
    trust_level: String,
    project_identifier: Option<String>,
    visibility: String,
    #[serde(default)]
    metadata: Map<String, Value>,
    embedding: Option<Vec<f32>>,
    created_at: String,
    updated_at: String,
    consolidated_into: Option<String>,
    #[serde(default)]
    reinforcement_count: i64,
    last_confirmed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureEdge {
    id: String,
    source_id: String,
    target_id: String,
    edge_type: String,
    #[serde(default)]
    metadata: Map<String, Value>,
    created_at: String,
    valid_from: Option<String>,
    valid_until: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Handler {
    SearchThoughts,
    GetBrief,
    SelectContext,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractCase {
    id: String,
    category: String,
    handler: Handler,
    arguments: Value,
    expected: Expected,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    ranked_ids: Option<Vec<String>>,
    #[serde(default)]
    forbidden_ids: Vec<String>,
    #[serde(default)]
    json_values: BTreeMap<String, Value>,
    #[serde(default)]
    minimum_numbers: BTreeMap<String, f64>,
    #[serde(default)]
    required_text: Vec<String>,
    #[serde(default)]
    forbidden_text: Vec<String>,
    max_estimated_tokens: Option<u64>,
    error_category: Option<String>,
}

#[derive(Debug)]
pub struct ContractSuiteResult {
    pub suite_version: String,
    pub cases: Vec<CaseResult>,
    pub case_duration_us: BTreeMap<String, u64>,
}

pub fn run_contract_suite(input: &str) -> Result<ContractSuiteResult, ContractError> {
    let suite: ContractSuite = serde_json::from_str(input)?;
    validate(&suite)?;
    let mut cases = Vec::with_capacity(suite.cases.len() + suite.hard_confusers.len());
    let mut case_duration_us = BTreeMap::new();
    for case in &suite.cases {
        let started = Instant::now();
        cases.push(run_case(&suite, case)?);
        case_duration_us.insert(
            case.id.clone(),
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
    }
    for case in &suite.hard_confusers {
        let started = Instant::now();
        cases.push(run_hard_confuser_case(case)?);
        case_duration_us.insert(
            case.id.clone(),
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
    }
    Ok(ContractSuiteResult {
        suite_version: suite.suite_version,
        cases,
        case_duration_us,
    })
}

fn validate(suite: &ContractSuite) -> Result<(), ContractError> {
    if suite.schema_version != 1 {
        return Err(ContractError::UnsupportedSchema(suite.schema_version));
    }
    reject_duplicate(
        suite.projects.iter().map(String::as_str),
        ContractError::DuplicateProject,
    )?;
    reject_duplicate(
        suite.thoughts.iter().map(|thought| thought.id.as_str()),
        ContractError::DuplicateThought,
    )?;
    reject_duplicate(
        suite.edges.iter().map(|edge| edge.id.as_str()),
        ContractError::DuplicateEdge,
    )?;
    reject_duplicate(
        suite
            .cases
            .iter()
            .map(|case| case.id.as_str())
            .chain(suite.hard_confusers.iter().map(|case| case.id.as_str())),
        ContractError::DuplicateCase,
    )?;
    for case in &suite.hard_confusers {
        if case.query.trim().is_empty()
            || case.relevant_content.trim().is_empty()
            || case.confuser_content.trim().is_empty()
        {
            return Err(ContractError::EmptyHardConfuser(case.id.clone()));
        }
    }

    let projects: BTreeSet<&str> = suite.projects.iter().map(String::as_str).collect();
    let thoughts: BTreeSet<&str> = suite
        .thoughts
        .iter()
        .map(|thought| thought.id.as_str())
        .collect();
    for thought in &suite.thoughts {
        if assistant_memory::thoughts::TrustLevel::parse(&thought.trust_level).is_none() {
            return Err(ContractError::InvalidTrust(thought.id.clone()));
        }
        if let Some(project) = thought.project_identifier.as_deref()
            && !projects.contains(project)
        {
            return Err(ContractError::UnknownProject {
                thought_id: thought.id.clone(),
                project: project.to_string(),
            });
        }
        validate_timestamp(&format!("thought {}", thought.id), &thought.created_at)?;
        validate_timestamp(&format!("thought {}", thought.id), &thought.updated_at)?;
        if let Some(timestamp) = thought.last_confirmed_at.as_deref() {
            validate_timestamp(&format!("thought {}", thought.id), timestamp)?;
        }
    }
    for edge in &suite.edges {
        for thought_id in [&edge.source_id, &edge.target_id] {
            if !thoughts.contains(thought_id.as_str()) {
                return Err(ContractError::UnknownEdgeThought {
                    edge_id: edge.id.clone(),
                    thought_id: thought_id.clone(),
                });
            }
        }
        validate_timestamp(&format!("edge {}", edge.id), &edge.created_at)?;
        for timestamp in [edge.valid_from.as_deref(), edge.valid_until.as_deref()]
            .into_iter()
            .flatten()
        {
            validate_timestamp(&format!("edge {}", edge.id), timestamp)?;
        }
    }
    for case in &suite.cases {
        for thought_id in case
            .expected
            .ranked_ids
            .iter()
            .flatten()
            .chain(&case.expected.forbidden_ids)
        {
            if !thoughts.contains(thought_id.as_str()) {
                return Err(ContractError::UnknownExpectedThought {
                    case_id: case.id.clone(),
                    thought_id: thought_id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_timestamp(entity: &str, value: &str) -> Result<(), ContractError> {
    chrono::DateTime::parse_from_rfc3339(value).map_err(|_| ContractError::InvalidTimestamp {
        entity: entity.to_string(),
        value: value.to_string(),
    })?;
    Ok(())
}

fn reject_duplicate<'a>(
    values: impl Iterator<Item = &'a str>,
    error: impl Fn(String) -> ContractError,
) -> Result<(), ContractError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(error(value.to_string()));
        }
    }
    Ok(())
}

fn seeded_memory(suite: &ContractSuite) -> Result<Memory, ContractError> {
    let memory = Memory::open_in_memory()?;
    for slug in &suite.projects {
        upsert_project(
            &memory.conn,
            &ProjectSeed {
                slug: slug.clone(),
                display_name: slug.clone(),
                ..Default::default()
            },
        )?;
    }

    for thought in &suite.thoughts {
        let project_id = thought
            .project_identifier
            .as_deref()
            .and_then(derive_existing_project_id);
        let metadata = serde_json::to_string(&thought.metadata)?;
        let embedding = thought.embedding.as_deref().map(embedding_to_bytes);
        memory.conn.execute(
            "INSERT INTO thoughts (id, content, summary, type, source, trust_level, project_id, project_identifier,
               visibility, metadata, embedding, created_at, updated_at, consolidated_into, reinforcement_count, last_confirmed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                thought.id,
                thought.content,
                thought.summary,
                thought.kind,
                thought.source,
                thought.trust_level,
                project_id,
                thought.project_identifier,
                thought.visibility,
                metadata,
                embedding,
                thought.created_at,
                thought.updated_at,
                thought.consolidated_into,
                thought.reinforcement_count,
                thought.last_confirmed_at,
            ],
        )?;
    }
    for edge in &suite.edges {
        let metadata = serde_json::to_string(&edge.metadata)?;
        memory.conn.execute(
            "INSERT INTO edges (id, source_id, target_id, edge_type, metadata, created_at, valid_from, valid_until)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                edge.id,
                edge.source_id,
                edge.target_id,
                edge.edge_type,
                metadata,
                edge.created_at,
                edge.valid_from,
                edge.valid_until,
            ],
        )?;
    }
    Ok(memory)
}

fn run_case(suite: &ContractSuite, case: &ContractCase) -> Result<CaseResult, ContractError> {
    let memory = seeded_memory(suite)?;
    let result = match case.handler {
        Handler::SearchThoughts => search_thoughts_tool(&memory, &case.arguments),
        Handler::GetBrief => get_brief_tool(&memory, &case.arguments),
        Handler::SelectContext => select_context_tool(&memory, &case.arguments),
    };
    let output = result.json();
    let ranked_ids = ranked_ids(&case.handler, &output);
    let estimated_tokens = output["estimated_tokens"]
        .as_u64()
        .unwrap_or_else(|| estimate_brief_tokens(&result.text).max(0) as u64);
    let mut failures = Vec::new();
    if result.is_error && case.expected.error_category.is_none() {
        failures.push(format!("handler returned an error: {}", result.text));
    }
    if let Some(expected) = case.expected.error_category.as_deref()
        && output["error"].as_str() != Some(expected)
    {
        failures.push(format!(
            "error category differs: expected {expected}, got {:?}",
            output["error"].as_str()
        ));
    }
    if let Some(expected) = &case.expected.ranked_ids
        && &ranked_ids != expected
    {
        failures.push(format!(
            "ranked IDs differ: expected {expected:?}, got {ranked_ids:?}"
        ));
    }
    for forbidden in &case.expected.forbidden_ids {
        if ranked_ids.contains(forbidden) {
            failures.push(format!("forbidden ID returned: {forbidden}"));
        }
    }
    for (pointer, expected) in &case.expected.json_values {
        if output.pointer(pointer) != Some(expected) {
            failures.push(format!(
                "JSON value at {pointer} differs: expected {expected}, got {:?}",
                output.pointer(pointer)
            ));
        }
    }
    for (pointer, minimum) in &case.expected.minimum_numbers {
        let actual = output.pointer(pointer).and_then(Value::as_f64);
        if actual.is_none_or(|actual| actual < *minimum) {
            failures.push(format!(
                "JSON number at {pointer} is below {minimum}: got {actual:?}"
            ));
        }
    }
    for required in &case.expected.required_text {
        if !value_contains_text(&output, required) {
            failures.push(format!("required text missing: {required}"));
        }
    }
    for forbidden in &case.expected.forbidden_text {
        if value_contains_text(&output, forbidden) {
            failures.push(format!("forbidden text returned: {forbidden}"));
        }
    }
    if let Some(maximum) = case.expected.max_estimated_tokens
        && estimated_tokens > maximum
    {
        failures.push(format!(
            "estimated tokens exceed {maximum}: got {estimated_tokens}"
        ));
    }

    let relevant_ids = case.expected.ranked_ids.clone().unwrap_or_default();
    let ranks = relevant_ranks(&ranked_ids, &relevant_ids);
    Ok(CaseResult {
        id: case.id.clone(),
        suite: CONTRACT_SUITE.into(),
        category: case.category.clone(),
        passed: failures.is_empty(),
        ranked_ids,
        relevant_ranks: ranks,
        relevant_ids,
        forbidden_ids: case.expected.forbidden_ids.clone(),
        metrics: None,
        estimated_tokens,
        serialized_bytes: result.text.len() as u64,
        failures,
        output,
    })
}

fn run_hard_confuser_case(case: &HardConfuserCase) -> Result<CaseResult, ContractError> {
    let memory = Memory::open_in_memory()?;
    let target_project = format!("hc-{}", case.id);
    let confuser_project = match case.scenario {
        HardConfuserScenario::SameTopicWrongProject => format!("{target_project}-other"),
        _ => target_project.clone(),
    };
    for slug in [&target_project, &confuser_project] {
        upsert_project(
            &memory.conn,
            &ProjectSeed {
                slug: slug.clone(),
                display_name: slug.clone(),
                ..Default::default()
            },
        )?;
    }

    let relevant_id = stable_hard_confuser_id(&case.id, "relevant");
    let confuser_id = stable_hard_confuser_id(&case.id, "confuser");
    insert_hard_confuser_thought(
        &memory,
        &relevant_id,
        &case.relevant_content,
        &target_project,
        "2026-08-26T10:00:00Z",
        None,
    )?;
    insert_hard_confuser_thought(
        &memory,
        &confuser_id,
        &case.confuser_content,
        &confuser_project,
        "2025-08-26T10:00:00Z",
        matches!(case.scenario, HardConfuserScenario::StaleFactCurrentFact)
            .then_some(relevant_id.as_str()),
    )?;

    let (handler, result) = match case.scenario {
        HardConfuserScenario::StaleFactCurrentFact => (
            Handler::GetBrief,
            get_brief_tool(
                &memory,
                &serde_json::json!({
                    "project_identifier": target_project,
                    "scope": "full",
                    "now": "2026-08-26T12:00:00Z",
                    "token_budget": 900,
                }),
            ),
        ),
        _ => (
            Handler::SearchThoughts,
            search_thoughts_tool(
                &memory,
                &serde_json::json!({
                    "query": case.query,
                    "project_identifier": target_project,
                    "limit": 10,
                }),
            ),
        ),
    };
    let production_output = result.json();
    let ranked_ids = ranked_ids(&handler, &production_output);
    let expected_ids = vec![relevant_id.clone()];
    let mut failures = Vec::new();
    if result.is_error {
        failures.push(format!("handler returned an error: {}", result.text));
    }
    if ranked_ids != expected_ids {
        failures.push(format!(
            "ranked IDs differ: expected {expected_ids:?}, got {ranked_ids:?}"
        ));
    }
    if ranked_ids.contains(&confuser_id) {
        failures.push(format!("forbidden confuser returned: {confuser_id}"));
    }
    let relevant = BTreeSet::from([relevant_id.clone()]);
    let metrics = score_pr_ranking(&ranked_ids, &relevant)?;

    Ok(CaseResult {
        id: case.id.clone(),
        suite: HARD_CONFUSER_SUITE.into(),
        category: case.scenario.category().into(),
        passed: failures.is_empty(),
        ranked_ids: ranked_ids.clone(),
        relevant_ranks: relevant_ranks(&ranked_ids, &expected_ids),
        relevant_ids: vec![relevant_id],
        forbidden_ids: vec![confuser_id],
        metrics: Some(metrics),
        estimated_tokens: estimate_brief_tokens(&result.text).max(0) as u64,
        serialized_bytes: result.text.len() as u64,
        failures,
        output: serde_json::json!({
            "mode": production_output["mode"],
            "ranked_ids": ranked_ids,
        }),
    })
}

fn insert_hard_confuser_thought(
    memory: &Memory,
    id: &str,
    content: &str,
    project_identifier: &str,
    timestamp: &str,
    consolidated_into: Option<&str>,
) -> Result<(), ContractError> {
    let project_id = derive_existing_project_id(project_identifier);
    memory.conn.execute(
        "INSERT INTO thoughts (id, content, summary, type, source, trust_level, project_id, project_identifier,
           visibility, metadata, created_at, updated_at, consolidated_into)
         VALUES (?1, ?2, ?2, 'decision', 'hard-confuser', 'trusted', ?3, ?4, 'personal', '{}', ?5, ?5, ?6)",
        params![
            id,
            content,
            project_id,
            project_identifier,
            timestamp,
            consolidated_into,
        ],
    )?;
    Ok(())
}

fn stable_hard_confuser_id(case_id: &str, role: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        // The derivation input keeps its old name: it seeds the corpus's synthetic vectors,
        // so a new one would move every number the evaluation reports.
        format!("shelby:{case_id}:{role}").as_bytes(),
    )
    .to_string()
}

fn ranked_ids(handler: &Handler, output: &Value) -> Vec<String> {
    let key = match handler {
        Handler::SearchThoughts => "results",
        Handler::GetBrief => "items",
        Handler::SelectContext => return Vec::new(),
    };
    output[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect()
}

fn value_contains_text(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text.contains(needle),
        Value::Array(items) => items.iter().any(|item| value_contains_text(item, needle)),
        Value::Object(object) => object
            .values()
            .any(|value| value_contains_text(value, needle)),
        _ => false,
    }
}

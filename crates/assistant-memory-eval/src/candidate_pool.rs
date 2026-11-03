//! Hybrid candidate-pool experiment (Advisaur-Docs#548). Runs the production
//! `search_thoughts` handler over a seeded corpus at fixed limits while overriding only the
//! per-source hybrid pool, and records metrics, ranked pages and fused totals per pool.
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use assistant_memory::Memory;
use assistant_memory::identity::derive_existing_project_id;
use assistant_memory::projects::{ProjectSeed, upsert_project};
use assistant_memory::rusqlite::params;
use assistant_memory::tools::{search_thoughts_tool, search_thoughts_with_hybrid_pool};
use assistant_memory::vectors::embedding_to_bytes;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::manifest::{RelevantRank, relevant_ranks};
use crate::metrics::score_ranking;

pub const POOL_SCHEMA_VERSION: u32 = 1;
pub const FIXTURE_SOURCE: &str = "repository:tests/fixtures/Candidate Pool Corpus-v1.json";
pub const LIMITS: [usize; 2] = [5, 10];
pub const FIXED_POOLS: [usize; 3] = [30, 100, 200];
/// Case classes, reported separately: the small-project and large-corpus representative
/// cases, and the synthetic stress controls.
pub const CLASSES: [&str; 3] = ["representative-small", "representative-large", "stress"];
const CURRENT: &str = "current";
const COMPLETE: &str = "pool-all";

#[derive(Debug, Error)]
pub enum CandidatePoolError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Memory(#[from] assistant_memory::Error),
    #[error(transparent)]
    Sqlite(#[from] assistant_memory::rusqlite::Error),
    #[error(transparent)]
    Metric(#[from] crate::metrics::MetricError),
    #[error("unsupported candidate-pool schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("duplicate thought ID: {0}")]
    DuplicateThought(String),
    #[error("duplicate case ID: {0}")]
    DuplicateCase(String),
    #[error("thought {0} has an embedding of the wrong dimension")]
    EmbeddingDimension(String),
    #[error("unknown project slug on {owner}: {project}")]
    UnknownProject { owner: String, project: String },
    #[error("case {case_id} labels unknown thought ID: {thought_id}")]
    UnknownRelevantThought { case_id: String, thought_id: String },
    #[error("case {0} has an unknown class")]
    UnknownClass(String),
    #[error("case {0} must set exactly one of project_identifier and all_projects")]
    AmbiguousScope(String),
    #[error("the corpus must hold more than {0} thoughts")]
    CorpusTooSmall(usize),
    #[error("case {case_id} returned a handler error: {text}")]
    Handler { case_id: String, text: String },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolFixture {
    schema_version: u32,
    suite_version: String,
    embedding_dimensions: usize,
    projects: Vec<String>,
    thoughts: Vec<PoolThought>,
    cases: Vec<PoolCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolThought {
    id: String,
    content: String,
    #[serde(rename = "type")]
    kind: String,
    project_identifier: String,
    embedding: Vec<f32>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolCase {
    id: String,
    class: String,
    category: String,
    query: String,
    embedding: Vec<f32>,
    project_identifier: Option<String>,
    #[serde(default)]
    all_projects: bool,
    #[serde(rename = "type")]
    kind: Option<String>,
    relevant_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolFixtureProvenance {
    pub source: String,
    pub suite_version: String,
    pub sha256: String,
    pub documents: usize,
    pub embedding_dimensions: usize,
    pub vectors: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PoolMetrics {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub ndcg_at_5: f64,
    pub ndcg_at_10: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolObservation {
    pub case_id: String,
    pub class: String,
    pub category: String,
    pub limit: usize,
    pub policy: String,
    pub pool_per_source: usize,
    pub offset: usize,
    pub page_ids: Vec<String>,
    pub total_count: i64,
    pub has_more: bool,
    pub corpus_size: usize,
    /// Metrics and ranks describe the first page only, so they are absent on later pages.
    pub metrics: Option<PoolMetrics>,
    pub relevant_ranks: Vec<RelevantRank>,
    /// Each relevant thought's rank in the whole fused pool this policy builds, read by
    /// paging through it; `None` means the pool never admitted the thought.
    pub relevant_fused_ranks: Vec<RelevantRank>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolAggregate {
    pub class: String,
    pub limit: usize,
    pub policy: String,
    pub pool_per_source: usize,
    pub cases: usize,
    pub mean_fused_total: f64,
    pub mean: PoolMetrics,
    pub delta_vs_current: PoolMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideParity {
    pub checked: usize,
    pub identical: usize,
    pub mismatches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolRuntime {
    pub generated_at: String,
    pub duration_ms: u64,
    pub target: String,
    pub observation_duration_us: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidatePoolReport {
    pub schema_version: u32,
    pub code_sha: String,
    pub fixture: PoolFixtureProvenance,
    pub configuration: BTreeMap<String, String>,
    pub observations: Vec<PoolObservation>,
    pub aggregates: Vec<PoolAggregate>,
    pub override_parity: OverrideParity,
    pub runtime: PoolRuntime,
    pub deterministic_digest: String,
}

#[derive(Serialize)]
struct DeterministicPoolResult<'a> {
    schema_version: u32,
    fixture: &'a PoolFixtureProvenance,
    configuration: &'a BTreeMap<String, String>,
    observations: &'a [PoolObservation],
    aggregates: &'a [PoolAggregate],
    override_parity: &'a OverrideParity,
}

impl CandidatePoolReport {
    pub fn compute_digest(&self) -> Result<String, serde_json::Error> {
        let bytes = serde_json::to_vec(&DeterministicPoolResult {
            schema_version: self.schema_version,
            fixture: &self.fixture,
            configuration: &self.configuration,
            observations: &self.observations,
            aggregates: &self.aggregates,
            override_parity: &self.override_parity,
        })?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    /// The key `observation_duration_us` uses for one observation.
    pub fn runtime_key(observation: &PoolObservation) -> String {
        format!(
            "{}/{}/{}/{}",
            observation.case_id, observation.limit, observation.policy, observation.offset
        )
    }
}

/// The production per-source pool at `limit`, mirrored for labelling; parity checks that
/// the handler really uses it.
pub fn production_pool(limit: usize) -> usize {
    (limit * 3).min(100)
}

pub fn run_candidate_pool_experiment(
    input: &str,
    code_sha: String,
) -> Result<CandidatePoolReport, CandidatePoolError> {
    let started = Instant::now();
    let fixture: PoolFixture = serde_json::from_str(input)?;
    validate(&fixture)?;
    let corpus_size = fixture.thoughts.len();
    let mut policies: Vec<(String, Option<usize>)> = vec![(CURRENT.into(), None)];
    policies.extend(FIXED_POOLS.map(|pool| (format!("pool-{pool}"), Some(pool))));
    policies.push((COMPLETE.into(), Some(corpus_size)));

    let mut observations = Vec::new();
    let mut observation_duration_us = BTreeMap::new();
    let mut parity = OverrideParity {
        checked: 0,
        identical: 0,
        mismatches: Vec::new(),
    };
    for case in &fixture.cases {
        // One fresh store per case, as in the contract suite. Searches only append
        // telemetry, which no ranking reads, so every pool sees identical inputs.
        let memory = seeded_memory(&fixture)?;
        let relevant: BTreeSet<String> = case.relevant_ids.iter().cloned().collect();
        let mut fused_rankings: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for limit in LIMITS {
            for offset in [0, limit, limit * 6] {
                let args = case_arguments(case, limit, offset);
                for (policy, pool) in &policies {
                    let started = Instant::now();
                    let result = match pool {
                        None => search_thoughts_tool(&memory, &args),
                        Some(pool) => {
                            search_thoughts_with_hybrid_pool(&memory, &args, Some(*pool as i64))
                        }
                    };
                    let elapsed = started.elapsed().as_micros();
                    if result.is_error {
                        return Err(CandidatePoolError::Handler {
                            case_id: case.id.clone(),
                            text: result.text,
                        });
                    }
                    let output = result.json();
                    let page_ids = page_ids(&output);
                    let pool_per_source = pool.unwrap_or_else(|| production_pool(limit));
                    let (metrics, ranks, fused_ranks) = if offset == 0 {
                        let at_5 = score_ranking(&page_ids, &relevant, 5)?;
                        let at_10 = score_ranking(&page_ids, &relevant, 10)?;
                        (
                            Some(PoolMetrics {
                                recall_at_5: at_5.recall_at_k,
                                recall_at_10: at_10.recall_at_k,
                                ndcg_at_5: at_5.ndcg_at_k,
                                ndcg_at_10: at_10.ndcg_at_k,
                            }),
                            relevant_ranks(&page_ids, &case.relevant_ids),
                            relevant_ranks(
                                fused_ranking(&memory, case, pool_per_source, &mut fused_rankings)?,
                                &case.relevant_ids,
                            ),
                        )
                    } else {
                        (None, Vec::new(), Vec::new())
                    };
                    let observation = PoolObservation {
                        case_id: case.id.clone(),
                        class: case.class.clone(),
                        category: case.category.clone(),
                        limit,
                        policy: policy.clone(),
                        pool_per_source,
                        offset,
                        page_ids,
                        total_count: output["total_count"].as_i64().unwrap_or(-1),
                        has_more: output["has_more"].as_bool().unwrap_or(false),
                        corpus_size,
                        metrics,
                        relevant_ranks: ranks,
                        relevant_fused_ranks: fused_ranks,
                    };
                    observation_duration_us.insert(
                        CandidatePoolReport::runtime_key(&observation),
                        u64::try_from(elapsed).unwrap_or(u64::MAX),
                    );
                    observations.push(observation);
                }

                // The override path given the production pool must match the default path
                // byte for byte. This checks the override plumbing only; both paths run this
                // branch's handler, so it says nothing about parity with another revision.
                let production = search_thoughts_tool(&memory, &args);
                let explicit = search_thoughts_with_hybrid_pool(
                    &memory,
                    &args,
                    Some(production_pool(limit) as i64),
                );
                parity.checked += 1;
                if production == explicit {
                    parity.identical += 1;
                } else {
                    parity
                        .mismatches
                        .push(format!("{}/{limit}/{offset}", case.id));
                }
            }
        }
    }
    observations.sort_by(|a, b| {
        (&a.case_id, a.limit, policy_order(&a.policy), a.offset).cmp(&(
            &b.case_id,
            b.limit,
            policy_order(&b.policy),
            b.offset,
        ))
    });
    let aggregates = aggregate(&observations, &policies);

    let configuration = BTreeMap::from([
        ("metric_version".into(), "retrieval-v1".into()),
        ("handler".into(), "search_thoughts (hybrid)".into()),
        (
            "limits".into(),
            LIMITS.map(|limit| limit.to_string()).join(","),
        ),
        (
            "pools".into(),
            policies
                .iter()
                .map(|(policy, pool)| match pool {
                    None => format!("{policy}=min(3*limit,100)"),
                    Some(pool) => format!("{policy}={pool}"),
                })
                .collect::<Vec<_>>()
                .join(","),
        ),
        ("offsets".into(), "0,limit,6*limit".into()),
        ("production_pool".into(), "min(3*limit,100)".into()),
        ("rrf_k".into(), "60".into()),
        ("vector_threshold".into(), "0.3".into()),
    ]);
    let mut report = CandidatePoolReport {
        schema_version: POOL_SCHEMA_VERSION,
        code_sha,
        fixture: PoolFixtureProvenance {
            source: FIXTURE_SOURCE.into(),
            suite_version: fixture.suite_version.clone(),
            sha256: format!("{:x}", Sha256::digest(input.as_bytes())),
            documents: corpus_size,
            embedding_dimensions: fixture.embedding_dimensions,
            vectors: "fixture-supplied deterministic synthetic vectors; no embedding model".into(),
        },
        configuration,
        observations,
        aggregates,
        override_parity: parity,
        runtime: PoolRuntime {
            generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            target: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            observation_duration_us,
        },
        deterministic_digest: String::new(),
    };
    report.deterministic_digest = report.compute_digest()?;
    Ok(report)
}

fn validate(fixture: &PoolFixture) -> Result<(), CandidatePoolError> {
    if fixture.schema_version != 1 {
        return Err(CandidatePoolError::UnsupportedSchema(
            fixture.schema_version,
        ));
    }
    if fixture.thoughts.len() <= 200 {
        return Err(CandidatePoolError::CorpusTooSmall(200));
    }
    let projects: BTreeSet<&str> = fixture.projects.iter().map(String::as_str).collect();
    let mut thoughts = BTreeSet::new();
    for thought in &fixture.thoughts {
        if !thoughts.insert(thought.id.as_str()) {
            return Err(CandidatePoolError::DuplicateThought(thought.id.clone()));
        }
        if thought.embedding.len() != fixture.embedding_dimensions {
            return Err(CandidatePoolError::EmbeddingDimension(thought.id.clone()));
        }
        if !projects.contains(thought.project_identifier.as_str()) {
            return Err(CandidatePoolError::UnknownProject {
                owner: thought.id.clone(),
                project: thought.project_identifier.clone(),
            });
        }
    }
    let mut cases = BTreeSet::new();
    for case in &fixture.cases {
        if !cases.insert(case.id.as_str()) {
            return Err(CandidatePoolError::DuplicateCase(case.id.clone()));
        }
        if !CLASSES.contains(&case.class.as_str()) {
            return Err(CandidatePoolError::UnknownClass(case.id.clone()));
        }
        match (&case.project_identifier, case.all_projects) {
            (Some(project), false) if !projects.contains(project.as_str()) => {
                return Err(CandidatePoolError::UnknownProject {
                    owner: case.id.clone(),
                    project: project.clone(),
                });
            }
            (Some(_), false) | (None, true) => {}
            _ => return Err(CandidatePoolError::AmbiguousScope(case.id.clone())),
        }
        if case.embedding.len() != fixture.embedding_dimensions {
            return Err(CandidatePoolError::EmbeddingDimension(case.id.clone()));
        }
        for thought_id in &case.relevant_ids {
            if !thoughts.contains(thought_id.as_str()) {
                return Err(CandidatePoolError::UnknownRelevantThought {
                    case_id: case.id.clone(),
                    thought_id: thought_id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn seeded_memory(fixture: &PoolFixture) -> Result<Memory, CandidatePoolError> {
    let memory = Memory::open_in_memory()?;
    for slug in &fixture.projects {
        upsert_project(
            &memory.conn,
            &ProjectSeed {
                slug: slug.clone(),
                display_name: slug.clone(),
                ..Default::default()
            },
        )?;
    }
    let tx = memory.conn.unchecked_transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO thoughts (id, content, type, source, trust_level, project_id, project_identifier,
               visibility, metadata, embedding, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'candidate-pool', 'trusted', ?4, ?5, 'personal', '{}', ?6, ?7, ?7)",
        )?;
        for thought in &fixture.thoughts {
            insert.execute(params![
                thought.id,
                thought.content,
                thought.kind,
                derive_existing_project_id(&thought.project_identifier),
                thought.project_identifier,
                embedding_to_bytes(&thought.embedding),
                thought.created_at,
            ])?;
        }
    }
    tx.commit()?;
    Ok(memory)
}

fn case_arguments(case: &PoolCase, limit: usize, offset: usize) -> Value {
    let mut args = json!({
        "query": case.query,
        "embedding": case.embedding,
        "limit": limit,
        "offset": offset,
    });
    match &case.project_identifier {
        Some(project) => args["project_identifier"] = json!(project),
        None => args["all_projects"] = json!(true),
    }
    if let Some(kind) = &case.kind {
        args["type"] = json!(kind);
    }
    args
}

/// The whole fused pool for `pool` per source, in rank order, cached per pool. The pool is
/// fixed, so paging it at the handler's maximum limit reads one ordering.
fn fused_ranking<'a>(
    memory: &Memory,
    case: &PoolCase,
    pool: usize,
    cache: &'a mut BTreeMap<usize, Vec<String>>,
) -> Result<&'a [String], CandidatePoolError> {
    if let std::collections::btree_map::Entry::Vacant(entry) = cache.entry(pool) {
        let mut ranking = Vec::new();
        loop {
            let args = case_arguments(case, 100, ranking.len());
            let result = search_thoughts_with_hybrid_pool(memory, &args, Some(pool as i64));
            if result.is_error {
                return Err(CandidatePoolError::Handler {
                    case_id: case.id.clone(),
                    text: result.text,
                });
            }
            let page = page_ids(&result.json());
            if page.is_empty() {
                break;
            }
            ranking.extend(page);
        }
        entry.insert(ranking);
    }
    Ok(&cache[&pool])
}

fn page_ids(output: &Value) -> Vec<String> {
    output["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect()
}

fn policy_order(policy: &str) -> usize {
    match policy {
        CURRENT => 0,
        COMPLETE => usize::MAX,
        other => other
            .strip_prefix("pool-")
            .and_then(|pool| pool.parse().ok())
            .unwrap_or(usize::MAX - 1),
    }
}

fn aggregate(
    observations: &[PoolObservation],
    policies: &[(String, Option<usize>)],
) -> Vec<PoolAggregate> {
    let mut aggregates = Vec::new();
    for class in CLASSES {
        for limit in LIMITS {
            let mean_for = |policy: &str| {
                let first_pages: Vec<&PoolObservation> = observations
                    .iter()
                    .filter(|o| o.class == class && o.limit == limit && o.policy == policy)
                    .filter(|o| o.offset == 0)
                    .collect();
                let count = first_pages.len().max(1) as f64;
                let sum = |metric: fn(&PoolMetrics) -> f64| {
                    first_pages
                        .iter()
                        .filter_map(|o| o.metrics.as_ref().map(metric))
                        .sum::<f64>()
                        / count
                };
                (
                    first_pages.len(),
                    first_pages.first().map_or(0, |o| o.pool_per_source),
                    first_pages
                        .iter()
                        .map(|o| o.total_count as f64)
                        .sum::<f64>()
                        / count,
                    PoolMetrics {
                        recall_at_5: sum(|m| m.recall_at_5),
                        recall_at_10: sum(|m| m.recall_at_10),
                        ndcg_at_5: sum(|m| m.ndcg_at_5),
                        ndcg_at_10: sum(|m| m.ndcg_at_10),
                    },
                )
            };
            let (_, _, _, current) = mean_for(CURRENT);
            for (policy, _) in policies {
                let (cases, pool_per_source, mean_fused_total, mean) = mean_for(policy);
                if cases == 0 {
                    continue;
                }
                aggregates.push(PoolAggregate {
                    class: class.into(),
                    limit,
                    policy: policy.clone(),
                    pool_per_source,
                    cases,
                    mean_fused_total,
                    mean,
                    delta_vs_current: PoolMetrics {
                        recall_at_5: mean.recall_at_5 - current.recall_at_5,
                        recall_at_10: mean.recall_at_10 - current.recall_at_10,
                        ndcg_at_5: mean.ndcg_at_5 - current.ndcg_at_5,
                        ndcg_at_10: mean.ndcg_at_10 - current.ndcg_at_10,
                    },
                });
            }
        }
    }
    aggregates
}

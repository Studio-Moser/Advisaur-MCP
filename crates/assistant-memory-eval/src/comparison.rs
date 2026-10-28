use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::manifest::{RESULT_SCHEMA_VERSION, ResultManifest};

const FLOAT_EPSILON: f64 = 1e-12;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricFloor {
    pub recall_at_5: f64,
    pub ndcg_at_10: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceBudget {
    pub max_total_estimated_tokens: u64,
    pub max_total_serialized_bytes: u64,
    pub max_case_estimated_tokens: u64,
    pub max_case_serialized_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatePolicy {
    pub schema_version: u32,
    pub policy_version: u32,
    pub category_floors: BTreeMap<String, MetricFloor>,
    pub resources: ResourceBudget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricDelta {
    pub base: f64,
    pub candidate: f64,
    pub delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub passed: bool,
    pub base_code_sha: String,
    pub candidate_code_sha: String,
    pub base_digest: String,
    pub candidate_digest: String,
    pub failures: Vec<String>,
    pub metric_deltas: BTreeMap<String, MetricDelta>,
    pub case_changes: Vec<String>,
    pub case_diffs: Vec<CaseRankingDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelevantRankDelta {
    pub id: String,
    pub base_rank: Option<usize>,
    pub candidate_rank: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseRankingDiff {
    pub id: String,
    pub base_ranked_ids: Vec<String>,
    pub candidate_ranked_ids: Vec<String>,
    pub relevant_rank_deltas: Vec<RelevantRankDelta>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ComparisonError {
    #[error("{manifest} result manifest is missing the aggregate `{aggregate}`")]
    MissingAggregate {
        manifest: &'static str,
        aggregate: String,
    },
    #[error("{manifest} result manifest has an invalid deterministic digest")]
    InvalidDigest { manifest: &'static str },
    #[error("{manifest} result manifest uses unsupported schema version {version}")]
    UnsupportedSchema {
        manifest: &'static str,
        version: u32,
    },
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("unsupported gate policy schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("invalid gate policy: {0}")]
    Invalid(String),
}

pub fn load_policy(input: &str) -> Result<GatePolicy, PolicyError> {
    let policy: GatePolicy = serde_json::from_str(input)?;
    if policy.schema_version != 1 {
        return Err(PolicyError::UnsupportedSchema(policy.schema_version));
    }
    if policy.policy_version == 0 {
        return Err(PolicyError::Invalid(
            "policy version must be greater than zero".into(),
        ));
    }
    if policy.category_floors.is_empty() {
        return Err(PolicyError::Invalid(
            "at least one category floor is required".into(),
        ));
    }
    for (category, floor) in &policy.category_floors {
        if !(0.0..=1.0).contains(&floor.recall_at_5) || !(0.0..=1.0).contains(&floor.ndcg_at_10) {
            return Err(PolicyError::Invalid(format!(
                "metric floor for {category} must be between zero and one"
            )));
        }
    }
    let resources = &policy.resources;
    if resources.max_total_estimated_tokens == 0
        || resources.max_total_serialized_bytes == 0
        || resources.max_case_estimated_tokens == 0
        || resources.max_case_serialized_bytes == 0
    {
        return Err(PolicyError::Invalid(
            "resource budgets must be greater than zero".into(),
        ));
    }
    Ok(policy)
}

pub fn compare(
    base: &ResultManifest,
    candidate: &ResultManifest,
    candidate_repeat: &ResultManifest,
    policy: &GatePolicy,
) -> Result<ComparisonReport, ComparisonError> {
    validate_manifest("base", base)?;
    validate_manifest("candidate", candidate)?;
    validate_manifest("candidate repeat", candidate_repeat)?;

    let base_overall = aggregate("base", base, "overall")?;
    let candidate_overall = aggregate("candidate", candidate, "overall")?;
    let mut failures = Vec::new();

    let metric_deltas = BTreeMap::from([
        (
            "Recall@5".into(),
            MetricDelta {
                base: base_overall.recall_at_5,
                candidate: candidate_overall.recall_at_5,
                delta: candidate_overall.recall_at_5 - base_overall.recall_at_5,
            },
        ),
        (
            "NDCG@10".into(),
            MetricDelta {
                base: base_overall.ndcg_at_10,
                candidate: candidate_overall.ndcg_at_10,
                delta: candidate_overall.ndcg_at_10 - base_overall.ndcg_at_10,
            },
        ),
    ]);

    if candidate_overall.recall_at_5 + FLOAT_EPSILON < base_overall.recall_at_5 {
        failures.push(format!(
            "Recall@5 regressed from {:.6} to {:.6}",
            base_overall.recall_at_5, candidate_overall.recall_at_5
        ));
    }
    if candidate_overall.ndcg_at_10 + FLOAT_EPSILON < base_overall.ndcg_at_10 {
        failures.push(format!(
            "NDCG@10 regressed from {:.6} to {:.6}",
            base_overall.ndcg_at_10, candidate_overall.ndcg_at_10
        ));
    }

    if base.schema_version != candidate.schema_version
        || base.suite_version != candidate.suite_version
        || base.policy_version != candidate.policy_version
        || candidate.policy_version != policy.policy_version
        || base.provenance != candidate.provenance
        || base.configuration != candidate.configuration
    {
        failures.push("configuration drift between base, candidate, or policy".into());
    }

    if candidate.deterministic_digest != candidate_repeat.deterministic_digest {
        failures.push("candidate deterministic digest changed between repeated runs".into());
    }

    for case in candidate.cases.iter().filter(|case| !case.passed) {
        let suite = if case.suite == "shelby-contract" {
            "contract"
        } else {
            "public"
        };
        failures.push(format!("{suite} case {} failed", case.id));
    }

    let base_contracts: BTreeMap<_, _> = base
        .cases
        .iter()
        .filter(|case| case.suite.starts_with("shelby-"))
        .map(|case| (case.id.as_str(), case))
        .collect();
    let candidate_contracts: BTreeMap<_, _> = candidate
        .cases
        .iter()
        .filter(|case| case.suite.starts_with("shelby-"))
        .map(|case| (case.id.as_str(), case))
        .collect();
    for id in base_contracts
        .keys()
        .chain(candidate_contracts.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let unchanged = base_contracts
            .get(id)
            .zip(candidate_contracts.get(id))
            .is_some_and(|(base_case, candidate_case)| {
                base_case.suite == candidate_case.suite
                    && base_case.category == candidate_case.category
                    && base_case.passed == candidate_case.passed
                    && base_case.ranked_ids == candidate_case.ranked_ids
                    && base_case.relevant_ids == candidate_case.relevant_ids
                    && base_case.forbidden_ids == candidate_case.forbidden_ids
                    && base_case.metrics == candidate_case.metrics
                    && base_case.output == candidate_case.output
            });
        if !unchanged {
            failures.push(format!("contract output drift for {id}"));
        }
    }

    for (category, floor) in &policy.category_floors {
        let metrics = aggregate("candidate", candidate, category)?;
        if metrics.recall_at_5 + FLOAT_EPSILON < floor.recall_at_5 {
            failures.push(format!(
                "category floor failed for {category}: Recall@5 {:.6} < {:.6}",
                metrics.recall_at_5, floor.recall_at_5
            ));
        }
        if metrics.ndcg_at_10 + FLOAT_EPSILON < floor.ndcg_at_10 {
            failures.push(format!(
                "category floor failed for {category}: NDCG@10 {:.6} < {:.6}",
                metrics.ndcg_at_10, floor.ndcg_at_10
            ));
        }
    }

    check_resource_budgets(candidate, policy, &mut failures);

    let base_public: BTreeMap<_, _> = base
        .cases
        .iter()
        .filter(|case| case.suite == "longmemeval-pr")
        .map(|case| (&case.id, case))
        .collect();
    let case_diffs: Vec<CaseRankingDiff> = candidate
        .cases
        .iter()
        .filter(|case| case.suite == "longmemeval-pr")
        .filter_map(|case| {
            base_public
                .get(&case.id)
                .filter(|base_case| base_case.ranked_ids != case.ranked_ids)
                .map(|base_case| {
                    let relevant_ids: BTreeSet<_> = base_case
                        .relevant_ids
                        .iter()
                        .chain(&case.relevant_ids)
                        .cloned()
                        .collect();
                    CaseRankingDiff {
                        id: case.id.clone(),
                        base_ranked_ids: base_case.ranked_ids.clone(),
                        candidate_ranked_ids: case.ranked_ids.clone(),
                        relevant_rank_deltas: relevant_ids
                            .into_iter()
                            .map(|id| RelevantRankDelta {
                                base_rank: rank_of(&base_case.ranked_ids, &id),
                                candidate_rank: rank_of(&case.ranked_ids, &id),
                                id,
                            })
                            .collect(),
                    }
                })
        })
        .collect();
    let case_changes = case_diffs.iter().map(|change| change.id.clone()).collect();

    Ok(ComparisonReport {
        passed: failures.is_empty(),
        base_code_sha: base.code_sha.clone(),
        candidate_code_sha: candidate.code_sha.clone(),
        base_digest: base.deterministic_digest.clone(),
        candidate_digest: candidate.deterministic_digest.clone(),
        failures,
        metric_deltas,
        case_changes,
        case_diffs,
    })
}

fn rank_of(ranked_ids: &[String], id: &str) -> Option<usize> {
    ranked_ids
        .iter()
        .position(|ranked| ranked == id)
        .map(|index| index + 1)
}

fn validate_manifest(name: &'static str, manifest: &ResultManifest) -> Result<(), ComparisonError> {
    if manifest.schema_version != RESULT_SCHEMA_VERSION {
        return Err(ComparisonError::UnsupportedSchema {
            manifest: name,
            version: manifest.schema_version,
        });
    }
    if manifest.compute_digest().ok().as_ref() != Some(&manifest.deterministic_digest) {
        return Err(ComparisonError::InvalidDigest { manifest: name });
    }
    Ok(())
}

fn aggregate<'a>(
    name: &'static str,
    manifest: &'a ResultManifest,
    aggregate_name: &str,
) -> Result<&'a crate::metrics::RetrievalMetrics, ComparisonError> {
    manifest
        .aggregates
        .get(aggregate_name)
        .ok_or_else(|| ComparisonError::MissingAggregate {
            manifest: name,
            aggregate: aggregate_name.into(),
        })
}

fn check_resource_budgets(
    candidate: &ResultManifest,
    policy: &GatePolicy,
    failures: &mut Vec<String>,
) {
    let budget = &policy.resources;
    if candidate.efficiency.estimated_tokens > budget.max_total_estimated_tokens {
        failures.push(format!(
            "resource budget exceeded: total estimated tokens {} > {}",
            candidate.efficiency.estimated_tokens, budget.max_total_estimated_tokens
        ));
    }
    if candidate.efficiency.serialized_bytes > budget.max_total_serialized_bytes {
        failures.push(format!(
            "resource budget exceeded: total serialized bytes {} > {}",
            candidate.efficiency.serialized_bytes, budget.max_total_serialized_bytes
        ));
    }
    for case in &candidate.cases {
        if case.estimated_tokens > budget.max_case_estimated_tokens {
            failures.push(format!(
                "resource budget exceeded for {}: estimated tokens {} > {}",
                case.id, case.estimated_tokens, budget.max_case_estimated_tokens
            ));
        }
        if case.serialized_bytes > budget.max_case_serialized_bytes {
            failures.push(format!(
                "resource budget exceeded for {}: serialized bytes {} > {}",
                case.id, case.serialized_bytes, budget.max_case_serialized_bytes
            ));
        }
    }
}

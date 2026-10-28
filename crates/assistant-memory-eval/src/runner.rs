use std::collections::BTreeMap;

use thiserror::Error;

use crate::contract::ContractSuiteResult;
use crate::longmemeval::LongMemEvalSuiteResult;
use crate::manifest::{
    DatasetProvenance, Efficiency, RESULT_SCHEMA_VERSION, ResultManifest, RuntimeMetadata,
};
use crate::metrics::RetrievalMetrics;

#[derive(Debug, Clone)]
pub struct RunMetadata {
    pub code_sha: String,
    pub policy_version: u32,
    pub generated_at: String,
    pub duration_ms: u64,
    pub target: String,
    pub configuration: BTreeMap<String, String>,
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("public case {0} has no retrieval metrics")]
    MissingMetrics(String),
    #[error("the public suite has no cases")]
    EmptyPublicSuite,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Default)]
struct MetricAccumulator {
    count: u64,
    precision_at_5: f64,
    recall_at_5: f64,
    ndcg_at_10: f64,
    mrr_at_10: f64,
}

impl MetricAccumulator {
    fn push(&mut self, metrics: RetrievalMetrics) {
        self.count += 1;
        self.precision_at_5 += metrics.precision_at_5;
        self.recall_at_5 += metrics.recall_at_5;
        self.ndcg_at_10 += metrics.ndcg_at_10;
        self.mrr_at_10 += metrics.mrr_at_10;
    }

    fn mean(&self) -> RetrievalMetrics {
        let count = self.count as f64;
        RetrievalMetrics {
            precision_at_5: self.precision_at_5 / count,
            recall_at_5: self.recall_at_5 / count,
            ndcg_at_10: self.ndcg_at_10 / count,
            mrr_at_10: self.mrr_at_10 / count,
        }
    }
}

pub fn build_result_manifest(
    mut contract: ContractSuiteResult,
    mut public: LongMemEvalSuiteResult,
    provenance: Vec<DatasetProvenance>,
    metadata: RunMetadata,
) -> Result<ResultManifest, RunnerError> {
    if public.cases.is_empty() {
        return Err(RunnerError::EmptyPublicSuite);
    }

    let mut overall = MetricAccumulator::default();
    let mut categories: BTreeMap<String, MetricAccumulator> = BTreeMap::new();
    for case in &public.cases {
        let metrics = case
            .metrics
            .ok_or_else(|| RunnerError::MissingMetrics(case.id.clone()))?;
        overall.push(metrics);
        categories
            .entry(case.category.clone())
            .or_default()
            .push(metrics);
    }
    for case in contract
        .cases
        .iter()
        .filter(|case| case.suite == "shelby-hard-confuser")
    {
        let metrics = case
            .metrics
            .ok_or_else(|| RunnerError::MissingMetrics(case.id.clone()))?;
        categories
            .entry(case.category.clone())
            .or_default()
            .push(metrics);
    }

    let mut aggregates: BTreeMap<String, RetrievalMetrics> = categories
        .into_iter()
        .map(|(name, accumulator)| (name, accumulator.mean()))
        .collect();
    aggregates.insert("overall".into(), overall.mean());

    let suite_version = format!("{}+{}", contract.suite_version, public.suite_version);
    let mut case_duration_us = std::mem::take(&mut contract.case_duration_us);
    case_duration_us.extend(std::mem::take(&mut public.case_duration_us));
    let mut durations: Vec<u64> = case_duration_us.values().copied().collect();
    durations.sort_unstable();
    let median_case_duration_us = median(&durations);
    let p95_case_duration_us = percentile_95(&durations);
    let mut cases = contract.cases;
    cases.extend(public.cases);
    let efficiency = Efficiency {
        estimated_tokens: cases.iter().map(|case| case.estimated_tokens).sum(),
        serialized_bytes: cases.iter().map(|case| case.serialized_bytes).sum(),
    };

    let mut manifest = ResultManifest {
        schema_version: RESULT_SCHEMA_VERSION,
        code_sha: metadata.code_sha,
        suite_version,
        policy_version: metadata.policy_version,
        provenance,
        configuration: metadata.configuration,
        cases,
        aggregates,
        efficiency,
        runtime: RuntimeMetadata {
            generated_at: metadata.generated_at,
            duration_ms: metadata.duration_ms,
            target: metadata.target,
            case_duration_us,
            median_case_duration_us,
            p95_case_duration_us,
        },
        deterministic_digest: String::new(),
    };
    manifest.finalize()?;
    Ok(manifest)
}

fn median(sorted: &[u64]) -> u64 {
    match sorted.len() {
        0 => 0,
        length if length % 2 == 1 => sorted[length / 2],
        length => {
            let lower = sorted[length / 2 - 1];
            let upper = sorted[length / 2];
            lower + (upper - lower) / 2
        }
    }
}

fn percentile_95(sorted: &[u64]) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * 95).div_ceil(100);
    sorted[rank.saturating_sub(1)]
}

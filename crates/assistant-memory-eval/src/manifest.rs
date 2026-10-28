use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::metrics::RetrievalMetrics;

pub const RESULT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetProvenance {
    pub name: String,
    pub source: String,
    pub revision: String,
    pub sha256: String,
    pub license: String,
    pub license_url: Option<String>,
    pub selection_method: Option<String>,
    pub manifest_sha256: String,
    pub selected_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Efficiency {
    pub estimated_tokens: u64,
    pub serialized_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeMetadata {
    pub generated_at: String,
    pub duration_ms: u64,
    pub target: String,
    #[serde(default)]
    pub case_duration_us: BTreeMap<String, u64>,
    #[serde(default)]
    pub median_case_duration_us: u64,
    #[serde(default)]
    pub p95_case_duration_us: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseResult {
    pub id: String,
    pub suite: String,
    pub category: String,
    pub passed: bool,
    pub ranked_ids: Vec<String>,
    pub relevant_ids: Vec<String>,
    pub relevant_ranks: Vec<RelevantRank>,
    pub forbidden_ids: Vec<String>,
    pub metrics: Option<RetrievalMetrics>,
    pub estimated_tokens: u64,
    pub serialized_bytes: u64,
    pub failures: Vec<String>,
    pub output: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelevantRank {
    pub id: String,
    pub rank: Option<usize>,
}

pub fn relevant_ranks(ranked_ids: &[String], relevant_ids: &[String]) -> Vec<RelevantRank> {
    relevant_ids
        .iter()
        .map(|id| RelevantRank {
            id: id.clone(),
            rank: ranked_ids
                .iter()
                .position(|ranked| ranked == id)
                .map(|index| index + 1),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultManifest {
    pub schema_version: u32,
    pub code_sha: String,
    pub suite_version: String,
    pub policy_version: u32,
    pub provenance: Vec<DatasetProvenance>,
    pub configuration: BTreeMap<String, String>,
    pub cases: Vec<CaseResult>,
    pub aggregates: BTreeMap<String, RetrievalMetrics>,
    pub efficiency: Efficiency,
    pub runtime: RuntimeMetadata,
    pub deterministic_digest: String,
}

#[derive(Serialize)]
struct DeterministicResult<'a> {
    schema_version: u32,
    suite_version: &'a str,
    policy_version: u32,
    provenance: &'a [DatasetProvenance],
    configuration: &'a BTreeMap<String, String>,
    cases: &'a [CaseResult],
    aggregates: &'a BTreeMap<String, RetrievalMetrics>,
    efficiency: Efficiency,
}

impl ResultManifest {
    pub fn compute_digest(&self) -> Result<String, serde_json::Error> {
        let deterministic = DeterministicResult {
            schema_version: self.schema_version,
            suite_version: &self.suite_version,
            policy_version: self.policy_version,
            provenance: &self.provenance,
            configuration: &self.configuration,
            cases: &self.cases,
            aggregates: &self.aggregates,
            efficiency: self.efficiency,
        };
        let bytes = serde_json::to_vec(&deterministic)?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }

    pub fn finalize(&mut self) -> Result<(), serde_json::Error> {
        self.deterministic_digest = self.compute_digest()?;
        Ok(())
    }
}

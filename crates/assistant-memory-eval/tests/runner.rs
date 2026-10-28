use std::collections::BTreeMap;

use assistant_memory_eval::contract::ContractSuiteResult;
use assistant_memory_eval::longmemeval::LongMemEvalSuiteResult;
use assistant_memory_eval::manifest::{CaseResult, DatasetProvenance};
use assistant_memory_eval::metrics::RetrievalMetrics;
use assistant_memory_eval::runner::{RunMetadata, build_result_manifest};
use serde_json::json;

fn public_case(id: &str, category: &str, recall: f64, ndcg: f64) -> CaseResult {
    CaseResult {
        id: id.into(),
        suite: "longmemeval-pr".into(),
        category: category.into(),
        passed: true,
        ranked_ids: vec!["ranked".into()],
        relevant_ids: vec!["ranked".into()],
        relevant_ranks: vec![],
        forbidden_ids: vec![],
        metrics: Some(RetrievalMetrics {
            precision_at_5: 0.2,
            recall_at_5: recall,
            ndcg_at_10: ndcg,
            mrr_at_10: 0.5,
        }),
        estimated_tokens: 20,
        serialized_bytes: 80,
        failures: vec![],
        output: json!({"ranked_session_ids": ["ranked"]}),
    }
}

fn contract_case() -> CaseResult {
    CaseResult {
        id: "contract".into(),
        suite: "shelby-contract".into(),
        category: "scope".into(),
        passed: true,
        ranked_ids: vec!["thought".into()],
        relevant_ids: vec!["thought".into()],
        relevant_ranks: vec![],
        forbidden_ids: vec![],
        metrics: None,
        estimated_tokens: 10,
        serialized_bytes: 40,
        failures: vec![],
        output: json!({"ids": ["thought"]}),
    }
}

fn hard_confuser_case() -> CaseResult {
    let mut case = public_case("hard", "same-topic-wrong-project", 1.0, 1.0);
    case.suite = "shelby-hard-confuser".into();
    case
}

fn metadata(generated_at: &str, code_sha: &str) -> RunMetadata {
    RunMetadata {
        code_sha: code_sha.into(),
        policy_version: 1,
        generated_at: generated_at.into(),
        duration_ms: 50,
        target: "test".into(),
        configuration: BTreeMap::from([
            ("recall_cutoff".into(), "5".into()),
            ("ranking_cutoff".into(), "10".into()),
        ]),
    }
}

#[test]
fn manifest_averages_public_metrics_by_category_and_overall() {
    let manifest = build_result_manifest(
        ContractSuiteResult {
            suite_version: "contract-v1".into(),
            cases: vec![contract_case(), hard_confuser_case()],
            case_duration_us: BTreeMap::from([("contract".into(), 10), ("hard".into(), 15)]),
        },
        LongMemEvalSuiteResult {
            suite_version: "public-v1".into(),
            cases: vec![
                public_case("a", "single", 1.0, 1.0),
                public_case("b", "single", 0.0, 0.5),
                public_case("c", "multi", 0.5, 0.25),
            ],
            case_duration_us: BTreeMap::from([
                ("a".into(), 20),
                ("b".into(), 30),
                ("c".into(), 40),
            ]),
        },
        vec![DatasetProvenance {
            name: "public".into(),
            source: "pinned".into(),
            revision: "rev".into(),
            sha256: "hash".into(),
            license: "MIT".into(),
            license_url: None,
            selection_method: None,
            manifest_sha256: "manifest".into(),
            selected_ids: vec!["a".into(), "b".into(), "c".into()],
        }],
        metadata("2026-01-01T00:00:00Z", "sha-a"),
    )
    .unwrap();

    assert_eq!(manifest.suite_version, "contract-v1+public-v1");
    assert_eq!(manifest.cases.len(), 5);
    assert_eq!(manifest.aggregates["single"].recall_at_5, 0.5);
    assert_eq!(manifest.aggregates["multi"].ndcg_at_10, 0.25);
    assert_eq!(manifest.aggregates["overall"].recall_at_5, 0.5);
    assert_eq!(manifest.aggregates["overall"].ndcg_at_10, 7.0 / 12.0);
    assert_eq!(
        manifest.aggregates["same-topic-wrong-project"].recall_at_5,
        1.0
    );
    assert_eq!(manifest.efficiency.estimated_tokens, 90);
    assert_eq!(manifest.efficiency.serialized_bytes, 360);
    assert_eq!(manifest.runtime.median_case_duration_us, 20);
    assert_eq!(manifest.runtime.p95_case_duration_us, 40);
    assert_eq!(manifest.deterministic_digest.len(), 64);
}

#[test]
fn runtime_and_code_sha_do_not_change_the_deterministic_digest() {
    let build = |metadata| {
        build_result_manifest(
            ContractSuiteResult {
                suite_version: "contract-v1".into(),
                cases: vec![contract_case()],
                case_duration_us: BTreeMap::from([("contract".into(), 10)]),
            },
            LongMemEvalSuiteResult {
                suite_version: "public-v1".into(),
                cases: vec![public_case("a", "single", 1.0, 1.0)],
                case_duration_us: BTreeMap::from([("a".into(), 20)]),
            },
            vec![],
            metadata,
        )
        .unwrap()
    };
    let first = build(metadata("2026-01-01T00:00:00Z", "sha-a"));
    let second = build(metadata("2026-02-01T00:00:00Z", "sha-b"));

    assert_eq!(first.deterministic_digest, second.deterministic_digest);
}

#[test]
fn manifest_rejects_public_cases_without_metrics() {
    let mut case = public_case("a", "single", 1.0, 1.0);
    case.metrics = None;

    let error = build_result_manifest(
        ContractSuiteResult {
            suite_version: "contract-v1".into(),
            cases: vec![],
            case_duration_us: BTreeMap::new(),
        },
        LongMemEvalSuiteResult {
            suite_version: "public-v1".into(),
            cases: vec![case],
            case_duration_us: BTreeMap::from([("a".into(), 20)]),
        },
        vec![],
        metadata("now", "sha"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("a"));
}

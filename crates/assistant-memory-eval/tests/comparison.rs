use std::collections::BTreeMap;

use assistant_memory_eval::comparison::{
    GatePolicy, MetricFloor, ResourceBudget, compare, load_policy,
};
use assistant_memory_eval::longmemeval::load_manifest;
use assistant_memory_eval::manifest::{
    CaseResult, DatasetProvenance, Efficiency, ResultManifest, RuntimeMetadata,
};
use assistant_memory_eval::metrics::RetrievalMetrics;

fn metrics(recall: f64, ndcg: f64) -> RetrievalMetrics {
    RetrievalMetrics {
        precision_at_5: 0.2,
        recall_at_5: recall,
        ndcg_at_10: ndcg,
        mrr_at_10: 0.5,
    }
}

fn manifest(recall: f64, ndcg: f64) -> ResultManifest {
    let mut manifest = ResultManifest {
        schema_version: 1,
        code_sha: "sha".into(),
        suite_version: "pr-v1".into(),
        policy_version: 1,
        provenance: vec![DatasetProvenance {
            name: "fixture".into(),
            source: "repository".into(),
            revision: "v1".into(),
            sha256: "abc".into(),
            license: "MIT".into(),
            license_url: None,
            selection_method: None,
            manifest_sha256: "manifest".into(),
            selected_ids: vec!["public-1".into()],
        }],
        configuration: BTreeMap::from([("recall_cutoff".into(), "5".into())]),
        cases: vec![
            CaseResult {
                id: "contract-1".into(),
                suite: "contract".into(),
                category: "scope".into(),
                passed: true,
                ranked_ids: vec!["a".into()],
                relevant_ids: vec!["a".into()],
                relevant_ranks: vec![],
                forbidden_ids: vec![],
                metrics: None,
                estimated_tokens: 10,
                serialized_bytes: 40,
                failures: vec![],
                output: serde_json::json!({"ids": ["a"]}),
            },
            CaseResult {
                id: "public-1".into(),
                suite: "longmemeval-pr".into(),
                category: "single-session-user".into(),
                passed: true,
                ranked_ids: vec!["evidence".into()],
                relevant_ids: vec!["evidence".into()],
                relevant_ranks: vec![],
                forbidden_ids: vec![],
                metrics: Some(metrics(recall, ndcg)),
                estimated_tokens: 20,
                serialized_bytes: 80,
                failures: vec![],
                output: serde_json::json!({"ranked_session_ids": ["evidence"]}),
            },
        ],
        aggregates: BTreeMap::from([
            ("overall".into(), metrics(recall, ndcg)),
            ("single-session-user".into(), metrics(recall, ndcg)),
        ]),
        efficiency: Efficiency {
            estimated_tokens: 30,
            serialized_bytes: 120,
        },
        runtime: RuntimeMetadata {
            generated_at: "now".into(),
            duration_ms: 1,
            target: "test".into(),
            case_duration_us: BTreeMap::new(),
            median_case_duration_us: 0,
            p95_case_duration_us: 0,
        },
        deterministic_digest: String::new(),
    };
    manifest.finalize().unwrap();
    manifest
}

fn policy() -> GatePolicy {
    GatePolicy {
        schema_version: 1,
        policy_version: 1,
        category_floors: BTreeMap::from([(
            "single-session-user".into(),
            MetricFloor {
                recall_at_5: 0.0,
                ndcg_at_10: 0.0,
            },
        )]),
        resources: ResourceBudget {
            max_total_estimated_tokens: 100,
            max_total_serialized_bytes: 1_000,
            max_case_estimated_tokens: 50,
            max_case_serialized_bytes: 500,
        },
    }
}

#[test]
fn public_case_movement_is_reported_but_does_not_fail_equal_aggregate_quality() {
    let base = manifest(1.0, 1.0);
    let mut candidate = manifest(1.0, 1.0);
    candidate.cases[1].ranked_ids = vec!["distractor".into(), "evidence".into()];
    candidate.finalize().unwrap();
    let repeat = candidate.clone();

    let report = compare(&base, &candidate, &repeat, &policy()).unwrap();

    assert!(report.passed, "{:?}", report.failures);
    assert_eq!(report.case_changes, vec!["public-1"]);
    assert_eq!(report.case_diffs[0].base_ranked_ids, vec!["evidence"]);
    assert_eq!(
        report.case_diffs[0].candidate_ranked_ids,
        vec!["distractor", "evidence"]
    );
    assert_eq!(
        report.case_diffs[0].relevant_rank_deltas[0].candidate_rank,
        Some(2)
    );
}

#[test]
fn gate_rejects_primary_regressions_contract_failures_and_repeat_drift() {
    let base = manifest(1.0, 1.0);

    let candidate = manifest(0.5, 0.9);
    let report = compare(&base, &candidate, &candidate, &policy()).unwrap();
    assert!(!report.passed);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("Recall@5 regressed"))
    );
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("NDCG@10 regressed"))
    );

    let mut contract_failure = manifest(1.0, 1.0);
    contract_failure.cases[0].passed = false;
    contract_failure.cases[0].failures = vec!["wrong scope".into()];
    contract_failure.finalize().unwrap();
    let report = compare(&base, &contract_failure, &contract_failure, &policy()).unwrap();
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("contract-1"))
    );

    let mut contract_drift = manifest(1.0, 1.0);
    contract_drift.cases[0].output = serde_json::json!({"ids": ["changed"]});
    contract_drift.finalize().unwrap();
    let report = compare(&base, &contract_drift, &contract_drift, &policy()).unwrap();
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("contract output drift"))
    );

    let mut public_failure = manifest(1.0, 1.0);
    public_failure.cases[1].passed = false;
    public_failure.cases[1].failures = vec!["handler error".into()];
    public_failure.finalize().unwrap();
    let report = compare(&base, &public_failure, &public_failure, &policy()).unwrap();
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("public-1"))
    );

    let mut repeat = manifest(1.0, 1.0);
    repeat.cases[1].ranked_ids = vec!["changed".into()];
    repeat.finalize().unwrap();
    let report = compare(&base, &base, &repeat, &policy()).unwrap();
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("deterministic digest"))
    );
}

#[test]
fn gate_rejects_policy_floors_resource_budgets_and_configuration_drift() {
    let base = manifest(0.5, 0.5);
    let candidate = base.clone();

    let mut strict = policy();
    strict
        .category_floors
        .get_mut("single-session-user")
        .unwrap()
        .recall_at_5 = 0.75;
    let report = compare(&base, &candidate, &candidate, &strict).unwrap();
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("category floor"))
    );

    let mut oversized = candidate.clone();
    oversized.cases[1].estimated_tokens = 60;
    oversized.efficiency.estimated_tokens = 110;
    oversized.finalize().unwrap();
    let report = compare(&base, &oversized, &oversized, &policy()).unwrap();
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("resource budget"))
    );

    let mut drifted = candidate.clone();
    drifted
        .configuration
        .insert("recall_cutoff".into(), "10".into());
    drifted.finalize().unwrap();
    let report = compare(&base, &drifted, &drifted, &policy()).unwrap();
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.contains("configuration drift"))
    );
}

#[test]
fn policy_loader_rejects_unsupported_schema_versions() {
    let input = serde_json::to_string(&policy()).unwrap();
    assert_eq!(load_policy(&input).unwrap(), policy());

    let unsupported = input.replacen("\"schema_version\":1", "\"schema_version\":2", 1);
    assert!(
        load_policy(&unsupported)
            .unwrap_err()
            .to_string()
            .contains("schema version")
    );

    let invalid_floor = input.replacen("\"recall_at_5\":0.0", "\"recall_at_5\":1.5", 1);
    assert!(
        load_policy(&invalid_floor)
            .unwrap_err()
            .to_string()
            .contains("metric floor")
    );

    let unknown = input.replacen(
        "\"policy_version\":1",
        "\"unknown\":true,\"policy_version\":1",
        1,
    );
    assert!(
        load_policy(&unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );
}

#[test]
fn gate_rejects_a_validly_redigested_unsupported_result_schema() {
    let mut unsupported = manifest(1.0, 1.0);
    unsupported.schema_version = 2;
    unsupported.finalize().unwrap();

    let error = compare(&unsupported, &unsupported, &unsupported, &policy()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "base result manifest uses unsupported schema version 2"
    );
}

#[test]
fn committed_policy_covers_every_pinned_public_category() {
    let public = load_manifest(include_str!(
        "../../../tests/fixtures/LongMemEval PR-v1.json"
    ))
    .unwrap();
    let policy = load_policy(include_str!(
        "../../../tests/fixtures/Memory Eval Policy-v1.json"
    ))
    .unwrap();
    let categories: std::collections::BTreeSet<_> = public
        .cases
        .iter()
        .map(|case| case.question_type.as_str())
        .collect();
    let policy_categories: std::collections::BTreeSet<_> =
        policy.category_floors.keys().map(String::as_str).collect();

    assert!(categories.is_subset(&policy_categories));
    assert_eq!(
        policy_categories
            .difference(&categories)
            .copied()
            .collect::<Vec<_>>(),
        vec![
            "near-duplicate-decision",
            "same-person-wrong-event",
            "same-topic-wrong-project",
            "stale-fact-current-fact",
        ]
    );
}

use std::collections::BTreeMap;

use assistant_memory_eval::comparison::{
    CaseRankingDiff, ComparisonReport, MetricDelta, RelevantRankDelta,
};
use assistant_memory_eval::manifest::{CaseResult, Efficiency, ResultManifest, RuntimeMetadata};
use assistant_memory_eval::metrics::RetrievalMetrics;
use assistant_memory_eval::report::{render_comparison_report, render_run_report};
use serde_json::json;

fn manifest() -> ResultManifest {
    let metrics = RetrievalMetrics {
        precision_at_5: 0.2,
        recall_at_5: 0.75,
        ndcg_at_10: 0.625,
        mrr_at_10: 0.5,
    };
    let mut manifest = ResultManifest {
        schema_version: 1,
        code_sha: "abc".into(),
        suite_version: "pr-v1".into(),
        policy_version: 1,
        provenance: vec![],
        configuration: BTreeMap::new(),
        cases: vec![CaseResult {
            id: "failed-contract".into(),
            suite: "shelby-contract".into(),
            category: "scope".into(),
            passed: false,
            ranked_ids: vec![],
            relevant_ids: vec![],
            relevant_ranks: vec![],
            forbidden_ids: vec![],
            metrics: None,
            estimated_tokens: 10,
            serialized_bytes: 40,
            failures: vec!["wrong project".into()],
            output: json!({}),
        }],
        aggregates: BTreeMap::from([("overall".into(), metrics)]),
        efficiency: Efficiency {
            estimated_tokens: 10,
            serialized_bytes: 40,
        },
        runtime: RuntimeMetadata {
            generated_at: "now".into(),
            duration_ms: 2,
            target: "test".into(),
            case_duration_us: BTreeMap::from([("failed-contract".into(), 1_500)]),
            median_case_duration_us: 1_500,
            p95_case_duration_us: 1_500,
        },
        deterministic_digest: String::new(),
    };
    manifest.finalize().unwrap();
    manifest
}

#[test]
fn run_report_surfaces_primary_metrics_efficiency_and_failures() {
    let report = render_run_report(&manifest()).unwrap();

    assert!(report.contains("Recall@5 | 0.750000"));
    assert!(report.contains("NDCG@10 | 0.625000"));
    assert!(report.contains("Estimated tokens | 10"));
    assert!(report.contains("Median case time | 1.500 ms"));
    assert!(report.contains("p95 case time | 1.500 ms"));
    assert!(report.contains("failed-contract: wrong project"));
}

#[test]
fn comparison_report_surfaces_gate_failures_deltas_and_case_changes() {
    let report = render_comparison_report(&ComparisonReport {
        passed: false,
        base_code_sha: "base-sha".into(),
        candidate_code_sha: "candidate-sha".into(),
        base_digest: "base-digest".into(),
        candidate_digest: "candidate-digest".into(),
        failures: vec!["Recall@5 regressed".into()],
        metric_deltas: BTreeMap::from([(
            "Recall@5".into(),
            MetricDelta {
                base: 0.75,
                candidate: 0.5,
                delta: -0.25,
            },
        )]),
        case_changes: vec!["public-1".into()],
        case_diffs: vec![CaseRankingDiff {
            id: "public-1".into(),
            base_ranked_ids: vec!["evidence".into()],
            candidate_ranked_ids: vec!["distractor".into(), "evidence".into()],
            relevant_rank_deltas: vec![RelevantRankDelta {
                id: "evidence".into(),
                base_rank: Some(1),
                candidate_rank: Some(2),
            }],
        }],
    });

    assert!(report.contains("Status: **FAIL**"));
    assert!(report.contains("Base code: `base-sha`"));
    assert!(report.contains("Candidate code: `candidate-sha`"));
    assert!(report.contains("Recall@5 regressed"));
    assert!(report.contains("0.750000 | 0.500000 | -0.250000"));
    assert!(report.contains("public-1"));
    assert!(report.contains("candidate rank Some(2)"));
}

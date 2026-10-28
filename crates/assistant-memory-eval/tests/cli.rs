use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use assistant_memory_eval::comparison::{GatePolicy, MetricFloor, ResourceBudget};
use assistant_memory_eval::manifest::{
    CaseResult, DatasetProvenance, Efficiency, ResultManifest, RuntimeMetadata,
};
use assistant_memory_eval::metrics::RetrievalMetrics;
use serde_json::json;
use uuid::Uuid;

fn manifest() -> ResultManifest {
    let metrics = RetrievalMetrics {
        precision_at_5: 0.2,
        recall_at_5: 1.0,
        ndcg_at_10: 1.0,
        mrr_at_10: 1.0,
    };
    let mut result = ResultManifest {
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
            selected_ids: vec!["public".into()],
        }],
        configuration: BTreeMap::new(),
        cases: vec![CaseResult {
            id: "public".into(),
            suite: "longmemeval-pr".into(),
            category: "single".into(),
            passed: true,
            ranked_ids: vec!["evidence".into()],
            relevant_ids: vec!["evidence".into()],
            relevant_ranks: vec![],
            forbidden_ids: vec![],
            metrics: Some(metrics),
            estimated_tokens: 10,
            serialized_bytes: 40,
            failures: vec![],
            output: json!({"ranked_session_ids": ["evidence"]}),
        }],
        aggregates: BTreeMap::from([("overall".into(), metrics), ("single".into(), metrics)]),
        efficiency: Efficiency {
            estimated_tokens: 10,
            serialized_bytes: 40,
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
    result.finalize().unwrap();
    result
}

fn policy() -> GatePolicy {
    GatePolicy {
        schema_version: 1,
        policy_version: 1,
        category_floors: BTreeMap::from([(
            "single".into(),
            MetricFloor {
                recall_at_5: 1.0,
                ndcg_at_10: 1.0,
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
fn help_documents_the_offline_fetch_run_compare_workflow() {
    let output = Command::new(env!("CARGO_BIN_EXE_assistant-memory-eval"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("fetch longmemeval-pr"));
    assert!(stdout.contains("run --suite pr"));
    assert!(stdout.contains("compare --base"));
}

#[test]
fn compare_command_writes_machine_and_human_readable_artifacts() {
    let directory = std::env::temp_dir().join(format!("shelby-eval-cli-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    let result = serde_json::to_vec_pretty(&manifest()).unwrap();
    for name in ["base.json", "candidate.json", "repeat.json"] {
        fs::write(directory.join(name), &result).unwrap();
    }
    fs::write(
        directory.join("policy.json"),
        serde_json::to_vec_pretty(&policy()).unwrap(),
    )
    .unwrap();
    let output_directory = directory.join("output");

    let status = Command::new(env!("CARGO_BIN_EXE_assistant-memory-eval"))
        .args([
            "compare",
            "--base",
            directory.join("base.json").to_str().unwrap(),
            "--candidate",
            directory.join("candidate.json").to_str().unwrap(),
            "--candidate-repeat",
            directory.join("repeat.json").to_str().unwrap(),
            "--policy",
            directory.join("policy.json").to_str().unwrap(),
            "--output",
            output_directory.to_str().unwrap(),
        ])
        .status()
        .unwrap();

    assert!(status.success());
    let comparison: serde_json::Value =
        serde_json::from_slice(&fs::read(output_directory.join("comparison.json")).unwrap())
            .unwrap();
    assert_eq!(comparison["passed"], true);
    assert!(
        fs::read_to_string(output_directory.join("comparison.md"))
            .unwrap()
            .contains("Status: **PASS**")
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn run_with_a_missing_cache_fails_offline_without_creating_dataset_data() {
    let directory = std::env::temp_dir().join(format!("shelby-eval-offline-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    let missing = directory.join("missing.json");
    let output = Command::new(env!("CARGO_BIN_EXE_assistant-memory-eval"))
        .args([
            "run",
            "--suite",
            "pr",
            "--dataset",
            missing.to_str().unwrap(),
            "--output",
            directory.join("output").to_str().unwrap(),
        ])
        .env("PATH", "/definitely-not-a-real-bin")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(!missing.exists(), "run must not download a missing dataset");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("No such file")
    );
    fs::remove_dir_all(directory).unwrap();
}

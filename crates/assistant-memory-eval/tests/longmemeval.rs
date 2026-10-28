use std::collections::BTreeMap;
use std::path::PathBuf;

use assistant_memory_eval::longmemeval::{
    load_manifest, load_selected_dataset, run_longmemeval_suite, verify_cache,
};

const DATASET: &str = "[{\"question_id\":\"q1\",\"question_type\":\"single-session-user\",\"question\":\"Where?\",\"haystack_session_ids\":[\"answer_s1\"],\"haystack_dates\":[\"2024/01/01\"],\"haystack_sessions\":[[{\"role\":\"user\",\"content\":\"At home\",\"has_answer\":true},{\"role\":\"assistant\",\"content\":\"Okay\"}]],\"answer_session_ids\":[\"answer_s1\"]}]";
const DATASET_SHA: &str = "f1713010f8249f56afece81bf1d292da48e4dd6544c150ccd8850b60b2326117";
const DUPLICATE_SESSION_DATASET: &str = "[{\"question_id\":\"q1\",\"question_type\":\"single-session-user\",\"question\":\"Where?\",\"haystack_session_ids\":[\"answer_s1\",\"answer_s1\"],\"haystack_dates\":[\"2024/01/01\",\"2024/01/02\"],\"haystack_sessions\":[[{\"role\":\"user\",\"content\":\"At home\",\"has_answer\":true},{\"role\":\"assistant\",\"content\":\"Okay\"}],[{\"role\":\"user\",\"content\":\"Distractor\",\"has_answer\":false}]],\"answer_session_ids\":[\"answer_s1\"]}]";
const DUPLICATE_SESSION_SHA: &str =
    "152d9b896861d7c6ca8a6627193af238b873987e3b6e632fb80c4f7480882b87";
const PINNED_MANIFEST: &str = include_str!("../../../tests/fixtures/LongMemEval PR-v1.json");

fn manifest(license: &str, sha: &str, question_id: &str) -> String {
    format!(
        r#"{{
          "schema_version": 1,
          "suite_version": "longmemeval-pr-v1",
          "selection_method": "test fixture",
          "dataset": {{
            "name": "LongMemEval-S cleaned",
            "source": "https://example.test/resolve/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/data.json",
            "revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "sha256": "{sha}",
            "license": "{license}",
            "license_url": "https://example.test/license"
          }},
          "cases": [{{
            "question_id": "{question_id}",
            "question_type": "single-session-user",
            "evidence_session_ids": ["answer_s1"],
            "evidence_turn_ids": ["answer_s1_1"]
          }}]
        }}"#
    )
}

fn dataset_file() -> PathBuf {
    dataset_file_with(DATASET)
}

fn dataset_file_with(content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("assistant-lme-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn cache_validation_fails_closed_on_license_and_checksum() {
    let path = dataset_file();

    let valid = load_manifest(&manifest("MIT", DATASET_SHA, "q1")).unwrap();
    verify_cache(&valid, &path).expect("pinned MIT cache");

    let unapproved = load_manifest(&manifest("proprietary", DATASET_SHA, "q1")).unwrap();
    assert_eq!(
        verify_cache(&unapproved, &path).unwrap_err().to_string(),
        "dataset license is not approved: proprietary"
    );

    let mismatch = load_manifest(&manifest("MIT", &"0".repeat(64), "q1")).unwrap();
    assert_eq!(
        verify_cache(&mismatch, &path).unwrap_err().to_string(),
        format!(
            "dataset checksum mismatch: expected {}, got {DATASET_SHA}",
            "0".repeat(64)
        )
    );

    std::fs::remove_file(path).unwrap();
}

#[test]
fn manifest_validation_rejects_mutable_or_duplicate_selection() {
    let mutable = manifest("MIT", DATASET_SHA, "q1").replace(&"a".repeat(40), "main");
    assert_eq!(
        load_manifest(&mutable).unwrap_err().to_string(),
        "dataset revision must be a 40-character lowercase hexadecimal commit SHA"
    );

    let mut duplicate: serde_json::Value =
        serde_json::from_str(&manifest("MIT", DATASET_SHA, "q1")).unwrap();
    let repeated = duplicate["cases"][0].clone();
    duplicate["cases"].as_array_mut().unwrap().push(repeated);
    assert_eq!(
        load_manifest(&duplicate.to_string())
            .unwrap_err()
            .to_string(),
        "duplicate selected question ID: q1"
    );
}

#[test]
fn manifest_validation_rejects_unsupported_schemas_and_unknown_fields() {
    let unsupported = manifest("MIT", DATASET_SHA, "q1").replacen(
        "\"schema_version\": 1",
        "\"schema_version\": 2",
        1,
    );
    assert_eq!(
        load_manifest(&unsupported).unwrap_err().to_string(),
        "unsupported LongMemEval manifest schema version: 2"
    );

    let unknown = manifest("MIT", DATASET_SHA, "q1").replacen(
        "\"suite_version\":",
        "\"unexpected\": true, \"suite_version\":",
        1,
    );
    assert!(
        load_manifest(&unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );
}

#[test]
fn adapter_streams_selected_cases_through_the_production_search_handler() {
    let path = dataset_file();
    let pinned = load_manifest(&manifest("MIT", DATASET_SHA, "q1")).unwrap();

    let selected = load_selected_dataset(&pinned, &path).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].question_id, "q1");

    let result = run_longmemeval_suite(&pinned, &path).unwrap();
    assert_eq!(result.cases.len(), 1);
    assert!(result.cases[0].passed);
    assert_eq!(result.cases[0].metrics.unwrap().recall_at_5, 0.0);
    assert!(
        !result.cases[0].output.to_string().contains("At home"),
        "result artifacts must not redistribute conversation text"
    );

    let missing = load_manifest(&manifest("MIT", DATASET_SHA, "q2")).unwrap();
    assert_eq!(
        load_selected_dataset(&missing, &path)
            .unwrap_err()
            .to_string(),
        "selected question is missing from the dataset: q2"
    );

    std::fs::remove_file(path).unwrap();
}

#[test]
fn adapter_handles_repeated_upstream_session_ids_without_storage_collisions() {
    let path = dataset_file_with(DUPLICATE_SESSION_DATASET);
    let manifest = load_manifest(&manifest("MIT", DUPLICATE_SESSION_SHA, "q1")).unwrap();

    let result = run_longmemeval_suite(&manifest, &path).unwrap();

    assert_eq!(result.cases.len(), 1);
    assert!(result.cases[0].passed);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn pinned_manifest_has_six_evidence_bearing_cases_per_question_type() {
    let manifest = load_manifest(PINNED_MANIFEST).unwrap();
    assert!(
        manifest
            .selection_method
            .to_lowercase()
            .contains("six evidence-bearing cases")
    );
    assert_eq!(
        manifest.dataset.license_url,
        "https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned"
    );
    let mut counts = BTreeMap::new();
    for case in &manifest.cases {
        *counts.entry(case.question_type.as_str()).or_insert(0) += 1;
        assert!(
            !case.evidence_session_ids.is_empty(),
            "{}",
            case.question_id
        );
        assert!(!case.evidence_turn_ids.is_empty(), "{}", case.question_id);
    }

    assert_eq!(manifest.cases.len(), 36);
    assert_eq!(counts.values().copied().collect::<Vec<_>>(), vec![6; 6]);
    assert!(manifest.dataset.source.contains(&manifest.dataset.revision));
    assert_eq!(
        manifest.dataset.sha256,
        "d6f21ea9d60a0d56f34a05b609c79c88a451d2ae03597821ea3d5a9678c3a442"
    );
}

#[test]
#[ignore = "requires the externally cached LongMemEval-S dataset"]
fn pinned_dataset_matches_and_runs_all_selected_cases() {
    let path = std::env::var("ASSISTANT_LONGMEMEVAL_DATA")
        .expect("set ASSISTANT_LONGMEMEVAL_DATA to the verified external JSON file");
    let manifest = load_manifest(PINNED_MANIFEST).unwrap();

    let result = run_longmemeval_suite(&manifest, PathBuf::from(path).as_path()).unwrap();

    assert_eq!(result.cases.len(), 36);
    assert!(result.cases.iter().all(|case| case.passed));
}

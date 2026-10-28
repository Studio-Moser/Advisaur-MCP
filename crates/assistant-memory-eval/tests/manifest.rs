use std::collections::BTreeMap;

use assistant_memory_eval::manifest::{
    CaseResult, DatasetProvenance, Efficiency, ResultManifest, RuntimeMetadata,
};

fn sample_manifest() -> ResultManifest {
    ResultManifest {
        schema_version: 1,
        code_sha: "candidate".into(),
        suite_version: "pr-v1".into(),
        policy_version: 1,
        provenance: vec![DatasetProvenance {
            name: "shelby-contract".into(),
            source: "repository".into(),
            revision: "v1".into(),
            sha256: "abc".into(),
            license: "MIT".into(),
            license_url: None,
            selection_method: None,
            manifest_sha256: "manifest".into(),
            selected_ids: vec!["scope-1".into()],
        }],
        configuration: BTreeMap::from([("cutoff".into(), "5".into())]),
        cases: vec![CaseResult {
            id: "scope-1".into(),
            suite: "shelby-contract".into(),
            category: "scope".into(),
            passed: true,
            ranked_ids: vec!["expected".into()],
            relevant_ids: vec!["expected".into()],
            relevant_ranks: vec![],
            forbidden_ids: vec![],
            metrics: None,
            estimated_tokens: 4,
            serialized_bytes: 16,
            failures: vec![],
            output: serde_json::json!({"items": ["expected"]}),
        }],
        aggregates: BTreeMap::new(),
        efficiency: Efficiency {
            estimated_tokens: 4,
            serialized_bytes: 16,
        },
        runtime: RuntimeMetadata {
            generated_at: "2026-08-26T10:00:00Z".into(),
            duration_ms: 12,
            target: "aarch64-apple-darwin".into(),
            case_duration_us: BTreeMap::new(),
            median_case_duration_us: 0,
            p95_case_duration_us: 0,
        },
        deterministic_digest: String::new(),
    }
}

#[test]
fn digest_excludes_runtime_noise_but_catches_behavior_changes() {
    let first = sample_manifest();
    let mut noisy = sample_manifest();
    noisy.runtime.generated_at = "2026-08-27T10:00:00Z".into();
    noisy.runtime.duration_ms = 9_999;
    noisy.code_sha = "different-code-same-output".into();

    assert_eq!(
        first.compute_digest().unwrap(),
        noisy.compute_digest().unwrap()
    );

    noisy.cases[0].ranked_ids = vec!["wrong".into()];
    assert_ne!(
        first.compute_digest().unwrap(),
        noisy.compute_digest().unwrap()
    );
}

#[test]
fn finalize_records_the_digest_in_serialized_results() {
    let mut manifest = sample_manifest();
    manifest.finalize().unwrap();

    assert_eq!(manifest.deterministic_digest.len(), 64);
    let json = serde_json::to_string(&manifest).unwrap();
    let decoded: ResultManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(
        decoded.deterministic_digest,
        manifest.compute_digest().unwrap()
    );
}

#[test]
fn result_manifest_decoding_rejects_unknown_top_level_and_nested_fields() {
    let mut top_level = serde_json::to_value(sample_manifest()).unwrap();
    top_level["unknown"] = true.into();
    assert!(
        serde_json::from_value::<ResultManifest>(top_level)
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );

    let mut nested = serde_json::to_value(sample_manifest()).unwrap();
    nested["cases"][0]["unknown"] = true.into();
    assert!(
        serde_json::from_value::<ResultManifest>(nested)
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );
}

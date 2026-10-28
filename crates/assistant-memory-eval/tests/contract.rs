use std::collections::BTreeMap;

use assistant_memory_eval::contract::run_contract_suite;

const FIXTURE: &str = include_str!("../../../tests/fixtures/Contract Corpus-v1.json");

#[test]
fn contract_suite_reseeds_each_case_in_a_fresh_store_and_meets_exact_expectations() {
    let result = run_contract_suite(FIXTURE).expect("valid contract suite");

    assert_eq!(result.suite_version, "contract-v1");
    assert_eq!(result.cases.len(), 58);
    assert!(
        result.cases.iter().all(|case| case.passed),
        "{:#?}",
        result
            .cases
            .iter()
            .filter(|case| !case.passed)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        result
            .cases
            .iter()
            .filter(|case| case.suite == "contract")
            .map(|case| case.output["mode"].as_str().unwrap_or("non-search"))
            .collect::<Vec<_>>(),
        vec![
            "fts",
            "vector",
            "non-search",
            "non-search",
            "non-search",
            "non-search"
        ]
    );

    let mut hard_confuser_counts = BTreeMap::new();
    for case in result
        .cases
        .iter()
        .filter(|case| case.suite == "hard-confuser")
    {
        *hard_confuser_counts
            .entry(case.category.as_str())
            .or_insert(0) += 1;
        let metrics = case.metrics.expect("hard-confuser retrieval metrics");
        assert_eq!(metrics.recall_at_5, 1.0, "{}", case.id);
        assert_eq!(metrics.ndcg_at_10, 1.0, "{}", case.id);
    }
    assert_eq!(
        hard_confuser_counts,
        BTreeMap::from([
            ("near-duplicate-decision", 13),
            ("same-person-wrong-event", 13),
            ("same-topic-wrong-project", 13),
            ("stale-fact-current-fact", 13),
        ])
    );
}

#[test]
fn contract_fixture_validation_rejects_duplicate_thought_ids() {
    let mut fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    let duplicate = fixture["thoughts"][0].clone();
    fixture["thoughts"].as_array_mut().unwrap().push(duplicate);

    let error = run_contract_suite(&fixture.to_string()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "duplicate thought ID: 00000000-0000-4000-8000-000000000001"
    );
}

#[test]
fn contract_fixture_validation_rejects_invalid_timestamps_and_expectations() {
    let mut bad_time: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    bad_time["thoughts"][0]["created_at"] = "yesterday".into();
    assert_eq!(
        run_contract_suite(&bad_time.to_string())
            .unwrap_err()
            .to_string(),
        "invalid timestamp on thought 00000000-0000-4000-8000-000000000001: yesterday"
    );

    let mut unknown_expected: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    unknown_expected["cases"][0]["expected"]["ranked_ids"][0] = "missing".into();
    assert_eq!(
        run_contract_suite(&unknown_expected.to_string())
            .unwrap_err()
            .to_string(),
        "case search-project-scope expects unknown thought ID: missing"
    );
}

#[test]
fn contract_fixture_rejects_unsupported_schema_versions() {
    let unsupported = FIXTURE.replacen("\"schema_version\": 1", "\"schema_version\": 2", 1);
    assert_eq!(
        run_contract_suite(&unsupported).unwrap_err().to_string(),
        "unsupported contract schema version: 2"
    );

    let unknown = FIXTURE.replacen(
        "\"suite_version\":",
        "\"unknown\": true, \"suite_version\":",
        1,
    );
    assert!(
        run_contract_suite(&unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );
}

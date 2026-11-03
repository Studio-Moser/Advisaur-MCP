use std::collections::BTreeSet;
use std::sync::LazyLock;

use assistant_memory_eval::candidate_pool::{
    CandidatePoolReport, LIMITS, PoolObservation, run_candidate_pool_experiment,
};
use assistant_memory_eval::report::render_candidate_pool_report;

const FIXTURE: &str = include_str!("../../../tests/fixtures/Candidate Pool Corpus-v1.json");

static REPORT: LazyLock<CandidatePoolReport> = LazyLock::new(|| {
    run_candidate_pool_experiment(FIXTURE, "test-sha".into()).expect("valid candidate-pool run")
});

fn observation<'a>(
    case_id: &str,
    limit: usize,
    policy: &str,
    offset: usize,
) -> &'a PoolObservation {
    REPORT
        .observations
        .iter()
        .find(|o| {
            o.case_id == case_id && o.limit == limit && o.policy == policy && o.offset == offset
        })
        .unwrap_or_else(|| panic!("missing {case_id}/{limit}/{policy}/{offset}"))
}

#[test]
fn candidate_pool_default_policy_reproduces_the_production_handler() {
    let parity = &REPORT.production_parity;
    assert!(parity.checked > 0);
    assert_eq!(parity.identical, parity.checked, "{:?}", parity.mismatches);
    for limit in LIMITS {
        let current = observation("stress-tail-025", limit, "current", 0);
        assert_eq!(current.pool_per_source, (limit * 3).min(100));
    }
}

#[test]
fn candidate_pool_report_covers_every_pool_limit_case_and_offset() {
    let cases: BTreeSet<&str> = REPORT
        .observations
        .iter()
        .map(|o| o.case_id.as_str())
        .collect();
    let policies = ["current", "pool-30", "pool-100", "pool-200", "pool-all"];
    for case in &cases {
        for limit in LIMITS {
            for policy in policies {
                for offset in [0, limit, limit * 6] {
                    observation(case, limit, policy, offset);
                }
                let first_page = observation(case, limit, policy, 0);
                assert!(first_page.metrics.is_some(), "{case}/{limit}/{policy}");
            }
        }
    }
    assert_eq!(
        REPORT.observations.len(),
        cases.len() * LIMITS.len() * policies.len() * 3
    );
    assert!(REPORT.fixture.documents > 200);
    assert_eq!(REPORT.observations[0].corpus_size, REPORT.fixture.documents);
    assert!(
        REPORT
            .observations
            .iter()
            .all(|o| o.total_count as usize <= o.corpus_size)
    );
    let classes: BTreeSet<&str> = REPORT.aggregates.iter().map(|a| a.class.as_str()).collect();
    assert_eq!(classes, BTreeSet::from(["representative", "stress"]));
    for key in [
        "metric_version",
        "limits",
        "pools",
        "offsets",
        "production_pool",
    ] {
        assert!(REPORT.configuration.contains_key(key), "{key}");
    }
    assert_eq!(REPORT.code_sha, "test-sha");
    assert_eq!(REPORT.fixture.sha256.len(), 64);
}

#[test]
fn candidate_pool_repeated_runs_are_semantically_identical() {
    let repeat = run_candidate_pool_experiment(FIXTURE, "test-sha".into()).expect("repeat run");
    assert_eq!(repeat.deterministic_digest, REPORT.deterministic_digest);
    assert_eq!(repeat.observations, REPORT.observations);
}

#[test]
fn candidate_pool_tail_controls_detect_truncation() {
    // The current pool at limit 5 (15 per source) cannot see a target at rank 25 of
    // both sources; a 30-candidate pool can, and fusion ranks it first.
    let truncated = observation("stress-tail-025", 5, "current", 0);
    assert_eq!(truncated.metrics.unwrap().recall_at_5, 0.0);
    let recovered = observation("stress-tail-025", 5, "pool-30", 0);
    assert_eq!(recovered.metrics.unwrap().recall_at_5, 1.0);

    let truncated = observation("stress-tail-060", 10, "current", 0);
    assert_eq!(truncated.metrics.unwrap().recall_at_10, 0.0);
    let recovered = observation("stress-tail-060", 10, "pool-100", 0);
    assert_eq!(recovered.metrics.unwrap().recall_at_10, 1.0);

    // Pools above the production 100 cap reach the deepest target, which grows the
    // fused total even though the target stays below the first page.
    let capped = observation("stress-tail-150", 10, "pool-100", 0);
    let deep = observation("stress-tail-150", 10, "pool-200", 0);
    assert!(deep.total_count > capped.total_count);
}

#[test]
fn candidate_pool_report_renders_classes_separately_with_deltas_and_pagination() {
    let markdown = render_candidate_pool_report(&REPORT);
    let representative = markdown
        .find("## Mean first-page metrics: representative cases")
        .expect("representative section");
    let stress = markdown
        .find("## Mean first-page metrics: stress cases")
        .expect("stress section");
    assert!(representative < stress);
    assert!(markdown.contains("ΔRecall@5"));
    assert!(markdown.contains(&REPORT.fixture.sha256));
    assert!(markdown.contains("## Pagination"));
    assert!(markdown.contains("| stress-tail-150 | 10 | pool-200 | 0 |"));
}

#[test]
fn candidate_pool_rejects_a_label_outside_the_corpus() {
    let broken = FIXTURE.replacen("\"kestrel-tail-025\"]", "\"missing-thought\"]", 1);
    assert!(run_candidate_pool_experiment(&broken, "test-sha".into()).is_err());
}

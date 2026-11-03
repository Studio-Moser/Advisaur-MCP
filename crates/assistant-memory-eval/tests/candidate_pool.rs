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
fn candidate_pool_override_path_matches_the_default_path() {
    let parity = &REPORT.override_parity;
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
    assert_eq!(
        classes,
        BTreeSet::from(["representative-large", "representative-small", "stress"])
    );
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

    // Only pools above the production 100 cap admit the deepest target; fusion still
    // ranks it below the first page.
    let capped = observation("stress-tail-150", 10, "pool-100", 0);
    assert_eq!(capped.relevant_fused_ranks[0].rank, None);
    let deep = observation("stress-tail-150", 10, "pool-200", 0);
    let rank = deep.relevant_fused_ranks[0]
        .rank
        .expect("admitted at pool-200");
    assert!(rank > 10, "fused rank {rank}");
}

#[test]
fn candidate_pool_first_page_ranks_agree_with_the_whole_fused_pool() {
    for o in REPORT.observations.iter().filter(|o| o.offset == 0) {
        for (page, fused) in o.relevant_ranks.iter().zip(&o.relevant_fused_ranks) {
            assert_eq!(page.id, fused.id);
            match page.rank {
                Some(rank) => assert_eq!(fused.rank, Some(rank), "{}/{}", o.case_id, o.policy),
                None => assert!(
                    fused.rank.is_none_or(|rank| rank > o.limit),
                    "{}/{}",
                    o.case_id,
                    o.policy
                ),
            }
        }
    }
}

#[test]
fn candidate_pool_large_representative_cases_exceed_the_production_pool() {
    let large: Vec<&PoolObservation> = REPORT
        .observations
        .iter()
        .filter(|o| o.class == "representative-large" && o.policy == "pool-all" && o.offset == 0)
        .collect();
    assert!(!large.is_empty());
    assert!(large.iter().all(|o| o.total_count > 30), "{large:#?}");
    assert!(large.iter().any(|o| o.total_count > 100));
}

#[test]
fn candidate_pool_report_renders_classes_separately_with_deltas_and_pagination() {
    let markdown = render_candidate_pool_report(&REPORT);
    let headings: Vec<usize> = ["representative-small", "representative-large", "stress"]
        .iter()
        .map(|class| {
            markdown
                .find(&format!("## Mean first-page metrics: {class} cases"))
                .unwrap_or_else(|| panic!("{class} section"))
        })
        .collect();
    assert!(headings.is_sorted());
    assert!(markdown.contains("Override path given the production pool vs default path"));
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

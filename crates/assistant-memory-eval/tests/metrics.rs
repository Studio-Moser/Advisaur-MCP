use std::collections::BTreeSet;

use assistant_memory_eval::metrics::{score_pr_ranking, score_ranking};

#[test]
fn scores_a_hand_checked_multi_evidence_ranking() {
    let ranked = ["a", "b", "c", "d"].map(str::to_owned);
    let relevant = ["b", "d"].map(str::to_owned).into_iter().collect();

    let score = score_ranking(&ranked, &relevant, 3).expect("valid ranking");

    assert!((score.precision_at_k - (1.0 / 3.0)).abs() < 1e-12);
    assert!((score.recall_at_k - 0.5).abs() < 1e-12);
    assert!((score.ndcg_at_k - 0.386_852_807_234_541_63).abs() < 1e-12);
    assert!((score.mrr_at_k - 0.5).abs() < 1e-12);
}

#[test]
fn pr_metrics_use_the_declared_cutoffs_independently() {
    let ranked = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"].map(str::to_owned);
    let relevant = ["b", "g"].map(str::to_owned).into_iter().collect();

    let score = score_pr_ranking(&ranked, &relevant).expect("valid ranking");

    assert!((score.precision_at_5 - 0.2).abs() < 1e-12);
    assert!((score.recall_at_5 - 0.5).abs() < 1e-12);
    assert!((score.ndcg_at_10 - 0.591_235_204_823_027_7).abs() < 1e-12);
    assert!((score.mrr_at_10 - 0.5).abs() < 1e-12);
}

#[test]
fn rejects_rankings_that_cannot_produce_meaningful_metrics() {
    let empty = BTreeSet::new();
    let one = ["a".to_owned()];
    assert_eq!(
        score_ranking(&one, &empty, 5).unwrap_err().to_string(),
        "relevant IDs must not be empty"
    );

    let relevant = ["a".to_owned()].into_iter().collect();
    assert_eq!(
        score_ranking(&["a".into(), "a".into()], &relevant, 5)
            .unwrap_err()
            .to_string(),
        "ranked IDs must be unique"
    );
    assert_eq!(
        score_ranking(&one, &relevant, 0).unwrap_err().to_string(),
        "k must be greater than zero"
    );
}

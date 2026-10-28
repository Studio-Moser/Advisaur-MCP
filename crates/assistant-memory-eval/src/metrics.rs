use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutoffMetrics {
    pub precision_at_k: f64,
    pub recall_at_k: f64,
    pub ndcg_at_k: f64,
    pub mrr_at_k: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalMetrics {
    pub precision_at_5: f64,
    pub recall_at_5: f64,
    pub ndcg_at_10: f64,
    pub mrr_at_10: f64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MetricError {
    #[error("k must be greater than zero")]
    ZeroCutoff,
    #[error("relevant IDs must not be empty")]
    EmptyRelevant,
    #[error("ranked IDs must be unique")]
    DuplicateRankedId,
}

pub fn score_ranking(
    ranked: &[String],
    relevant: &BTreeSet<String>,
    k: usize,
) -> Result<CutoffMetrics, MetricError> {
    if k == 0 {
        return Err(MetricError::ZeroCutoff);
    }
    if relevant.is_empty() {
        return Err(MetricError::EmptyRelevant);
    }
    if ranked.iter().collect::<BTreeSet<_>>().len() != ranked.len() {
        return Err(MetricError::DuplicateRankedId);
    }

    let mut hits = 0usize;
    let mut dcg = 0.0;
    let mut first_relevant_rank = None;
    for (index, id) in ranked.iter().take(k).enumerate() {
        if relevant.contains(id) {
            hits += 1;
            let rank = index + 1;
            dcg += 1.0 / ((rank + 1) as f64).log2();
            first_relevant_rank.get_or_insert(rank);
        }
    }

    let ideal_hits = relevant.len().min(k);
    let idcg: f64 = (1..=ideal_hits)
        .map(|rank| 1.0 / ((rank + 1) as f64).log2())
        .sum();

    Ok(CutoffMetrics {
        precision_at_k: hits as f64 / k as f64,
        recall_at_k: hits as f64 / relevant.len() as f64,
        ndcg_at_k: dcg / idcg,
        mrr_at_k: first_relevant_rank.map_or(0.0, |rank| 1.0 / rank as f64),
    })
}

pub fn score_pr_ranking(
    ranked: &[String],
    relevant: &BTreeSet<String>,
) -> Result<RetrievalMetrics, MetricError> {
    let at_5 = score_ranking(ranked, relevant, 5)?;
    let at_10 = score_ranking(ranked, relevant, 10)?;
    Ok(RetrievalMetrics {
        precision_at_5: at_5.precision_at_k,
        recall_at_5: at_5.recall_at_k,
        ndcg_at_10: at_10.ndcg_at_k,
        mrr_at_10: at_10.mrr_at_k,
    })
}

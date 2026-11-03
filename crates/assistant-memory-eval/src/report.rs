use std::fmt::Write;

use thiserror::Error;

use crate::candidate_pool::{CLASSES, CandidatePoolReport};
use crate::comparison::ComparisonReport;
use crate::manifest::ResultManifest;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("result manifest is missing the overall aggregate")]
    MissingOverall,
}

pub fn render_run_report(manifest: &ResultManifest) -> Result<String, ReportError> {
    let overall = manifest
        .aggregates
        .get("overall")
        .ok_or(ReportError::MissingOverall)?;
    let failed_cases: Vec<_> = manifest.cases.iter().filter(|case| !case.passed).collect();
    let status = if failed_cases.is_empty() {
        "PASS"
    } else {
        "FAIL"
    };
    let mut report = format!(
        "# Memory Evaluation\n\nStatus: **{status}**\n\n\
         | Metric | Value |\n| --- | ---: |\n\
         | Recall@5 | {:.6} |\n| NDCG@10 | {:.6} |\n\
         | Precision@5 | {:.6} |\n| MRR@10 | {:.6} |\n\n\
         | Efficiency | Value |\n| --- | ---: |\n\
         | Estimated tokens | {} |\n| Serialized bytes | {} |\n\
         | Total run time | {} ms |\n| Median case time | {:.3} ms |\n| p95 case time | {:.3} ms |\n\n\
         Deterministic digest: `{}`\n\n## Failed cases\n\n",
        overall.recall_at_5,
        overall.ndcg_at_10,
        overall.precision_at_5,
        overall.mrr_at_10,
        manifest.efficiency.estimated_tokens,
        manifest.efficiency.serialized_bytes,
        manifest.runtime.duration_ms,
        manifest.runtime.median_case_duration_us as f64 / 1_000.0,
        manifest.runtime.p95_case_duration_us as f64 / 1_000.0,
        manifest.deterministic_digest,
    );
    if failed_cases.is_empty() {
        report.push_str("None.\n");
    } else {
        for case in failed_cases {
            let details = if case.failures.is_empty() {
                "unspecified failure".into()
            } else {
                case.failures.join("; ")
            };
            let _ = writeln!(report, "- {}: {details}", case.id);
        }
    }
    Ok(report)
}

pub fn render_comparison_report(comparison: &ComparisonReport) -> String {
    let status = if comparison.passed { "PASS" } else { "FAIL" };
    let mut report = format!(
        "# Memory Evaluation Gate\n\nStatus: **{status}**\n\n\
         Base code: `{}`  \nCandidate code: `{}`  \n\
         Base digest: `{}`  \nCandidate digest: `{}`\n\n",
        comparison.base_code_sha,
        comparison.candidate_code_sha,
        comparison.base_digest,
        comparison.candidate_digest,
    );
    report.push_str("| Metric | Base | Candidate | Delta |\n| --- | ---: | ---: | ---: |\n");
    for (name, metric) in &comparison.metric_deltas {
        let _ = writeln!(
            report,
            "| {name} | {:.6} | {:.6} | {:+.6} |",
            metric.base, metric.candidate, metric.delta
        );
    }
    report.push_str("\n## Gate failures\n\n");
    if comparison.failures.is_empty() {
        report.push_str("None.\n");
    } else {
        for failure in &comparison.failures {
            let _ = writeln!(report, "- {failure}");
        }
    }
    report.push_str("\n## Public cases with ranking changes\n\n");
    if comparison.case_changes.is_empty() {
        report.push_str("None.\n");
    } else {
        for case in &comparison.case_diffs {
            let _ = writeln!(
                report,
                "- {}: base {:?}; candidate {:?}",
                case.id, case.base_ranked_ids, case.candidate_ranked_ids
            );
            for relevant in &case.relevant_rank_deltas {
                let _ = writeln!(
                    report,
                    "  - relevant {}: base rank {:?}; candidate rank {:?}",
                    relevant.id, relevant.base_rank, relevant.candidate_rank
                );
            }
        }
    }
    report
}

pub fn render_candidate_pool_report(report: &CandidatePoolReport) -> String {
    let parity = &report.override_parity;
    let mut out = format!(
        "# Hybrid Candidate-Pool Benchmark\n\n\
         Code: `{}`  \nFixture: `{}` ({}), SHA-256 `{}`  \n\
         Documents: {}; vectors: {} ({} dimensions)  \n\
         Metric version: `{}`; limits: {}; offsets: {}  \nPools per source: {}  \n\
         Override path given the production pool vs default path: {}/{} identical  \n\
         Deterministic digest: `{}`  \nRun: {} on {}, {} ms\n",
        report.code_sha,
        report.fixture.source,
        report.fixture.suite_version,
        report.fixture.sha256,
        report.fixture.documents,
        report.fixture.vectors,
        report.fixture.embedding_dimensions,
        report
            .configuration
            .get("metric_version")
            .map_or("", String::as_str),
        report
            .configuration
            .get("limits")
            .map_or("", String::as_str),
        report
            .configuration
            .get("offsets")
            .map_or("", String::as_str),
        report.configuration.get("pools").map_or("", String::as_str),
        parity.identical,
        parity.checked,
        report.deterministic_digest,
        report.runtime.generated_at,
        report.runtime.target,
        report.runtime.duration_ms,
    );
    for mismatch in &parity.mismatches {
        let _ = writeln!(out, "- override/default mismatch: {mismatch}");
    }

    for class in CLASSES {
        let _ = write!(
            out,
            "\n## Mean first-page metrics: {class} cases\n\n\
             Deltas are against `current` at the same limit. Median time is per handler call.\n\n\
             | Limit | Policy | Pool/source | Cases | Mean fused total | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | \
             ΔRecall@5 | ΔRecall@10 | ΔNDCG@5 | ΔNDCG@10 | Median µs |\n\
             | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n"
        );
        for aggregate in report.aggregates.iter().filter(|a| a.class == class) {
            let mut durations: Vec<u64> = report
                .observations
                .iter()
                .filter(|o| {
                    o.class == class
                        && o.limit == aggregate.limit
                        && o.policy == aggregate.policy
                        && o.offset == 0
                })
                .filter_map(|o| {
                    report
                        .runtime
                        .observation_duration_us
                        .get(&CandidatePoolReport::runtime_key(o))
                        .copied()
                })
                .collect();
            durations.sort_unstable();
            let median = durations.get(durations.len() / 2).copied().unwrap_or(0);
            let (m, d) = (&aggregate.mean, &aggregate.delta_vs_current);
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {:.1} | {:.4} | {:.4} | {:.4} | {:.4} | {:+.4} | {:+.4} | {:+.4} | {:+.4} | {} |",
                aggregate.limit,
                aggregate.policy,
                aggregate.pool_per_source,
                aggregate.cases,
                aggregate.mean_fused_total,
                m.recall_at_5,
                m.recall_at_10,
                m.ndcg_at_5,
                m.ndcg_at_10,
                d.recall_at_5,
                d.recall_at_10,
                d.ndcg_at_5,
                d.ndcg_at_10,
                median,
            );
        }
    }

    out.push_str(
        "\n## First page per case\n\n\
         Page ranks are positions on the first page; fused ranks are positions in the whole \
         fused pool. `-` means not returned or not admitted.\n\n\
         | Case | Class | Limit | Policy | Page ranks | Fused ranks | Recall@10 | NDCG@10 | Fused total | Corpus |\n\
         | --- | --- | ---: | --- | --- | --- | ---: | ---: | ---: | ---: |\n",
    );
    for o in report.observations.iter().filter(|o| o.offset == 0) {
        let format_ranks = |ranks: &[crate::manifest::RelevantRank]| {
            ranks
                .iter()
                .map(|r| format!("{}:{}", r.id, r.rank.map_or("-".into(), |r| r.to_string())))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let ranks = format_ranks(&o.relevant_ranks);
        let fused = format_ranks(&o.relevant_fused_ranks);
        let metrics = o.metrics.unwrap_or(crate::candidate_pool::PoolMetrics {
            recall_at_5: 0.0,
            recall_at_10: 0.0,
            ndcg_at_5: 0.0,
            ndcg_at_10: 0.0,
        });
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {ranks} | {fused} | {:.4} | {:.4} | {} | {} |",
            o.case_id,
            o.class,
            o.limit,
            o.policy,
            metrics.recall_at_10,
            metrics.ndcg_at_10,
            o.total_count,
            o.corpus_size,
        );
    }

    out.push_str(
        "\n## Pagination\n\n\
         `total_count` is the filtered fused pool, not the corpus or the full match set.\n\n\
         | Case | Limit | Policy | Offset | Page size | total_count | has_more | Corpus |\n\
         | --- | ---: | --- | ---: | ---: | ---: | --- | ---: |\n",
    );
    for o in &report.observations {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            o.case_id,
            o.limit,
            o.policy,
            o.offset,
            o.page_ids.len(),
            o.total_count,
            o.has_more,
            o.corpus_size,
        );
    }
    out
}

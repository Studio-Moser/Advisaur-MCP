use std::fmt::Write;

use thiserror::Error;

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
        "# Shelby Memory Evaluation\n\nStatus: **{status}**\n\n\
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
        "# Shelby Memory Evaluation Gate\n\nStatus: **{status}**\n\n\
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

use assistant_memory::repair::RepairReport;

pub fn strip_frontmatter(markdown: &str) -> &str {
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return markdown;
    };
    let Some(end) = rest.find("\n---\n") else {
        return markdown;
    };
    rest[end + 5..]
        .strip_prefix('\n')
        .unwrap_or(&rest[end + 5..])
}

pub fn format_repair_report(report: &RepairReport, apply: bool) -> String {
    let mut lines = Vec::new();
    if apply {
        lines.push("APPLIED — project_identifier back-fill complete".to_string());
    } else {
        lines.push("DRY RUN — no changes written (re-run with --apply to write)".to_string());
    }
    lines.push(String::new());
    lines.push(format!(
        "Scanned {} thoughts missing project_identifier",
        report.scanned
    ));
    lines.push(format!(
        "  {} high-confidence fixes identified",
        report.high_confidence.len()
    ));
    lines.push(format!(
        "  {} flagged for review (low-confidence)",
        report.flagged.len()
    ));
    if apply {
        lines.push(format!("  {} written", report.applied));
        lines.push(format!(
            "  {} flagged for review (marked needs_project_review in metadata)",
            report.flagged.len()
        ));
    }

    if !report.high_confidence.is_empty() {
        lines.push(String::new());
        lines.push("High-confidence assignments:".into());
        let mut tally: Vec<(&str, usize)> = Vec::new();
        for item in &report.high_confidence {
            let slug = item.suggested_slug.as_deref().unwrap_or("(unknown)");
            if let Some((_, count)) = tally.iter_mut().find(|(existing, _)| *existing == slug) {
                *count += 1;
            } else {
                tally.push((slug, 1));
            }
        }
        for (slug, count) in tally {
            let suffix = if count == 1 { "" } else { "s" };
            lines.push(format!("  {slug}: {count} thought{suffix}"));
        }
    }

    if !report.flagged.is_empty() {
        lines.push(String::new());
        lines.push("Flagged (ambiguous — manual review needed):".into());
        for item in &report.flagged {
            lines.push(format!("  {}  reason: {}", item.id, item.reason));
        }
    }
    if !apply {
        lines.push(String::new());
        lines.push("Re-run with --apply to write high-confidence assignments.".into());
    }
    lines.join("\n")
}

pub fn protocol() -> &'static str {
    include_str!("../../../assets/Memory Protocol.md")
}

pub fn forage() -> &'static str {
    strip_frontmatter(include_str!("../../../skills/advisaur-forage/SKILL.md"))
}

pub fn onboard() -> &'static str {
    strip_frontmatter(include_str!("../../../skills/advisaur-onboard/SKILL.md"))
}

pub fn migrate() -> &'static str {
    include_str!("../../../assets/Migration Prompt.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistant_memory::repair::{RepairConfidence, RepairItem};

    #[test]
    fn frontmatter_is_stripped_only_when_complete() {
        assert_eq!(strip_frontmatter("---\nname: x\n---\nbody\n"), "body\n");
        assert_eq!(
            strip_frontmatter("---\nname: x\nbody"),
            "---\nname: x\nbody"
        );
        assert!(forage().starts_with("# Advisaur Forage"));
        assert!(onboard().starts_with("# Advisaur Onboard"));
    }

    #[test]
    fn repair_report_matches_the_existing_cli_contract() {
        let report = RepairReport {
            scanned: 2,
            high_confidence: vec![RepairItem {
                id: "one".into(),
                suggested_slug: Some("shelby".into()),
                confidence: RepairConfidence::High,
                reason: "distinctive topic → shelby".into(),
            }],
            flagged: vec![RepairItem {
                id: "two".into(),
                suggested_slug: None,
                confidence: RepairConfidence::Low,
                reason: "no distinctive signal".into(),
            }],
            applied: 1,
        };
        assert_eq!(
            format_repair_report(&report, true),
            "APPLIED — project_identifier back-fill complete\n\nScanned 2 thoughts missing project_identifier\n  1 high-confidence fixes identified\n  1 flagged for review (low-confidence)\n  1 written\n  1 flagged for review (marked needs_project_review in metadata)\n\nHigh-confidence assignments:\n  shelby: 1 thought\n\nFlagged (ambiguous — manual review needed):\n  two  reason: no distinctive signal"
        );
    }
}

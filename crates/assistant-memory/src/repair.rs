//! Deterministic repair of legacy thoughts that predate canonical project identity.
use rusqlite::Connection;
use serde_json::Value;
use std::collections::BTreeSet;

use crate::error::Result;
use crate::projects::{Project, get_project_by_alias, list_projects};
use crate::seed::{Seed, ensure_seed_projects, source_alias_map, to_registry_seeds};
use crate::thoughts::{ThoughtUpdate, get_thought, parse_json_array, update_thought};

const REPAIRED_BY: &str = "integrity-project-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairConfidence {
    High,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairItem {
    pub id: String,
    pub suggested_slug: Option<String>,
    pub confidence: RepairConfidence,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairReport {
    pub scanned: usize,
    pub high_confidence: Vec<RepairItem>,
    pub flagged: Vec<RepairItem>,
    pub applied: usize,
}

#[derive(Debug)]
struct RepairCandidate {
    id: String,
    project: Option<String>,
    topics: Vec<String>,
    source: Option<String>,
}

fn repair_candidates(conn: &Connection) -> Result<Vec<RepairCandidate>> {
    let mut statement = conn.prepare(
        "SELECT id, project, topics, source
         FROM thoughts
         WHERE project_id IS NULL AND (project_identifier IS NULL OR project_identifier = '')
         ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        let topics: Option<String> = row.get("topics")?;
        Ok(RepairCandidate {
            id: row.get("id")?,
            project: row.get("project")?,
            topics: parse_json_array(topics.as_deref()),
            source: row.get("source")?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn infer_by_path(path: Option<&str>, projects: &[Project]) -> Option<String> {
    let path = path?;
    let mut matches = projects
        .iter()
        .flat_map(|project| {
            project.member_paths.iter().filter_map(move |member_path| {
                let matches = path == member_path
                    || path
                        .strip_prefix(member_path)
                        .is_some_and(|suffix| suffix.starts_with('/'));
                matches.then_some((member_path.len(), project.current_slug.as_str()))
            })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(right.1)));
    matches.first().map(|(_, slug)| (*slug).to_string())
}

fn infer_by_topics(topics: &[String], seed: &Seed) -> (Option<String>, bool) {
    let hits = topics
        .iter()
        .filter_map(|topic| seed.topic_clusters.get(&topic.to_lowercase()))
        .cloned()
        .collect::<BTreeSet<_>>();
    match hits.len() {
        0 => (None, false),
        1 => (hits.into_iter().next(), false),
        _ => (None, true),
    }
}

fn infer_by_source(source: Option<&str>, seed: &Seed) -> Option<String> {
    let source = source?.to_lowercase();
    let aliases = source_alias_map(seed);
    let mut matching = aliases
        .iter()
        .filter(|(alias, _)| source.contains(alias.as_str()))
        .collect::<Vec<_>>();
    matching.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.0.cmp(right.0)));
    matching.first().map(|(_, slug)| (*slug).clone())
}

fn classify(candidate: RepairCandidate, projects: &[Project], seed: &Seed) -> RepairItem {
    if let Some(slug) = infer_by_path(candidate.project.as_deref(), projects) {
        return RepairItem {
            id: candidate.id,
            reason: format!("legacy project path matches {slug} member_path"),
            suggested_slug: Some(slug),
            confidence: RepairConfidence::High,
        };
    }
    let (topic_slug, topics_ambiguous) = infer_by_topics(&candidate.topics, seed);
    if let Some(slug) = topic_slug {
        return RepairItem {
            id: candidate.id,
            reason: format!("distinctive topic → {slug}"),
            suggested_slug: Some(slug),
            confidence: RepairConfidence::High,
        };
    }
    if let Some(slug) = infer_by_source(candidate.source.as_deref(), seed) {
        return RepairItem {
            id: candidate.id,
            reason: format!("distinctive source → {slug}"),
            suggested_slug: Some(slug),
            confidence: RepairConfidence::High,
        };
    }
    RepairItem {
        id: candidate.id,
        suggested_slug: None,
        confidence: RepairConfidence::Low,
        reason: if topics_ambiguous {
            "topics map to multiple projects".into()
        } else {
            "no distinctive signal".into()
        },
    }
}

pub fn plan_project_repairs(conn: &Connection, seed: &Seed) -> Result<RepairReport> {
    let projects = list_projects(conn)?;
    let candidates = repair_candidates(conn)?;
    let mut report = RepairReport {
        scanned: candidates.len(),
        ..Default::default()
    };
    for candidate in candidates {
        let item = classify(candidate, &projects, seed);
        match item.confidence {
            RepairConfidence::High => report.high_confidence.push(item),
            RepairConfidence::Low => report.flagged.push(item),
        }
    }
    Ok(report)
}

pub fn repair_projects(conn: &mut Connection, seed: &Seed, apply: bool) -> Result<RepairReport> {
    ensure_seed_projects(conn, &to_registry_seeds(seed))?;
    let mut report = plan_project_repairs(conn, seed)?;
    if !apply {
        return Ok(report);
    }

    let transaction = conn.transaction()?;
    let mut applied = 0;
    for item in &report.high_confidence {
        let Some(slug) = item.suggested_slug.as_deref() else {
            continue;
        };
        let Some(thought) = get_thought(&transaction, &item.id)? else {
            continue;
        };
        let Some(project) = get_project_by_alias(&transaction, slug)? else {
            continue;
        };
        let mut metadata = thought.metadata.unwrap_or_default();
        metadata.insert("repaired_by".into(), Value::String(REPAIRED_BY.into()));
        metadata.insert("repaired_reason".into(), Value::String(item.reason.clone()));
        metadata.insert(
            "repaired_from".into(),
            Value::String(
                if thought.project_identifier.as_deref() == Some("") {
                    "empty"
                } else {
                    "null"
                }
                .into(),
            ),
        );
        update_thought(
            &transaction,
            &item.id,
            &ThoughtUpdate {
                project_id: Some(project.project_id),
                project_identifier: Some(slug.into()),
                metadata: Some(metadata),
                ..Default::default()
            },
        )?;
        applied += 1;
    }
    for item in &report.flagged {
        let Some(thought) = get_thought(&transaction, &item.id)? else {
            continue;
        };
        let mut metadata = thought.metadata.unwrap_or_default();
        metadata.insert("needs_project_review".into(), Value::Bool(true));
        metadata.insert(
            "suggested_project".into(),
            item.suggested_slug
                .as_ref()
                .map_or(Value::Null, |slug| Value::String(slug.clone())),
        );
        update_thought(
            &transaction,
            &item.id,
            &ThoughtUpdate {
                metadata: Some(metadata),
                ..Default::default()
            },
        )?;
    }
    transaction.commit()?;
    report.applied = applied;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;
    use crate::seed::{Seed, SeedProject, ensure_seed_projects, to_registry_seeds};
    use crate::thoughts::{ThoughtInput, get_thought, insert_thought};
    use serde_json::{Map, Value, json};
    use std::collections::HashMap;

    fn project(slug: &str, paths: &[&str], aliases: &[&str]) -> SeedProject {
        SeedProject {
            slug: slug.into(),
            display_name: slug.into(),
            member_paths: paths.iter().map(|value| (*value).into()).collect(),
            source_aliases: aliases.iter().map(|value| (*value).into()).collect(),
            ..Default::default()
        }
    }

    fn seed() -> Seed {
        Seed {
            projects: vec![
                project("workspace", &["/p"], &["alpha"]),
                project("shelby", &["/p/shelby"], &["alpha-beta", "shelby"]),
                project("other", &["/p/other"], &["beta"]),
            ],
            topic_clusters: HashMap::from([
                ("memory".into(), "shelby".into()),
                ("markets".into(), "other".into()),
            ]),
        }
    }

    fn insert_with_id(
        m: &Memory,
        id: &str,
        project: Option<&str>,
        project_identifier: Option<&str>,
        topics: &[&str],
        source: Option<&str>,
        metadata: Option<Map<String, Value>>,
    ) {
        let generated = insert_thought(
            &m.conn,
            &ThoughtInput {
                content: format!("thought {id}"),
                project: project.map(str::to_owned),
                project_identifier: project_identifier.map(str::to_owned),
                topics: Some(topics.iter().map(|value| (*value).into()).collect()),
                source: source.map(str::to_owned),
                metadata,
                ..Default::default()
            },
        )
        .unwrap();
        m.conn
            .execute(
                "UPDATE thoughts SET id = ?1 WHERE id = ?2",
                [id, &generated],
            )
            .unwrap();
    }

    fn register(m: &Memory, seed: &Seed) {
        ensure_seed_projects(&m.conn, &to_registry_seeds(seed)).unwrap();
    }

    #[test]
    fn path_inference_wins_and_uses_the_longest_member_path() {
        let m = Memory::open_in_memory().unwrap();
        let seed = seed();
        register(&m, &seed);
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000001",
            Some("/p/shelby/crates/server"),
            None,
            &["markets"],
            Some("beta"),
            None,
        );

        let report = plan_project_repairs(&m.conn, &seed).unwrap();

        assert_eq!(report.scanned, 1);
        assert_eq!(report.high_confidence.len(), 1);
        assert_eq!(
            report.high_confidence[0].suggested_slug.as_deref(),
            Some("shelby")
        );
        assert_eq!(
            report.high_confidence[0].reason,
            "legacy project path matches shelby member_path"
        );
    }

    #[test]
    fn one_topic_cluster_is_high_confidence_and_conflicts_are_flagged() {
        let m = Memory::open_in_memory().unwrap();
        let seed = seed();
        register(&m, &seed);
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000001",
            None,
            None,
            &["Memory"],
            None,
            None,
        );
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000002",
            None,
            None,
            &["memory", "markets"],
            None,
            None,
        );

        let report = plan_project_repairs(&m.conn, &seed).unwrap();

        assert_eq!(
            report.high_confidence[0].suggested_slug.as_deref(),
            Some("shelby")
        );
        assert_eq!(report.flagged.len(), 1);
        assert_eq!(report.flagged[0].reason, "topics map to multiple projects");
    }

    #[test]
    fn source_inference_prefers_longest_alias_then_lexicographic_ties() {
        let m = Memory::open_in_memory().unwrap();
        let seed = seed();
        register(&m, &seed);
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000001",
            None,
            None,
            &[],
            Some("daily-alpha-beta-report"),
            None,
        );
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000002",
            None,
            None,
            &[],
            Some("alpha and beta"),
            None,
        );

        let report = plan_project_repairs(&m.conn, &seed).unwrap();

        assert_eq!(
            report.high_confidence[0].suggested_slug.as_deref(),
            Some("shelby")
        );
        assert_eq!(
            report.high_confidence[1].suggested_slug.as_deref(),
            Some("workspace")
        );
    }

    #[test]
    fn empty_seed_has_no_implicit_project_knowledge() {
        let m = Memory::open_in_memory().unwrap();
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000001",
            None,
            None,
            &["memory"],
            Some("shelby"),
            None,
        );

        let report = plan_project_repairs(&m.conn, &Seed::default()).unwrap();

        assert!(report.high_confidence.is_empty());
        assert_eq!(report.flagged.len(), 1);
        assert_eq!(report.flagged[0].reason, "no distinctive signal");
    }

    #[test]
    fn dry_run_does_not_mutate_candidate_thoughts() {
        let mut m = Memory::open_in_memory().unwrap();
        let seed = seed();
        register(&m, &seed);
        let id = "00000000-0000-0000-0000-000000000001";
        insert_with_id(&m, id, None, None, &["memory"], None, None);
        let before = get_thought(&m.conn, id).unwrap().unwrap();

        let report = repair_projects(&mut m.conn, &seed, false).unwrap();

        assert_eq!(report.applied, 0);
        assert_eq!(get_thought(&m.conn, id).unwrap().unwrap(), before);
    }

    #[test]
    fn apply_assigns_identity_merges_metadata_and_records_null_or_empty_origin() {
        let mut m = Memory::open_in_memory().unwrap();
        let seed = seed();
        let mut metadata = Map::new();
        metadata.insert("prior_key".into(), json!("prior_value"));
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000001",
            None,
            None,
            &["memory"],
            None,
            Some(metadata),
        );
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000002",
            None,
            Some(""),
            &["memory"],
            None,
            None,
        );

        let report = repair_projects(&mut m.conn, &seed, true).unwrap();

        assert_eq!(report.applied, 2);
        for (id, origin) in [
            ("00000000-0000-0000-0000-000000000001", "null"),
            ("00000000-0000-0000-0000-000000000002", "empty"),
        ] {
            let thought = get_thought(&m.conn, id).unwrap().unwrap();
            assert_eq!(thought.project_identifier.as_deref(), Some("shelby"));
            assert!(thought.project_id.is_some());
            let metadata = thought.metadata.unwrap();
            assert_eq!(metadata["repaired_by"], "integrity-project-v1");
            assert_eq!(metadata["repaired_from"], origin);
        }
        assert_eq!(
            get_thought(&m.conn, "00000000-0000-0000-0000-000000000001")
                .unwrap()
                .unwrap()
                .metadata
                .unwrap()["prior_key"],
            "prior_value"
        );
    }

    #[test]
    fn low_confidence_candidates_are_queued_without_ai_reviewed() {
        let mut m = Memory::open_in_memory().unwrap();
        let seed = seed();
        let id = "00000000-0000-0000-0000-000000000001";
        insert_with_id(&m, id, None, None, &["generic"], None, None);

        let report = repair_projects(&mut m.conn, &seed, true).unwrap();

        assert_eq!(report.flagged.len(), 1);
        let metadata = get_thought(&m.conn, id).unwrap().unwrap().metadata.unwrap();
        assert_eq!(metadata["needs_project_review"], true);
        assert_eq!(metadata["suggested_project"], Value::Null);
        assert!(!metadata.contains_key("ai_reviewed"));
    }

    #[test]
    fn authoritative_or_legacy_resolved_thoughts_are_not_candidates() {
        let mut m = Memory::open_in_memory().unwrap();
        let seed = seed();
        register(&m, &seed);
        let project_id = crate::projects::get_project_by_alias(&m.conn, "shelby")
            .unwrap()
            .unwrap()
            .project_id;
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000001",
            None,
            Some("shelby"),
            &["markets"],
            None,
            None,
        );
        insert_with_id(
            &m,
            "00000000-0000-0000-0000-000000000002",
            None,
            None,
            &["markets"],
            None,
            None,
        );
        m.conn
            .execute(
                "UPDATE thoughts SET project_id = ?1 WHERE id = ?2",
                [&project_id, "00000000-0000-0000-0000-000000000002"],
            )
            .unwrap();

        let report = repair_projects(&mut m.conn, &seed, true).unwrap();

        assert_eq!(report.scanned, 0);
    }

    #[test]
    fn unresolved_suggested_alias_is_not_written() {
        let mut m = Memory::open_in_memory().unwrap();
        let mut seed = Seed::default();
        seed.topic_clusters
            .insert("orphan".into(), "missing-project".into());
        let id = "00000000-0000-0000-0000-000000000001";
        insert_with_id(&m, id, None, None, &["orphan"], None, None);

        let report = repair_projects(&mut m.conn, &seed, true).unwrap();

        assert_eq!(report.applied, 0);
        let thought = get_thought(&m.conn, id).unwrap().unwrap();
        assert!(thought.project_id.is_none());
        assert!(thought.project_identifier.is_none());
    }

    #[test]
    fn apply_rolls_back_every_repair_when_one_update_fails() {
        let mut m = Memory::open_in_memory().unwrap();
        let seed = seed();
        let high = "00000000-0000-0000-0000-000000000001";
        let flagged = "00000000-0000-0000-0000-000000000002";
        insert_with_id(&m, high, None, None, &["memory"], None, None);
        insert_with_id(&m, flagged, None, None, &["generic"], None, None);
        m.conn
            .execute_batch(
                "CREATE TRIGGER fail_flagged_repair BEFORE UPDATE OF metadata ON thoughts
                 WHEN NEW.id = '00000000-0000-0000-0000-000000000002'
                 BEGIN SELECT RAISE(ABORT, 'forced repair failure'); END;",
            )
            .unwrap();

        assert!(repair_projects(&mut m.conn, &seed, true).is_err());

        for id in [high, flagged] {
            let thought = get_thought(&m.conn, id).unwrap().unwrap();
            assert!(thought.project_id.is_none());
            assert!(thought.project_identifier.is_none());
            assert!(thought.metadata.is_none());
        }
    }
}

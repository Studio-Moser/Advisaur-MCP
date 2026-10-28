//! Per-user project seed (ADR 0001 §1b.5): `~/.shelbymcp/projects.seed.json`,
//! empty by default, strict whole-file validation.
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::now_iso;
use crate::projects::{ProjectSeed, create_local_only_project, get_project_by_id};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeedProject {
    pub slug: String,
    pub display_name: String,
    pub member_repos: Vec<String>,
    pub member_paths: Vec<String>,
    pub provisional: bool,
    pub source_aliases: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seed {
    pub projects: Vec<SeedProject>,
    pub topic_clusters: HashMap<String, String>,
}

pub fn seed_default_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".shelbymcp").join("projects.seed.json")
}

fn string_array(v: Option<&Value>) -> Option<Vec<String>> {
    match v {
        None => Some(vec![]),
        Some(Value::Array(items)) => items
            .iter()
            .map(|x| x.as_str().map(str::to_owned))
            .collect(),
        Some(_) => None,
    }
}

fn parse_project(raw: &Value) -> Option<SeedProject> {
    let o = raw.as_object()?;
    let provisional = match o.get("provisional") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return None,
    };
    Some(SeedProject {
        slug: o.get("slug")?.as_str()?.to_string(),
        display_name: o.get("displayName")?.as_str()?.to_string(),
        member_repos: string_array(o.get("memberRepos"))?,
        member_paths: string_array(o.get("memberPaths"))?,
        provisional,
        source_aliases: string_array(o.get("sourceAliases"))?,
    })
}

/// Parse seed JSON text; any malformed element rejects the whole file to empty.
pub fn parse_seed(text: &str) -> Seed {
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(text) else {
        return Seed::default();
    };
    let projects = match obj.get("projects") {
        None => vec![],
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match parse_project(item) {
                    Some(p) => out.push(p),
                    None => return Seed::default(),
                }
            }
            out
        }
        Some(_) => return Seed::default(),
    };
    let topic_clusters = match obj.get("topicClusters") {
        None => HashMap::new(),
        Some(Value::Object(m)) => {
            let mut out = HashMap::new();
            for (k, v) in m {
                let Some(s) = v.as_str() else {
                    return Seed::default();
                };
                out.insert(k.clone(), s.to_string());
            }
            out
        }
        Some(_) => return Seed::default(),
    };
    Seed {
        projects,
        topic_clusters,
    }
}

/// Load from disk; absent or unreadable files yield the empty seed, never an error.
pub fn load_seed(path: &Path) -> Seed {
    std::fs::read_to_string(path)
        .map(|t| parse_seed(&t))
        .unwrap_or_default()
}

pub fn to_registry_seeds(seed: &Seed) -> Vec<ProjectSeed> {
    seed.projects
        .iter()
        .map(|p| ProjectSeed {
            slug: p.slug.clone(),
            display_name: p.display_name.clone(),
            member_repos: p.member_repos.clone(),
            member_paths: p.member_paths.clone(),
            provisional: p.provisional,
        })
        .collect()
}

/// `lowercased alias → slug` used by source inference.
pub fn source_alias_map(seed: &Seed) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for p in &seed.projects {
        for alias in &p.source_aliases {
            map.insert(alias.to_lowercase(), p.slug.clone());
        }
    }
    map
}

/// Idempotently seed known projects without touching established identity fields.
pub fn ensure_seed_projects(conn: &Connection, projects: &[ProjectSeed]) -> Result<()> {
    for project in projects {
        let existing_id: Option<String> = conn
            .query_row("SELECT project_id FROM project_slug_aliases WHERE slug = ?1 AND status IN ('tentative', 'current')", params![project.slug], |r| r.get(0))
            .optional()?;
        let existing = match existing_id {
            Some(id) => get_project_by_id(conn, &id)?,
            None => None,
        };
        let Some(existing) = existing else {
            create_local_only_project(conn, project)?;
            continue;
        };
        if !existing.provisional
            && (!existing.member_repos.is_empty() || !existing.member_paths.is_empty())
        {
            continue;
        }
        conn.execute(
            "UPDATE projects SET display_name = ?1, member_repos = ?2, member_paths = ?3, provisional = ?4, updated_at = ?5 WHERE project_id = ?6",
            params![project.display_name, serde_json::to_string(&project.member_repos)?, serde_json::to_string(&project.member_paths)?, project.provisional as i64, now_iso(), existing.project_id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;
    use crate::projects::get_project_by_alias;

    #[test]
    fn strict_whole_file_validation() {
        assert_eq!(parse_seed("not json"), Seed::default());
        assert_eq!(parse_seed("[]"), Seed::default());
        let good = r#"{"projects":[{"slug":"a","displayName":"A","memberPaths":["/x"],"sourceAliases":["Slack"]}],"topicClusters":{"auth":"a"}}"#;
        let seed = parse_seed(good);
        assert_eq!(seed.projects.len(), 1);
        assert_eq!(seed.topic_clusters["auth"], "a");
        assert_eq!(source_alias_map(&seed)["slack"], "a");
        let bad_project = r#"{"projects":[{"slug":"a","displayName":"A"},{"slug":"b"}]}"#;
        assert_eq!(
            parse_seed(bad_project),
            Seed::default(),
            "one bad entry rejects the file"
        );
        let bad_topics = r#"{"projects":[],"topicClusters":{"auth":1}}"#;
        assert_eq!(parse_seed(bad_topics), Seed::default());
        let bad_flag = r#"{"projects":[{"slug":"a","displayName":"A","provisional":"yes"}]}"#;
        assert_eq!(parse_seed(bad_flag), Seed::default());
        assert_eq!(
            load_seed(Path::new("/definitely/missing.json")),
            Seed::default()
        );
    }

    #[test]
    fn ensure_seed_creates_then_updates_only_unclaimed_projects() {
        let m = Memory::open_in_memory().unwrap();
        let seeds = to_registry_seeds(&parse_seed(
            r#"{"projects":[{"slug":"alpha","displayName":"Alpha","memberPaths":["/w/alpha"]}]}"#,
        ));
        ensure_seed_projects(&m.conn, &seeds).unwrap();
        let p = get_project_by_alias(&m.conn, "alpha").unwrap().unwrap();
        assert_eq!(p.member_paths, vec!["/w/alpha"]);
        // established (non-provisional, with paths): later seeds don't overwrite
        let seeds2 = to_registry_seeds(&parse_seed(
            r#"{"projects":[{"slug":"alpha","displayName":"Renamed","memberPaths":["/other"]}]}"#,
        ));
        ensure_seed_projects(&m.conn, &seeds2).unwrap();
        let p = get_project_by_alias(&m.conn, "alpha").unwrap().unwrap();
        assert_eq!(p.display_name, "Alpha");
        // provisional projects do get updated
        create_local_only_project(
            &m.conn,
            &ProjectSeed {
                slug: "beta".into(),
                display_name: "beta".into(),
                provisional: true,
                ..Default::default()
            },
        )
        .unwrap();
        let seeds3 = to_registry_seeds(&parse_seed(
            r#"{"projects":[{"slug":"beta","displayName":"Beta","memberRepos":["h/o/r"]}]}"#,
        ));
        ensure_seed_projects(&m.conn, &seeds3).unwrap();
        let p = get_project_by_alias(&m.conn, "beta").unwrap().unwrap();
        assert_eq!((p.display_name.as_str(), p.provisional), ("Beta", false));
    }
}

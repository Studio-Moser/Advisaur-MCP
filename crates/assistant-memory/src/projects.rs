//! Project registry (ADR 0001 §1a) and alias lookups.
pub mod membership;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::identity::{derive_existing_project_id, is_valid_project_slug};
use crate::now_iso;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProjectSeed {
    pub slug: String,
    pub display_name: String,
    pub member_repos: Vec<String>,
    pub member_paths: Vec<String>,
    pub provisional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub slug: String,
    pub project_id: String,
    pub current_slug: String,
    pub identity_state: String,
    pub display_name: String,
    pub member_repos: Vec<String>,
    pub member_paths: Vec<String>,
    pub provisional: bool,
}

fn row_to_project(r: &Row) -> rusqlite::Result<Project> {
    let repos: Option<String> = r.get("member_repos")?;
    let paths: Option<String> = r.get("member_paths")?;
    Ok(Project {
        slug: r.get("slug")?,
        project_id: r.get("project_id")?,
        current_slug: r.get("current_slug")?,
        identity_state: r.get("identity_state")?,
        display_name: r.get("display_name")?,
        member_repos: crate::thoughts::parse_json_array(repos.as_deref()),
        member_paths: crate::thoughts::parse_json_array(paths.as_deref()),
        provisional: r.get::<_, i64>("provisional")? == 1,
    })
}

/// Strip `.git`, scheme, and convert `git@host:org/repo` → `host/org/repo`
/// (identical to Shelby-MacOS `ProjectDetector.normalizeGitRemoteURL`).
pub fn normalize_git_remote(url: &str) -> String {
    let mut s = url.trim().to_string();
    if let Some(stripped) = s.strip_suffix(".git") {
        s = stripped.to_string();
    }
    if s.contains('@') && s.contains(':') && !s.contains("://") {
        let after_at = &s[s.find('@').unwrap() + 1..];
        s = after_at.replacen(':', "/", 1);
    }
    if let Some(rest) = s.strip_prefix("https://") {
        s = rest.to_string();
    } else if let Some(rest) = s.strip_prefix("http://") {
        s = rest.to_string();
    }
    s
}

/// Legacy-compatible upsert keyed by frozen `slug`; identity columns never change.
pub fn upsert_project(conn: &Connection, seed: &ProjectSeed) -> Result<()> {
    let project_id = derive_existing_project_id(&seed.slug)
        .ok_or_else(|| Error::InvalidInput("invalid_legacy_slug".into()))?;
    let now = now_iso();
    conn.execute_batch("SAVEPOINT project_write")?;
    let r = (|| -> Result<()> {
        conn.execute(
            "INSERT INTO projects (slug, project_id, current_slug, identity_state, display_name, member_repos, member_paths, provisional, created_at, updated_at)
             VALUES (?1, ?2, ?1, 'local_only', ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(slug) DO UPDATE SET display_name = excluded.display_name, member_repos = excluded.member_repos,
               member_paths = excluded.member_paths, provisional = excluded.provisional, updated_at = excluded.updated_at",
            params![seed.slug, project_id, seed.display_name, serde_json::to_string(&seed.member_repos)?, serde_json::to_string(&seed.member_paths)?, seed.provisional as i64, now],
        )?;
        conn.execute(
            "INSERT INTO project_slug_aliases (slug, project_id, status, claimed_at)
             SELECT slug, project_id, 'tentative', ?2 FROM projects WHERE slug = ?1 ON CONFLICT(slug) DO NOTHING",
            params![seed.slug, now],
        )?;
        Ok(())
    })();
    match r {
        Ok(()) => conn.execute_batch("RELEASE project_write")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO project_write; RELEASE project_write");
            return Err(e);
        }
    }
    Ok(())
}

/// New project with a fresh UUIDv4 identity; the legacy `slug` key stores the UUID.
pub fn create_local_only_project(conn: &Connection, seed: &ProjectSeed) -> Result<Project> {
    if !is_valid_project_slug(&seed.slug) {
        return Err(Error::InvalidInput("invalid_project_slug".into()));
    }
    let project_id = uuid::Uuid::new_v4().to_string();
    let now = now_iso();
    conn.execute_batch("SAVEPOINT project_write")?;
    let r = (|| -> Result<()> {
        conn.execute(
            "INSERT INTO projects (slug, project_id, current_slug, identity_state, display_name, member_repos, member_paths, provisional, created_at, updated_at)
             VALUES (?1, ?1, ?2, 'local_only', ?3, ?4, ?5, ?6, ?7, ?7)",
            params![project_id, seed.slug, seed.display_name, serde_json::to_string(&seed.member_repos)?, serde_json::to_string(&seed.member_paths)?, seed.provisional as i64, now],
        )?;
        conn.execute(
            "INSERT INTO project_slug_aliases (slug, project_id, status, claimed_at) VALUES (?1, ?2, 'tentative', ?3)",
            params![seed.slug, project_id, now],
        )?;
        Ok(())
    })();
    match r {
        Ok(()) => conn.execute_batch("RELEASE project_write")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO project_write; RELEASE project_write");
            return Err(e);
        }
    }
    get_project_by_id(conn, &project_id)?
        .ok_or_else(|| Error::NotFound("project_creation_failed".into()))
}

pub fn get_project_by_id(conn: &Connection, project_id: &str) -> Result<Option<Project>> {
    Ok(conn
        .query_row(
            "SELECT * FROM projects WHERE project_id = ?1",
            params![project_id],
            row_to_project,
        )
        .optional()?)
}

pub fn get_project_by_alias(conn: &Connection, slug: &str) -> Result<Option<Project>> {
    Ok(conn
        .query_row(
            "SELECT projects.* FROM project_slug_aliases JOIN projects ON projects.project_id = project_slug_aliases.project_id
             WHERE project_slug_aliases.slug = ?1",
            params![slug],
            row_to_project,
        )
        .optional()?)
}

pub fn get_project_by_slug(conn: &Connection, slug: &str) -> Result<Option<Project>> {
    Ok(conn
        .query_row(
            "SELECT * FROM projects WHERE slug = ?1",
            params![slug],
            row_to_project,
        )
        .optional()?)
}

pub fn list_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare("SELECT * FROM projects ORDER BY slug")?;
    let rows = stmt.query_map([], row_to_project)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn find_project_by_repo(conn: &Connection, remote: &str) -> Result<Option<Project>> {
    let target = normalize_git_remote(remote);
    Ok(list_projects(conn)?.into_iter().find(|p| {
        p.member_repos
            .iter()
            .any(|r| normalize_git_remote(r) == target)
    }))
}

/// Real path when it exists, otherwise a lexically absolute path (mirrors `path.resolve`).
pub fn canonical_path(input: &str) -> PathBuf {
    let p = Path::new(input);
    if p.exists()
        && let Ok(real) = std::fs::canonicalize(p)
    {
        return real;
    }
    if p.is_absolute() {
        lexical_normalize(p)
    } else {
        lexical_normalize(
            &std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .join(p),
        )
    }
}

fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Registered project whose `member_paths` holds the longest prefix of `dir`.
pub fn find_project_by_path(conn: &Connection, dir: &str) -> Result<Option<Project>> {
    let target = canonical_path(dir);
    let mut best: Option<(usize, Project)> = None;
    for project in list_projects(conn)? {
        for member in &project.member_paths {
            let candidate = canonical_path(member);
            let len = candidate.as_os_str().len();
            let matches = target == candidate || target.starts_with(&candidate);
            if matches && best.as_ref().is_none_or(|(l, _)| len > *l) {
                best = Some((len, project.clone()));
            }
        }
    }
    Ok(best.map(|(_, p)| p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;

    fn seed(slug: &str, paths: &[&str]) -> ProjectSeed {
        ProjectSeed {
            slug: slug.into(),
            display_name: slug.into(),
            member_paths: paths.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn git_remote_normalization_matches_swift() {
        assert_eq!(
            normalize_git_remote("git@github.com:Studio-Moser/Shelby-MCP.git"),
            "github.com/Studio-Moser/Shelby-MCP"
        );
        assert_eq!(
            normalize_git_remote("https://github.com/Studio-Moser/Shelby-MCP.git"),
            "github.com/Studio-Moser/Shelby-MCP"
        );
        assert_eq!(
            normalize_git_remote("http://host/org/repo"),
            "host/org/repo"
        );
        assert_eq!(
            normalize_git_remote("ssh://git@host/org/repo.git"),
            "ssh://git@host/org/repo"
        );
    }

    #[test]
    fn upsert_derives_v5_identity_and_create_uses_v4() {
        let m = Memory::open_in_memory().unwrap();
        upsert_project(&m.conn, &seed("shelby", &[])).unwrap();
        upsert_project(
            &m.conn,
            &ProjectSeed {
                display_name: "Shelby!".into(),
                ..seed("shelby", &[])
            },
        )
        .unwrap();
        let p = get_project_by_alias(&m.conn, "shelby").unwrap().unwrap();
        assert_eq!(p.project_id, "4abbc729-70e8-5120-9cfc-bd34739c024e");
        assert_eq!(p.display_name, "Shelby!");
        let created = create_local_only_project(&m.conn, &seed("fresh", &["/tmp/x"])).unwrap();
        assert_eq!(
            created.slug, created.project_id,
            "legacy key holds the uuid"
        );
        assert_eq!(
            get_project_by_id(&m.conn, &created.project_id)
                .unwrap()
                .unwrap()
                .current_slug,
            "fresh"
        );
        assert!(get_project_by_slug(&m.conn, "fresh").unwrap().is_none());
        assert!(create_local_only_project(&m.conn, &seed("Bad Slug", &[])).is_err());
        assert_eq!(list_projects(&m.conn).unwrap().len(), 2);
    }

    #[test]
    fn longest_member_path_and_repo_lookup() {
        let m = Memory::open_in_memory().unwrap();
        upsert_project(&m.conn, &seed("workspace", &["/workspace"])).unwrap();
        upsert_project(
            &m.conn,
            &ProjectSeed {
                member_repos: vec!["git@github.com:org/alpha.git".into()],
                ..seed("alpha", &["/workspace/alpha"])
            },
        )
        .unwrap();
        assert_eq!(
            find_project_by_path(&m.conn, "/workspace/alpha/packages/app")
                .unwrap()
                .unwrap()
                .current_slug,
            "alpha"
        );
        assert_eq!(
            find_project_by_path(&m.conn, "/workspace/beta")
                .unwrap()
                .unwrap()
                .current_slug,
            "workspace"
        );
        assert!(
            find_project_by_path(&m.conn, "/elsewhere")
                .unwrap()
                .is_none()
        );
        assert!(
            find_project_by_path(&m.conn, "/workspacex")
                .unwrap()
                .is_none(),
            "prefix must end at a path separator"
        );
        assert_eq!(
            find_project_by_repo(&m.conn, "https://github.com/org/alpha")
                .unwrap()
                .unwrap()
                .current_slug,
            "alpha"
        );
    }
}

//! Project scope resolution (ADR 0001 §1b): registry-first, fail closed.
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

use crate::detect::detect_project;
use crate::error::Result;
use crate::identity::{is_canonical_project_id, slugify};
use crate::projects::{
    ProjectSeed, canonical_path, create_local_only_project, find_project_by_path,
    find_project_by_repo, get_project_by_alias, get_project_by_id, normalize_git_remote,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectReference {
    Resolved {
        project_id: String,
        current_slug: String,
    },
    InvalidProjectId,
    UnknownProjectId,
    UnknownAlias,
    ConflictingProjectScope,
    Unresolved,
}

impl ProjectReference {
    pub fn kind(&self) -> &'static str {
        match self {
            ProjectReference::Resolved { .. } => "resolved",
            ProjectReference::InvalidProjectId => "invalid_project_id",
            ProjectReference::UnknownProjectId => "unknown_project_id",
            ProjectReference::UnknownAlias => "unknown_alias",
            ProjectReference::ConflictingProjectScope => "conflicting_project_scope",
            ProjectReference::Unresolved => "unresolved",
        }
    }
}

/// Resolve immutable and compatibility references without mutating the registry.
pub fn resolve_project_reference(
    conn: &Connection,
    project_id: Option<&str>,
    project_identifier: Option<&str>,
) -> Result<ProjectReference> {
    if let Some(id) = project_id
        && !is_canonical_project_id(id)
    {
        return Ok(ProjectReference::InvalidProjectId);
    }
    let by_id = match project_id {
        Some(id) => match get_project_by_id(conn, id)? {
            Some(p) => Some(p),
            None => return Ok(ProjectReference::UnknownProjectId),
        },
        None => None,
    };
    let by_alias = match project_identifier {
        Some(slug) => match get_project_by_alias(conn, slug)? {
            Some(p) => Some(p),
            None => return Ok(ProjectReference::UnknownAlias),
        },
        None => None,
    };
    if let (Some(a), Some(b)) = (&by_id, &by_alias)
        && a.project_id != b.project_id
    {
        return Ok(ProjectReference::ConflictingProjectScope);
    }
    Ok(match by_id.or(by_alias) {
        Some(p) => ProjectReference::Resolved {
            project_id: p.project_id,
            current_slug: p.current_slug,
        },
        None => ProjectReference::Unresolved,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeSource {
    MemberPath,
    GitRemote,
    Derived,
    Explicit,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScopeResolution {
    Resolved {
        slug: String,
        source: ScopeSource,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        member_paths: Vec<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        member_repos: Vec<String>,
    },
    Unresolved,
    Ambiguous {
        slugs: Vec<String>,
    },
    InvalidExplicit {
        slug: String,
    },
}

enum PathResolution {
    Resolved(ScopeResolution),
    SlugCollision,
}

fn resolve_path(conn: &Connection, input: &str) -> Result<Option<PathResolution>> {
    let cwd = canonical_path(input);
    if let Some(p) = find_project_by_path(conn, &cwd.to_string_lossy())? {
        return Ok(Some(PathResolution::Resolved(ScopeResolution::Resolved {
            slug: p.current_slug,
            source: ScopeSource::MemberPath,
            member_paths: vec![],
            member_repos: vec![],
        })));
    }
    let Some(detected) = detect_project(&cwd) else {
        return Ok(None);
    };
    let project_root = canonical_path(&detected.project_root.to_string_lossy())
        .to_string_lossy()
        .into_owned();
    if let Some(remote) = detected.remote {
        let normalized = normalize_git_remote(&remote);
        if let Some(p) = find_project_by_repo(conn, &normalized)? {
            return Ok(Some(PathResolution::Resolved(ScopeResolution::Resolved {
                slug: p.current_slug,
                source: ScopeSource::GitRemote,
                member_paths: vec![],
                member_repos: vec![],
            })));
        }
        let basename = Path::new(&normalized)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let slug = slugify(&basename);
        if get_project_by_alias(conn, &slug)?.is_some() {
            return Ok(Some(PathResolution::SlugCollision));
        }
        return Ok(Some(PathResolution::Resolved(ScopeResolution::Resolved {
            slug,
            source: ScopeSource::Derived,
            member_paths: vec![project_root],
            member_repos: vec![normalized],
        })));
    }
    let basename = Path::new(&project_root)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let slug = slugify(&basename);
    if get_project_by_alias(conn, &slug)?.is_some() {
        return Ok(Some(PathResolution::SlugCollision));
    }
    Ok(Some(PathResolution::Resolved(ScopeResolution::Resolved {
        slug,
        source: ScopeSource::Derived,
        member_paths: vec![project_root],
        member_repos: vec![],
    })))
}

/// Resolve explicit or multi-root scope without mutating the registry.
pub fn resolve_project_scope(
    conn: &Connection,
    paths: &[String],
    explicit: Option<&str>,
) -> Result<ScopeResolution> {
    if let Some(explicit) = explicit {
        let project = get_project_by_alias(conn, explicit)?;
        return Ok(match project {
            Some(p) if slugify(explicit) == explicit => ScopeResolution::Resolved {
                slug: p.current_slug,
                source: ScopeSource::Explicit,
                member_paths: vec![],
                member_repos: vec![],
            },
            _ => ScopeResolution::InvalidExplicit {
                slug: explicit.to_string(),
            },
        });
    }
    let mut resolved: Vec<ScopeResolution> = Vec::new();
    for candidate in paths {
        match resolve_path(conn, candidate)? {
            Some(PathResolution::SlugCollision) => return Ok(ScopeResolution::Unresolved),
            Some(PathResolution::Resolved(r)) => resolved.push(r),
            None => {}
        }
    }
    if resolved.is_empty() {
        return Ok(ScopeResolution::Unresolved);
    }
    let mut slugs: Vec<String> = resolved
        .iter()
        .filter_map(|r| match r {
            ScopeResolution::Resolved { slug, .. } => Some(slug.clone()),
            _ => None,
        })
        .collect();
    slugs.sort();
    slugs.dedup();
    if slugs.len() > 1 {
        return Ok(ScopeResolution::Ambiguous { slugs });
    }
    let source_of = |r: &ScopeResolution| match r {
        ScopeResolution::Resolved { source, .. } => *source,
        _ => ScopeSource::Explicit,
    };
    resolved.sort_by_key(source_of);
    let best = resolved[0].clone();
    if source_of(&best) != ScopeSource::Derived {
        return Ok(best);
    }
    let mut member_paths = Vec::new();
    let mut member_repos = Vec::new();
    for r in &resolved {
        if let ScopeResolution::Resolved {
            member_paths: mp,
            member_repos: mr,
            ..
        } = r
        {
            for p in mp {
                if !member_paths.contains(p) {
                    member_paths.push(p.clone());
                }
            }
            for x in mr {
                if !member_repos.contains(x) {
                    member_repos.push(x.clone());
                }
            }
        }
    }
    let ScopeResolution::Resolved { slug, source, .. } = best else {
        unreachable!()
    };
    Ok(ScopeResolution::Resolved {
        slug,
        source,
        member_paths,
        member_repos,
    })
}

/// Persist a detected derived scope before a personal capture.
pub fn upsert_provisional_project(conn: &Connection, resolution: &ScopeResolution) -> Result<()> {
    if let ScopeResolution::Resolved {
        slug,
        source: ScopeSource::Derived,
        member_paths,
        member_repos,
    } = resolution
        && get_project_by_alias(conn, slug)?.is_none()
    {
        create_local_only_project(
            conn,
            &ProjectSeed {
                slug: slug.clone(),
                display_name: slug.clone(),
                member_repos: member_repos.clone(),
                member_paths: member_paths.clone(),
                provisional: true,
            },
        )?;
    }
    Ok(())
}

/// Decode, standardize, and dedupe every `file://` root a client advertises.
pub fn normalize_file_roots(root_uris: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for uri in root_uris {
        let Some(rest) = uri.strip_prefix("file://") else {
            continue;
        };
        // strip an optional host segment ("file://localhost/x" → "/x")
        let path_part = match rest.find('/') {
            Some(0) => rest.to_string(),
            Some(i) => rest[i..].to_string(),
            None => continue,
        };
        let decoded = percent_decode(&path_part);
        let mut standardized = canonical_path(&decoded).to_string_lossy().into_owned();
        if standardized.len() > 1 {
            while standardized.ends_with('/') {
                standardized.pop();
            }
        }
        if !out.contains(&standardized) {
            out.push(standardized);
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// cwd is a fallback only for no-roots clients, and production `/` is never a project.
pub fn fallback_resolution_roots(cwd: &str) -> Vec<String> {
    let p = Path::new(cwd);
    if !p.is_absolute() || cwd == "/" || !p.is_dir() {
        return vec![];
    }
    vec![canonical_path(cwd).to_string_lossy().into_owned()]
}

/// Effective read scope after defaults (ADR 0001 §7): exact project, or shared-only fail-safe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppliedScope {
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub include_shared: bool,
    pub shared_only: bool,
    pub all_projects: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeArgs<'a> {
    pub project_id: Option<&'a str>,
    pub project_identifier: Option<&'a str>,
    pub include_shared: Option<bool>,
    pub shared_only: bool,
    pub all_projects: bool,
}

pub fn apply_default_scope(
    conn: &Connection,
    args: &ScopeArgs,
    paths: &[String],
) -> Result<std::result::Result<AppliedScope, ProjectReference>> {
    let explicit = args.project_id.is_some() || args.project_identifier.is_some();
    if explicit {
        return Ok(
            match resolve_project_reference(conn, args.project_id, args.project_identifier)? {
                ProjectReference::Resolved {
                    project_id,
                    current_slug,
                } => Ok(AppliedScope {
                    project_id: Some(project_id),
                    project_identifier: Some(current_slug),
                    include_shared: args.include_shared.unwrap_or(true),
                    shared_only: args.shared_only,
                    all_projects: args.all_projects,
                }),
                other => Err(other),
            },
        );
    }
    if args.all_projects {
        return Ok(Ok(AppliedScope {
            project_id: None,
            project_identifier: None,
            include_shared: args.include_shared.unwrap_or(false),
            shared_only: args.shared_only,
            all_projects: true,
        }));
    }
    let fail_safe = AppliedScope {
        project_id: None,
        project_identifier: None,
        include_shared: args.include_shared.unwrap_or(false),
        shared_only: true,
        all_projects: false,
    };
    let ScopeResolution::Resolved { slug, .. } = resolve_project_scope(conn, paths, None)? else {
        return Ok(Ok(fail_safe));
    };
    Ok(Ok(
        match resolve_project_reference(conn, None, Some(&slug))? {
            ProjectReference::Resolved {
                project_id,
                current_slug,
            } => AppliedScope {
                project_id: Some(project_id),
                project_identifier: Some(current_slug),
                include_shared: args.include_shared.unwrap_or(true),
                shared_only: args.shared_only,
                all_projects: false,
            },
            _ => fail_safe,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;
    use crate::projects::upsert_project;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/project-scope-resolution.json");

    #[test]
    fn scope_resolution_fixture() {
        let fx: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        for case in fx["cases"].as_array().unwrap() {
            let m = Memory::open_in_memory().unwrap();
            for p in case["projects"].as_array().unwrap() {
                upsert_project(
                    &m.conn,
                    &ProjectSeed {
                        slug: p["slug"].as_str().unwrap().into(),
                        display_name: p["slug"].as_str().unwrap().into(),
                        member_paths: p["member_paths"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_str().unwrap().into())
                            .collect(),
                        member_repos: p["member_repos"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_str().unwrap().into())
                            .collect(),
                        provisional: false,
                    },
                )
                .unwrap();
            }
            let roots: Vec<String> = case["roots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().into())
                .collect();
            let got = resolve_project_scope(&m.conn, &roots, case["explicit"].as_str()).unwrap();
            let got_json = serde_json::to_value(&got).unwrap();
            assert_eq!(got_json, case["expected"], "case: {}", case["name"]);
        }
    }

    #[test]
    fn reference_resolution_fails_closed() {
        let m = Memory::open_in_memory().unwrap();
        upsert_project(
            &m.conn,
            &ProjectSeed {
                slug: "shelby".into(),
                display_name: "Shelby".into(),
                ..Default::default()
            },
        )
        .unwrap();
        upsert_project(
            &m.conn,
            &ProjectSeed {
                slug: "other".into(),
                display_name: "Other".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let shelby = "4abbc729-70e8-5120-9cfc-bd34739c024e";
        assert_eq!(
            resolve_project_reference(&m.conn, Some("NOT-A-UUID"), None).unwrap(),
            ProjectReference::InvalidProjectId
        );
        assert_eq!(
            resolve_project_reference(&m.conn, Some("11111111-1111-4111-8111-111111111111"), None)
                .unwrap(),
            ProjectReference::UnknownProjectId
        );
        assert_eq!(
            resolve_project_reference(&m.conn, None, Some("ghost")).unwrap(),
            ProjectReference::UnknownAlias
        );
        assert_eq!(
            resolve_project_reference(&m.conn, Some(shelby), Some("other")).unwrap(),
            ProjectReference::ConflictingProjectScope
        );
        assert_eq!(
            resolve_project_reference(&m.conn, None, None).unwrap(),
            ProjectReference::Unresolved
        );
        assert_eq!(
            resolve_project_reference(&m.conn, Some(shelby), Some("shelby")).unwrap(),
            ProjectReference::Resolved {
                project_id: shelby.into(),
                current_slug: "shelby".into()
            }
        );
    }

    #[test]
    fn default_scope_is_shared_only_when_unresolved() {
        let m = Memory::open_in_memory().unwrap();
        let args = ScopeArgs {
            project_id: None,
            project_identifier: None,
            include_shared: None,
            shared_only: false,
            all_projects: false,
        };
        let applied = apply_default_scope(&m.conn, &args, &["/nonexistent/markerless".into()])
            .unwrap()
            .unwrap();
        assert!(applied.shared_only && applied.project_id.is_none());
        upsert_project(
            &m.conn,
            &ProjectSeed {
                slug: "alpha".into(),
                display_name: "a".into(),
                member_paths: vec!["/workspace/alpha".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let applied = apply_default_scope(&m.conn, &args, &["/workspace/alpha/src".into()])
            .unwrap()
            .unwrap();
        assert_eq!(applied.project_identifier.as_deref(), Some("alpha"));
        assert!(applied.include_shared && !applied.shared_only);
        let bad = ScopeArgs {
            project_identifier: Some("nope"),
            ..args.clone()
        };
        assert_eq!(
            apply_default_scope(&m.conn, &bad, &[])
                .unwrap()
                .unwrap_err(),
            ProjectReference::UnknownAlias
        );
        let all = ScopeArgs {
            all_projects: true,
            ..args.clone()
        };
        assert!(
            apply_default_scope(&m.conn, &all, &[])
                .unwrap()
                .unwrap()
                .all_projects
        );
    }

    #[test]
    fn roots_and_fallback() {
        assert_eq!(
            normalize_file_roots(&[
                "file:///workspace/x/".into(),
                "file:///workspace/x".into(),
                "https://nope".into(),
                "file:///a%20b".into()
            ]),
            vec!["/workspace/x".to_string(), "/a b".to_string()]
        );
        assert!(fallback_resolution_roots("/").is_empty());
        assert!(fallback_resolution_roots("relative").is_empty());
        assert!(fallback_resolution_roots("/definitely/missing").is_empty());
        assert_eq!(fallback_resolution_roots("/tmp").len(), 1);
    }

    #[test]
    fn provisional_upsert_only_for_derived() {
        let m = Memory::open_in_memory().unwrap();
        let derived = ScopeResolution::Resolved {
            slug: "newproj".into(),
            source: ScopeSource::Derived,
            member_paths: vec!["/w/newproj".into()],
            member_repos: vec![],
        };
        upsert_provisional_project(&m.conn, &derived).unwrap();
        upsert_provisional_project(&m.conn, &derived).unwrap();
        let p = get_project_by_alias(&m.conn, "newproj").unwrap().unwrap();
        assert!(p.provisional);
        let explicit = ScopeResolution::Resolved {
            slug: "x".into(),
            source: ScopeSource::Explicit,
            member_paths: vec![],
            member_repos: vec![],
        };
        upsert_provisional_project(&m.conn, &explicit).unwrap();
        assert!(get_project_by_alias(&m.conn, "x").unwrap().is_none());
    }
}

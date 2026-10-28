//! Filesystem project detection: walk up to the nearest marker, read the git origin.
use std::path::{Path, PathBuf};

const MARKERS: [&str; 7] = [
    ".git",
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "go.mod",
    "Package.swift",
    "Makefile",
];

#[derive(Debug, Clone, PartialEq)]
pub struct DetectedProject {
    pub project_root: PathBuf,
    pub remote: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn detect_project(cwd: &Path) -> Option<DetectedProject> {
    let mut dir = crate::projects::canonical_path(&cwd.to_string_lossy());
    let home = home_dir();
    loop {
        if MARKERS.iter().any(|m| dir.join(m).exists()) {
            return Some(DetectedProject {
                remote: read_origin_remote(&dir),
                project_root: dir,
            });
        }
        let Some(parent) = dir.parent().map(Path::to_path_buf) else {
            break;
        };
        if home.as_ref() == Some(&dir) || parent == dir {
            break;
        }
        dir = parent;
    }
    None
}

fn read_origin_remote(project_root: &Path) -> Option<String> {
    let dot_git = project_root.join(".git");
    let mut cfg = dot_git.join("config");
    let meta = std::fs::metadata(&dot_git).ok()?;
    if meta.is_file() {
        let pointer = std::fs::read_to_string(&dot_git).ok()?;
        let pointer = pointer.trim();
        let rest = pointer
            .strip_prefix("gitdir:")
            .or_else(|| pointer.strip_prefix("GITDIR:"))?
            .trim();
        if rest.is_empty() {
            return None;
        }
        let git_dir = crate::projects::canonical_path(&project_root.join(rest).to_string_lossy());
        cfg = git_dir.join("..").join("..").join("config");
    }
    let text = std::fs::read_to_string(cfg).ok()?;
    let mut in_origin = false;
    for line in text.lines() {
        let t = line.trim();
        if t == "[remote \"origin\"]" {
            in_origin = true;
            continue;
        }
        if t.starts_with('[') && in_origin {
            break;
        }
        if in_origin
            && t.starts_with("url")
            && let Some(eq) = t.find('=')
        {
            let v = t[eq + 1..].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_marker_root_and_origin() {
        let root = std::env::temp_dir().join(format!("shelby-detect-{}", uuid::Uuid::new_v4()));
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git").join("config"), "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com:org/repo.git\n[branch \"main\"]\n").unwrap();
        let d = detect_project(&nested).unwrap();
        assert_eq!(d.project_root, std::fs::canonicalize(&root).unwrap());
        assert_eq!(d.remote.as_deref(), Some("git@github.com:org/repo.git"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn worktree_gitdir_pointer_is_followed() {
        let base = std::env::temp_dir().join(format!("shelby-detect-wt-{}", uuid::Uuid::new_v4()));
        let main = base.join("main");
        let wt = base.join("wt");
        std::fs::create_dir_all(main.join(".git").join("worktrees").join("wt")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(
            main.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = https://h/o/r.git\n",
        )
        .unwrap();
        std::fs::write(
            wt.join(".git"),
            format!(
                "gitdir: {}\n",
                main.join(".git").join("worktrees").join("wt").display()
            ),
        )
        .unwrap();
        let d = detect_project(&wt).unwrap();
        assert_eq!(d.remote.as_deref(), Some("https://h/o/r.git"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn markerless_dir_is_none() {
        let dir = std::env::temp_dir().join(format!("shelby-detect-none-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // temp dirs live under /var or /tmp which carry no markers up to root
        assert!(detect_project(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

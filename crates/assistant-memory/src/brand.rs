//! The product's name, held once for the workspace (ADR-0012 D1), and the server's data folder.
//!
//! Every brand-visible string reads these. `assistant-integrations` depends on nothing, so it
//! repeats `SERVER_NAME` and `DISPLAY_NAME`; a test in `assistant-mcp` checks they agree, and
//! another fails on a brand literal anywhere else in `src`.

use std::path::{Path, PathBuf};

#[doc(hidden)]
#[macro_export]
macro_rules! product_name {
    () => {
        "Advisaur"
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! product_slug {
    () => {
        "advisaur"
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! product_env {
    () => {
        "ADVISAUR"
    };
}

pub const PRODUCT_NAME: &str = product_name!();
pub const PRODUCT_SLUG: &str = product_slug!();
/// The server's npm package, binary, client config key and `serverInfo` name.
pub const SERVER_NAME: &str = concat!(product_slug!(), "-mcp");
/// The server in prose and messages.
pub const DISPLAY_NAME: &str = concat!(product_name!(), " MCP");
/// The `serverInfo` title.
pub const SERVER_TITLE: &str = concat!(product_name!(), " memory");
pub const STATUS_URI: &str = concat!(product_slug!(), "-mcp://status");

/// The data folder under the home directory (ADR-0012 D2): the database and the project seed.
pub const DATA_DIR_NAME: &str = ".studiomoser-assistant-mcp";
/// The folder the server used before the rename. It's moved once, when the server runs with
/// its default database (ADR-0012 D6).
pub const LEGACY_DATA_DIR_NAME: &str = ".shelbymcp";

/// The data folder: the renamed one, or the old one while it hasn't moved yet.
pub fn data_dir(home: &Path) -> PathBuf {
    let current = home.join(DATA_DIR_NAME);
    let legacy = home.join(LEGACY_DATA_DIR_NAME);
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}

/// Whether `path` is a folder itself, not a link to one.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// Whether the old folder has moved: it's gone, or it's the link the move left, to the renamed
/// folder. A link the user made to somewhere else is theirs, and so is a real folder.
pub fn legacy_dir_moved(home: &Path) -> bool {
    let legacy = home.join(LEGACY_DATA_DIR_NAME);
    let current = home.join(DATA_DIR_NAME);
    if !current.is_dir() {
        return false;
    }
    match std::fs::symlink_metadata(&legacy) {
        Err(_) => true,
        Ok(metadata) if metadata.file_type().is_symlink() => {
            std::fs::read_link(&legacy).is_ok_and(|target| target == current)
        }
        Ok(_) => false,
    }
}

/// A path inside the old folder, pointed into the renamed one once the folder has moved. Covers a
/// database path a client set before the rename, and the moment between the move and the link
/// that takes the old folder's place.
pub fn redirect_legacy_path(home: &Path, path: &Path) -> PathBuf {
    match path.strip_prefix(home.join(LEGACY_DATA_DIR_NAME)) {
        Ok(rest) if legacy_dir_moved(home) => home.join(DATA_DIR_NAME).join(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(unix)]
fn link_legacy(legacy: &Path, current: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(current, legacy)
}

#[cfg(windows)]
fn link_legacy(legacy: &Path, current: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(current, legacy)
}

#[cfg(not(any(unix, windows)))]
fn link_legacy(_legacy: &Path, _current: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("links aren't supported here"))
}

/// Moves the old data folder to the renamed one when only the old exists, and leaves a link in
/// its place, so a client still configured with a path inside it reaches the same database.
/// Returns what to tell the user: the move, a failed move (the old folder stays in use), or two
/// real folders, which it never merges.
pub fn move_legacy_data_dir(home: &Path) -> Option<String> {
    let current = home.join(DATA_DIR_NAME);
    let legacy = home.join(LEGACY_DATA_DIR_NAME);
    if !is_real_dir(&legacy) {
        return None;
    }
    if current.exists() {
        return Some(format!(
            "Two memory folders exist: {} is the one the server uses by default, and {} is the old \
             one. Move what you need from the old one, then remove it.",
            current.display(),
            legacy.display()
        ));
    }
    if let Err(error) = std::fs::rename(&legacy, &current) {
        return Some(format!(
            "Couldn't move {} to {} ({error}); using the old folder",
            legacy.display(),
            current.display()
        ));
    }
    Some(match link_legacy(&legacy, &current) {
        Ok(()) => format!(
            "Moved the memory database from {} to {}, and left a link in its place (ADR-0012)",
            legacy.display(),
            current.display()
        ),
        Err(error) => format!(
            "Moved the memory database from {} to {} (ADR-0012); couldn't link the old folder \
             ({error}), so paths inside it are redirected",
            legacy.display(),
            current.display()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        let path = std::env::temp_dir().join(format!("assistant-brand-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn names_derive_from_the_slug() {
        assert_eq!(SERVER_NAME, "advisaur-mcp");
        assert_eq!(STATUS_URI, "advisaur-mcp://status");
        assert_eq!(SERVER_TITLE, "Advisaur memory");
    }

    #[test]
    fn the_old_folder_moves_once_leaving_a_link_and_is_read_until_it_does() {
        let home = home();
        let legacy = home.join(LEGACY_DATA_DIR_NAME);
        let current = home.join(DATA_DIR_NAME);
        assert_eq!(data_dir(&home), current);
        assert_eq!(move_legacy_data_dir(&home), None);

        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("memory.db"), b"memories").unwrap();
        assert_eq!(data_dir(&home), legacy);

        assert!(move_legacy_data_dir(&home).unwrap().starts_with("Moved"));
        assert_eq!(data_dir(&home), current);
        assert_eq!(
            std::fs::read(current.join("memory.db")).unwrap(),
            b"memories"
        );
        // A client still configured inside the old folder reaches the same database.
        if cfg!(unix) {
            assert!(
                std::fs::symlink_metadata(&legacy)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(
                std::fs::read(legacy.join("memory.db")).unwrap(),
                b"memories"
            );
        }
        assert_eq!(
            redirect_legacy_path(&home, &legacy.join("memory.db")),
            current.join("memory.db")
        );
        // Done once: the link isn't a folder to move.
        assert_eq!(move_legacy_data_dir(&home), None);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn two_real_folders_are_reported_never_merged() {
        let home = home();
        let legacy = home.join(LEGACY_DATA_DIR_NAME);
        let current = home.join(DATA_DIR_NAME);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("memory.db"), b"old").unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("memory.db"), b"new").unwrap();
        assert!(
            move_legacy_data_dir(&home)
                .unwrap()
                .starts_with("Two memory folders")
        );
        assert_eq!(std::fs::read(legacy.join("memory.db")).unwrap(), b"old");
        assert_eq!(std::fs::read(current.join("memory.db")).unwrap(), b"new");
        // A real old folder is the user's: paths inside it aren't redirected.
        assert_eq!(
            redirect_legacy_path(&home, &legacy.join("memory.db")),
            legacy.join("memory.db")
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_the_user_made_to_their_own_folder_is_theirs() {
        let home = home();
        let theirs = home.join("team-memory");
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::create_dir_all(home.join(DATA_DIR_NAME)).unwrap();
        std::os::unix::fs::symlink(&theirs, home.join(LEGACY_DATA_DIR_NAME)).unwrap();
        let path = home.join(LEGACY_DATA_DIR_NAME).join("new.db");
        assert_eq!(redirect_legacy_path(&home, &path), path);
        assert_eq!(move_legacy_data_dir(&home), None);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn an_old_path_is_redirected_while_the_old_folder_is_missing() {
        let home = home();
        let current = home.join(DATA_DIR_NAME);
        std::fs::create_dir_all(&current).unwrap();
        let old = home.join(LEGACY_DATA_DIR_NAME).join("memory.db");
        assert_eq!(redirect_legacy_path(&home, &old), current.join("memory.db"));
        let elsewhere = home.join("elsewhere.db");
        assert_eq!(redirect_legacy_path(&home, &elsewhere), elsewhere);
        std::fs::remove_dir_all(&home).unwrap();
    }
}

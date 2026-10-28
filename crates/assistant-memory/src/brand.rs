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

/// Moves the old data folder to the renamed one when only the old exists. Returns what to tell
/// the user, if anything moved or the move failed (then the old folder stays in use).
pub fn move_legacy_data_dir(home: &Path) -> Option<String> {
    let current = home.join(DATA_DIR_NAME);
    let legacy = home.join(LEGACY_DATA_DIR_NAME);
    if current.exists() || !legacy.exists() {
        return None;
    }
    Some(match std::fs::rename(&legacy, &current) {
        Ok(()) => format!(
            "Moved the memory database from {} to {} (ADR-0012)",
            legacy.display(),
            current.display()
        ),
        Err(error) => format!(
            "Couldn't move {} to {} ({error}); using the old folder",
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
    fn the_old_folder_moves_once_and_is_read_until_it_does() {
        let home = home();
        assert_eq!(data_dir(&home), home.join(DATA_DIR_NAME));
        assert_eq!(move_legacy_data_dir(&home), None);

        std::fs::create_dir_all(home.join(LEGACY_DATA_DIR_NAME)).unwrap();
        std::fs::write(
            home.join(LEGACY_DATA_DIR_NAME).join("memory.db"),
            b"memories",
        )
        .unwrap();
        assert_eq!(data_dir(&home), home.join(LEGACY_DATA_DIR_NAME));

        assert!(move_legacy_data_dir(&home).unwrap().starts_with("Moved"));
        assert_eq!(data_dir(&home), home.join(DATA_DIR_NAME));
        assert_eq!(
            std::fs::read(home.join(DATA_DIR_NAME).join("memory.db")).unwrap(),
            b"memories"
        );
        assert!(!home.join(LEGACY_DATA_DIR_NAME).exists());

        // A new folder is never replaced: an old one that turns up later stays where it is.
        std::fs::create_dir_all(home.join(LEGACY_DATA_DIR_NAME)).unwrap();
        assert_eq!(move_legacy_data_dir(&home), None);
        assert_eq!(data_dir(&home), home.join(DATA_DIR_NAME));
        std::fs::remove_dir_all(&home).unwrap();
    }
}

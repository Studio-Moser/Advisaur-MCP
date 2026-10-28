pub mod commands;
pub mod config;
pub mod http;
pub mod oauth;
pub mod prompts;
pub mod schemas;
pub mod server;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn open_memory(path: &str) -> assistant_memory::Result<assistant_memory::Memory> {
    if path == ":memory:" {
        assistant_memory::Memory::open_in_memory()
    } else {
        // Again just before opening: another server may have moved the old folder since. A path
        // still inside it opens only if the folder is there: creating it again would start a
        // second database beside the moved one.
        use assistant_memory::brand::{DATA_DIR_NAME, LEGACY_DATA_DIR_NAME, redirect_legacy_path};
        let home = config::home_dir();
        let legacy = home.join(LEGACY_DATA_DIR_NAME);
        let path = redirect_legacy_path(&home, std::path::Path::new(path));
        match path.strip_prefix(&legacy) {
            // No old folder at all, not even a link: the path means the renamed one. (A link
            // whose target is missing, such as an unmounted drive, is still the user's folder.)
            Ok(rest) if std::fs::symlink_metadata(&legacy).is_err() => {
                assistant_memory::Memory::open(home.join(DATA_DIR_NAME).join(rest))
            }
            Ok(rest) => {
                // Folders below it may be made one at a time, which fails if the old folder
                // itself has gone; it's never made again.
                let mut folder = legacy.clone();
                if let Some(parent) = rest.parent() {
                    for part in parent.components() {
                        folder.push(part);
                        if let Err(error) = std::fs::create_dir(&folder)
                            && error.kind() != std::io::ErrorKind::AlreadyExists
                        {
                            return Err(error.into());
                        }
                    }
                }
                assistant_memory::Memory::open_existing_dir(path)
            }
            Err(_) => assistant_memory::Memory::open(path),
        }
    }
}

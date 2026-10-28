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
        let path = redirect_legacy_path(&home, std::path::Path::new(path));
        match path.strip_prefix(home.join(LEGACY_DATA_DIR_NAME)) {
            // No old folder at all: the path means the renamed one.
            Ok(rest) if !home.join(LEGACY_DATA_DIR_NAME).exists() => {
                assistant_memory::Memory::open(home.join(DATA_DIR_NAME).join(rest))
            }
            Ok(_) => assistant_memory::Memory::open_existing_dir(path),
            Err(_) => assistant_memory::Memory::open(path),
        }
    }
}

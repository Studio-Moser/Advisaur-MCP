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
        // Again just before opening: another server may have moved the old folder since.
        assistant_memory::Memory::open(assistant_memory::brand::redirect_legacy_path(
            &config::home_dir(),
            std::path::Path::new(path),
        ))
    }
}

//! `advisaur-mcp`: command dispatcher and MCP server process.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use assistant_integrations::{Change, Client, CommandRunner, IntegrationPaths, setup, uninstall};
use assistant_mcp::config::{self, Cli, Transport, parse_args};
use assistant_mcp::{VERSION, commands, http, open_memory, server};
use rmcp::ServiceExt;

struct SystemCommandRunner;

fn resolve_program(program: &str, search_path: Option<&OsStr>, windows: bool) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_path_buf());
    }
    let search_path = search_path?;
    std::env::split_paths(search_path).find_map(|directory| {
        if windows
            && let Some(executable) = ["exe", "cmd", "bat", "com"]
                .iter()
                .map(|extension| directory.join(format!("{program}.{extension}")))
                .find(|candidate| candidate.is_file())
        {
            return Some(executable);
        }
        let direct = directory.join(program);
        direct.is_file().then_some(direct)
    })
}

impl CommandRunner for SystemCommandRunner {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        let search_path = std::env::var_os("PATH");
        resolve_program(program, search_path.as_deref(), cfg!(windows))
    }

    fn run(&self, program: &Path, args: &[&str]) -> std::io::Result<bool> {
        Command::new(program)
            .args(args)
            .status()
            .map(|status| status.success())
    }
}

fn integration_paths() -> IntegrationPaths {
    IntegrationPaths {
        home_dir: config::home_dir(),
        app_data_dir: std::env::var_os("APPDATA").map(Into::into),
    }
}

fn parse_client(value: Option<String>, command: &str) -> Result<Client, i32> {
    let Some(value) = value else {
        eprintln!(
            "[ERROR] Missing client.\nUsage: {} {command} <client>",
            assistant_memory::brand::SERVER_NAME
        );
        return Err(1);
    };
    let Some(client) = Client::parse(&value) else {
        eprintln!(
            "[ERROR] Unknown client: {value}\nUsage: {} {command} <client>\nClients: claude-code, claude-desktop, cursor, codex, devin, gemini, antigravity",
            assistant_memory::brand::SERVER_NAME
        );
        return Err(1);
    };
    Ok(client)
}

fn print_change(client: Client, change: Change) -> i32 {
    match change {
        Change::Changed { path, message } => {
            println!(
                "{message} for {} at {}",
                client.info().display_name,
                path.display()
            );
            0
        }
        Change::AlreadyConfigured { path } => {
            println!(
                "{} is already configured for {} at {}",
                assistant_memory::brand::DISPLAY_NAME,
                client.info().display_name,
                path.display()
            );
            0
        }
        Change::AlreadyAbsent { path } => {
            println!(
                "{} is already absent for {} at {}",
                assistant_memory::brand::DISPLAY_NAME,
                client.info().display_name,
                path.display()
            );
            0
        }
        Change::ManualAction { message } => {
            eprintln!("[ACTION] Manual action required: {message}");
            2
        }
    }
}

fn run_setup(client: Option<String>, forage: bool, onboard: bool) -> i32 {
    let client = match parse_client(client, "setup") {
        Ok(client) => client,
        Err(code) => return code,
    };
    let change = match setup(client, &integration_paths(), &SystemCommandRunner) {
        Ok(change) => change,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return 1;
        }
    };
    let code = print_change(client, change);
    if code != 0 {
        return code;
    }
    if forage {
        println!(
            "Forage is included in the {}. Run `{} forage` for the standalone skill body.",
            client.info().package_kind,
            assistant_memory::brand::SERVER_NAME
        );
    }
    if onboard {
        print!("{}", commands::onboard());
    }
    0
}

fn run_uninstall(client: Option<String>) -> i32 {
    let client = match parse_client(client, "uninstall") {
        Ok(client) => client,
        Err(code) => return code,
    };
    match uninstall(client, &integration_paths(), &SystemCommandRunner) {
        Ok(change) => print_change(client, change),
        Err(error) => {
            eprintln!("[ERROR] {error}");
            1
        }
    }
}

fn run_repair(apply: bool) -> i32 {
    let path = config::env_var(config::DB_PATH_ENV)
        .map(|path| config::resolve_db_path(&path))
        .unwrap_or_else(config::default_db_path);
    let mut memory = match open_memory(&path) {
        Ok(memory) => memory,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return 1;
        }
    };
    let seed = assistant_memory::seed::load_seed(&assistant_memory::seed::seed_default_path());
    match assistant_memory::repair::repair_projects(&mut memory.conn, &seed, apply) {
        Ok(report) => {
            eprintln!("{}", commands::format_repair_report(&report, apply));
            0
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            1
        }
    }
}

/// Moves the old data folder to the renamed one (ADR-0012 D6) when the server is about to use
/// its default database, and points the database there. A path given by flag or variable is the
/// user's to keep, so nothing moves then.
fn adopt_data_dir(cli: Cli) -> Cli {
    let uses_default = match &cli {
        Cli::Serve(cfg) => cfg.db_path == config::default_db_path(),
        Cli::RepairProjects { .. } => {
            std::env::var_os(config::DB_PATH_ENV.0).is_none()
                && std::env::var_os(config::DB_PATH_ENV.1).is_none()
        }
        _ => false,
    };
    if !uses_default {
        return cli;
    }
    if let Some(message) = assistant_memory::brand::move_legacy_data_dir(&config::home_dir()) {
        eprintln!("[WARN] {message}");
    }
    match cli {
        Cli::Serve(mut cfg) => {
            cfg.db_path = config::default_db_path();
            Cli::Serve(cfg)
        }
        other => other,
    }
}

async fn run_server(cfg: config::ServeConfig) -> i32 {
    if cfg.verbose {
        eprintln!("[INFO] Verbose mode enabled");
        eprintln!("[INFO] Database: {}", cfg.db_path);
    }
    let memory = match open_memory(&cfg.db_path) {
        Ok(memory) => Arc::new(Mutex::new(memory)),
        Err(error) => {
            eprintln!("[FATAL] {error}");
            return 1;
        }
    };
    server::seed_registry(&memory);
    if cfg.transport == Transport::Http {
        if let Err(error) = http::serve(memory, &cfg).await {
            eprintln!("[FATAL] {error}");
            return 1;
        }
        return 0;
    }
    let service = match server::MemoryServer::new(memory)
        .serve(rmcp::transport::stdio())
        .await
    {
        Ok(service) => service,
        Err(error) => {
            eprintln!("[FATAL] {error}");
            return 1;
        }
    };
    eprintln!(
        "[INFO] {} running on stdio",
        assistant_memory::brand::SERVER_NAME
    );
    let _ = service.waiting().await;
    0
}

async fn run() -> i32 {
    let cli = match parse_args(std::env::args().skip(1).collect()) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("[ERROR] {message}\n\n{}", config::help());
            return 1;
        }
    };
    let cli = adopt_data_dir(cli);
    match cli {
        Cli::Version => {
            println!("{} v{VERSION}", assistant_memory::brand::SERVER_NAME);
            0
        }
        Cli::Help => {
            print!("{}", config::help());
            0
        }
        Cli::Setup {
            client,
            forage,
            onboard,
        } => run_setup(client, forage, onboard),
        Cli::Uninstall { client } => run_uninstall(client),
        Cli::Protocol => {
            print!("{}", commands::protocol());
            0
        }
        Cli::Forage => {
            eprintln!(
                "Install the package-native Forage skill, or paste this body into a scheduled task."
            );
            print!("{}", commands::forage());
            0
        }
        Cli::Onboard => {
            eprintln!("Paste this into a conversation with your primary AI tool.");
            print!("{}", commands::onboard());
            0
        }
        Cli::Migrate => {
            eprintln!("Paste this into an AI tool that knows you, then import its response.");
            print!("{}", commands::migrate());
            0
        }
        Cli::RepairProjects { apply } => run_repair(apply),
        Cli::Serve(cfg) => run_server(cfg).await,
    }
}

#[tokio::main]
async fn main() {
    let code = run().await;
    if code != 0 {
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn windows_resolution_preserves_the_cmd_shim_path() {
        let directory = std::env::temp_dir().join(format!(
            "shelby-command-path-{}-{}",
            std::process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("gemini"), "#!/bin/sh\n").unwrap();
        let shim = directory.join("gemini.cmd");
        fs::write(&shim, "@echo off\r\n").unwrap();
        let search_path = std::env::join_paths([&directory]).unwrap();

        let resolved = resolve_program("gemini", Some(&search_path), true);

        assert_eq!(resolved, Some(shim));
        fs::remove_dir_all(directory).unwrap();
    }
}

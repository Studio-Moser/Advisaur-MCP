use serde_json::json;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::catalog::Client;
use crate::config::{
    IntegrationError, JsonChange, JsonStatus, Result, json_status, merge_json, remove_json,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Changed { path: PathBuf, message: String },
    AlreadyConfigured { path: PathBuf },
    AlreadyAbsent { path: PathBuf },
    ManualAction { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationPaths {
    pub home_dir: PathBuf,
    pub app_data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationStatus {
    Configured { path: PathBuf },
    NotConfigured { path: PathBuf },
    ManualAction { message: String },
}

pub trait CommandRunner {
    fn resolve(&self, program: &str) -> Option<PathBuf>;
    fn run(&self, program: &Path, args: &[&str]) -> io::Result<bool>;
}

fn config_path(client: Client, paths: &IntegrationPaths) -> Option<PathBuf> {
    Some(match client {
        Client::ClaudeCode => paths.home_dir.join(".claude.json"),
        Client::ClaudeDesktop => match &paths.app_data_dir {
            Some(app_data) => app_data.join("Claude/claude_desktop_config.json"),
            None if cfg!(target_os = "macos") => paths
                .home_dir
                .join("Library/Application Support/Claude/claude_desktop_config.json"),
            None if cfg!(target_os = "windows") => paths
                .home_dir
                .join("AppData/Roaming/Claude/claude_desktop_config.json"),
            None => paths
                .home_dir
                .join(".config/Claude/claude_desktop_config.json"),
        },
        Client::Cursor => paths.home_dir.join(".cursor/mcp.json"),
        Client::Codex => paths.home_dir.join(".codex/config.toml"),
        Client::Devin => return None,
        Client::Windsurf => paths.home_dir.join(".codeium/windsurf/mcp_config.json"),
        Client::Gemini => paths.home_dir.join(".gemini/settings.json"),
        Client::Antigravity => paths.home_dir.join(".gemini/config/mcp_config.json"),
    })
}

fn json_client(client: Client) -> bool {
    !matches!(client, Client::Codex)
}

fn codex_status(path: &Path) -> Result<IntegrationStatus> {
    if !path.exists() {
        return Ok(IntegrationStatus::NotConfigured {
            path: path.to_path_buf(),
        });
    }
    let content = fs::read_to_string(path).map_err(|source| IntegrationError::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    let configured = content
        .lines()
        .any(|line| line.trim() == "[mcp_servers.shelbymcp]");
    Ok(if configured {
        IntegrationStatus::Configured {
            path: path.to_path_buf(),
        }
    } else {
        IntegrationStatus::NotConfigured {
            path: path.to_path_buf(),
        }
    })
}

pub fn status(client: Client, paths: &IntegrationPaths) -> Result<IntegrationStatus> {
    let Some(path) = config_path(client, paths) else {
        return Ok(IntegrationStatus::ManualAction {
            message: "Devin MCP integrations are organization-managed; use Settings > MCP Marketplace > Add Your Own"
                .into(),
        });
    };
    if !json_client(client) {
        return codex_status(&path);
    }
    Ok(match json_status(&path)? {
        JsonStatus::Configured => IntegrationStatus::Configured { path },
        JsonStatus::NotConfigured => IntegrationStatus::NotConfigured { path },
        JsonStatus::ManualAction(message) => IntegrationStatus::ManualAction { message },
    })
}

fn portable_entry_for(is_windows: bool) -> serde_json::Value {
    if is_windows {
        json!({ "command": "cmd", "args": ["/c", "npx", "-y", "shelbymcp"] })
    } else {
        json!({ "command": "npx", "args": ["-y", "shelbymcp"] })
    }
}

fn portable_entry() -> serde_json::Value {
    portable_entry_for(cfg!(windows))
}

fn setup_command(client: Client) -> Option<(&'static str, &'static [&'static str])> {
    match client {
        Client::ClaudeCode => Some((
            "claude",
            &[
                "mcp",
                "add",
                "--scope",
                "user",
                "--transport",
                "stdio",
                "shelbymcp",
                "--",
                "npx",
                "-y",
                "shelbymcp",
            ],
        )),
        Client::Codex => Some((
            "codex",
            &["mcp", "add", "shelbymcp", "--", "npx", "-y", "shelbymcp"],
        )),
        Client::Gemini => Some((
            "gemini",
            &[
                "mcp",
                "add",
                "--scope",
                "user",
                "shelbymcp",
                "npx",
                "-y",
                "shelbymcp",
            ],
        )),
        _ => None,
    }
}

fn uninstall_command(client: Client) -> Option<(&'static str, &'static [&'static str])> {
    match client {
        Client::ClaudeCode => Some(("claude", &["mcp", "remove", "shelbymcp"])),
        Client::Codex => Some(("codex", &["mcp", "remove", "shelbymcp"])),
        Client::Gemini => Some(("gemini", &["mcp", "remove", "--scope", "user", "shelbymcp"])),
        _ => None,
    }
}

fn run_command(runner: &dyn CommandRunner, program: &Path, args: &[&str]) -> Result<()> {
    let display = program.display().to_string();
    match runner.run(program, args) {
        Ok(true) => Ok(()),
        Ok(false) => Err(IntegrationError::CommandFailed { program: display }),
        Err(source) => Err(IntegrationError::Command {
            program: display,
            source,
        }),
    }
}

fn changed(path: PathBuf, message: impl Into<String>) -> Change {
    Change::Changed {
        path,
        message: message.into(),
    }
}

fn from_json_change(change: JsonChange, path: PathBuf, action: &str) -> Change {
    match change {
        JsonChange::Changed => changed(path, format!("ShelbyMCP {action}")),
        JsonChange::AlreadyConfigured => Change::AlreadyConfigured { path },
        JsonChange::AlreadyAbsent => Change::AlreadyAbsent { path },
        JsonChange::ManualAction(message) => Change::ManualAction { message },
    }
}

pub fn setup(
    client: Client,
    paths: &IntegrationPaths,
    runner: &dyn CommandRunner,
) -> Result<Change> {
    let Some(path) = config_path(client, paths) else {
        return Ok(Change::ManualAction {
            message: "Devin MCP integrations are organization-managed; use Settings > MCP Marketplace > Add Your Own"
                .into(),
        });
    };
    match status(client, paths)? {
        IntegrationStatus::Configured { .. } => {
            return Ok(Change::AlreadyConfigured { path });
        }
        IntegrationStatus::ManualAction { message } => {
            return Ok(Change::ManualAction { message });
        }
        IntegrationStatus::NotConfigured { .. } => {}
    }

    if let Some((program, args)) = setup_command(client) {
        if let Some(executable) = runner.resolve(program) {
            run_command(runner, &executable, args)?;
            return Ok(changed(path, format!("ShelbyMCP added with {program}")));
        }
        if client != Client::Gemini {
            return Ok(Change::ManualAction {
                message: format!(
                    "{} CLI is unavailable; install the {} or run its MCP setup manually",
                    client.info().display_name,
                    client.info().package_kind
                ),
            });
        }
    }

    Ok(from_json_change(
        merge_json(&path, portable_entry())?,
        path,
        "configured",
    ))
}

pub fn uninstall(
    client: Client,
    paths: &IntegrationPaths,
    runner: &dyn CommandRunner,
) -> Result<Change> {
    let Some(path) = config_path(client, paths) else {
        return Ok(Change::ManualAction {
            message: "Remove ShelbyMCP from Devin in Settings > MCP Marketplace".into(),
        });
    };
    match status(client, paths)? {
        IntegrationStatus::NotConfigured { .. } => return Ok(Change::AlreadyAbsent { path }),
        IntegrationStatus::ManualAction { message } => {
            return Ok(Change::ManualAction { message });
        }
        IntegrationStatus::Configured { .. } => {}
    }

    if let Some((program, args)) = uninstall_command(client) {
        if let Some(executable) = runner.resolve(program) {
            run_command(runner, &executable, args)?;
            return Ok(changed(path, format!("ShelbyMCP removed with {program}")));
        }
        if client != Client::Gemini {
            return Ok(Change::ManualAction {
                message: format!(
                    "{} CLI is unavailable; remove the ShelbyMCP integration manually",
                    client.info().display_name
                ),
            });
        }
    }

    Ok(from_json_change(remove_json(&path)?, path, "removed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CLIENTS, Client};
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_HOME: AtomicU64 = AtomicU64::new(1);

    struct TempHome(PathBuf);

    impl TempHome {
        fn new() -> Self {
            let id = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("shelby-integrations-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn paths(&self) -> IntegrationPaths {
            IntegrationPaths {
                home_dir: self.0.clone(),
                app_data_dir: None,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct FakeRunner {
        resolved: HashMap<String, PathBuf>,
        results: HashMap<String, io::Result<bool>>,
        calls: RefCell<Vec<(String, Vec<String>)>>,
    }

    impl FakeRunner {
        fn succeeds(programs: &[&str]) -> Self {
            Self {
                resolved: programs
                    .iter()
                    .map(|program| ((*program).into(), PathBuf::from(program)))
                    .collect(),
                results: programs
                    .iter()
                    .map(|program| ((*program).into(), Ok(true)))
                    .collect(),
                calls: RefCell::default(),
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn resolve(&self, program: &str) -> Option<PathBuf> {
            self.resolved.get(program).cloned()
        }

        fn run(&self, program: &Path, args: &[&str]) -> io::Result<bool> {
            let program = program.to_string_lossy().into_owned();
            self.calls.borrow_mut().push((
                program.clone(),
                args.iter().map(|value| (*value).into()).collect(),
            ));
            match self.results.get(&program) {
                Some(Ok(result)) => Ok(*result),
                Some(Err(error)) => Err(io::Error::new(error.kind(), error.to_string())),
                None => Ok(false),
            }
        }
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn cursor_path(home: &TempHome) -> PathBuf {
        home.0.join(".cursor/mcp.json")
    }

    #[test]
    fn missing_cursor_config_reports_not_configured() {
        let home = TempHome::new();
        assert!(matches!(
            status(Client::Cursor, &home.paths()).unwrap(),
            IntegrationStatus::NotConfigured { path } if path == cursor_path(&home)
        ));
    }

    #[test]
    fn json_setup_and_uninstall_preserve_unrelated_servers_and_keys() {
        let home = TempHome::new();
        let path = cursor_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "theme": "dark",
                "mcpServers": { "other": { "command": "other" } }
            }))
            .unwrap(),
        )
        .unwrap();
        let runner = FakeRunner::default();

        assert!(matches!(
            setup(Client::Cursor, &home.paths(), &runner).unwrap(),
            Change::Changed { .. }
        ));
        let configured = read_json(&path);
        assert_eq!(configured["theme"], "dark");
        assert_eq!(configured["mcpServers"]["other"]["command"], "other");
        assert_eq!(configured["mcpServers"]["shelbymcp"]["command"], "npx");
        assert_eq!(
            configured["mcpServers"]["shelbymcp"]["args"],
            json!(["-y", "shelbymcp"])
        );

        assert!(matches!(
            uninstall(Client::Cursor, &home.paths(), &runner).unwrap(),
            Change::Changed { .. }
        ));
        let removed = read_json(&path);
        assert_eq!(removed["theme"], "dark");
        assert_eq!(removed["mcpServers"]["other"]["command"], "other");
        assert!(removed["mcpServers"].get("shelbymcp").is_none());
    }

    #[test]
    fn malformed_json_is_left_byte_identical_for_setup_and_uninstall() {
        let home = TempHome::new();
        let path = cursor_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let malformed = b"{ not valid json\n";
        fs::write(&path, malformed).unwrap();
        let runner = FakeRunner::default();

        assert!(matches!(
            setup(Client::Cursor, &home.paths(), &runner).unwrap(),
            Change::ManualAction { .. }
        ));
        assert_eq!(fs::read(&path).unwrap(), malformed);
        assert!(matches!(
            uninstall(Client::Cursor, &home.paths(), &runner).unwrap(),
            Change::ManualAction { .. }
        ));
        assert_eq!(fs::read(&path).unwrap(), malformed);
    }

    #[test]
    fn json_setup_and_uninstall_are_idempotent() {
        let home = TempHome::new();
        let paths = home.paths();
        let runner = FakeRunner::default();

        assert!(matches!(
            setup(Client::Cursor, &paths, &runner).unwrap(),
            Change::Changed { .. }
        ));
        assert!(matches!(
            setup(Client::Cursor, &paths, &runner).unwrap(),
            Change::AlreadyConfigured { .. }
        ));
        assert!(matches!(
            uninstall(Client::Cursor, &paths, &runner).unwrap(),
            Change::Changed { .. }
        ));
        assert!(matches!(
            uninstall(Client::Cursor, &paths, &runner).unwrap(),
            Change::AlreadyAbsent { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn atomic_json_replacement_preserves_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let home = TempHome::new();
        let path = cursor_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{}\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        setup(Client::Cursor, &home.paths(), &FakeRunner::default()).unwrap();

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn available_client_clis_receive_current_portable_commands() {
        let cases = [
            (
                Client::ClaudeCode,
                "claude",
                vec![
                    "mcp",
                    "add",
                    "--scope",
                    "user",
                    "--transport",
                    "stdio",
                    "shelbymcp",
                    "--",
                    "npx",
                    "-y",
                    "shelbymcp",
                ],
            ),
            (
                Client::Codex,
                "codex",
                vec!["mcp", "add", "shelbymcp", "--", "npx", "-y", "shelbymcp"],
            ),
            (
                Client::Gemini,
                "gemini",
                vec![
                    "mcp",
                    "add",
                    "--scope",
                    "user",
                    "shelbymcp",
                    "npx",
                    "-y",
                    "shelbymcp",
                ],
            ),
        ];
        for (client, program, expected) in cases {
            let home = TempHome::new();
            let runner = FakeRunner::succeeds(&[program]);
            assert!(matches!(
                setup(client, &home.paths(), &runner).unwrap(),
                Change::Changed { .. }
            ));
            assert_eq!(
                runner.calls.borrow().as_slice(),
                &[(
                    program.into(),
                    expected.into_iter().map(str::to_owned).collect()
                )]
            );
        }
    }

    #[test]
    fn resolved_cmd_shim_is_executed_verbatim() {
        let home = TempHome::new();
        let shim = PathBuf::from("C:/npm/gemini.cmd");
        let mut runner = FakeRunner::default();
        runner.resolved.insert("gemini".into(), shim.clone());
        runner.results.insert(shim.display().to_string(), Ok(true));

        assert!(matches!(
            setup(Client::Gemini, &home.paths(), &runner).unwrap(),
            Change::Changed { .. }
        ));
        assert_eq!(runner.calls.borrow()[0].0, shim.display().to_string());
    }

    #[test]
    fn failed_client_cli_is_an_error_not_a_false_success() {
        let home = TempHome::new();
        let mut runner = FakeRunner::succeeds(&["codex"]);
        runner.results.insert("codex".into(), Ok(false));

        assert!(matches!(
            setup(Client::Codex, &home.paths(), &runner),
            Err(IntegrationError::CommandFailed { program }) if program == "codex"
        ));
    }

    #[test]
    fn setup_never_writes_global_instruction_files() {
        let home = TempHome::new();
        let paths = home.paths();
        let runner = FakeRunner::default();
        for client in CLIENTS.iter().map(|info| info.client) {
            let _ = setup(client, &paths, &runner).unwrap();
        }

        for forbidden in [
            ".claude/CLAUDE.md",
            ".codex/AGENTS.md",
            ".gemini/GEMINI.md",
            ".cursor/rules/shelbymcp.mdc",
            ".codeium/windsurf/memories/global_rules.md",
        ] {
            assert!(!home.0.join(forbidden).exists(), "wrote {forbidden}");
        }
    }

    #[test]
    fn devin_requires_its_organization_managed_marketplace() {
        let home = TempHome::new();
        let result = setup(Client::Devin, &home.paths(), &FakeRunner::default()).unwrap();

        assert!(matches!(result, Change::ManualAction { .. }));
        assert!(!home.0.join(".codeium/windsurf/mcp_config.json").exists());
    }

    #[test]
    fn windsurf_alias_still_configures_the_legacy_local_client() {
        let home = TempHome::new();
        let result = setup(Client::Windsurf, &home.paths(), &FakeRunner::default()).unwrap();

        assert!(matches!(result, Change::Changed { .. }));
        assert_eq!(
            read_json(&home.0.join(".codeium/windsurf/mcp_config.json"))["mcpServers"]["shelbymcp"]
                ["command"],
            "npx"
        );
    }

    #[test]
    fn windows_json_fallback_wraps_the_npm_shim_without_shell_interpolation() {
        assert_eq!(
            portable_entry_for(true),
            json!({ "command": "cmd", "args": ["/c", "npx", "-y", "shelbymcp"] })
        );
        assert_eq!(
            portable_entry_for(false),
            json!({ "command": "npx", "args": ["-y", "shelbymcp"] })
        );
    }
}

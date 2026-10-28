use serde_json::json;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::catalog::Client;
use crate::config::{
    DISPLAY_NAME, IntegrationError, JsonChange, JsonStatus, Keys, LEGACY_SERVER_KEY, Result,
    SERVER_KEY, is_old_setup_entry, json_keys, json_legacy_entry, json_status, merge_json,
    remove_json,
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

/// The server's keys in Codex's TOML config, under the current name or the old one.
fn codex_keys(path: &Path) -> Result<Keys> {
    let content = read_codex(path)?;
    let has = |key: &str| {
        let table = format!("mcp_servers.{key}");
        content
            .lines()
            .any(|line| toml_table(line).is_some_and(|name| name == table))
    };
    Ok(Keys {
        current: has(SERVER_KEY),
        legacy: has(LEGACY_SERVER_KEY),
    })
}

fn read_codex(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).map_err(|source| IntegrationError::Filesystem {
        path: path.to_path_buf(),
        source,
    })
}

/// A TOML table header's dotted name, unquoted and without spacing or a trailing comment:
/// `[ mcp_servers."advisaur-mcp" ] # memory` is `mcp_servers.advisaur-mcp`.
fn toml_table(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with('[') || line.starts_with("[[") {
        return None;
    }
    let close = line.find(']')?;
    let rest = line[close + 1..].trim();
    if !(rest.is_empty() || rest.starts_with('#')) {
        return None;
    }
    let parts: Vec<String> = line[1..close]
        .split('.')
        .map(|part| unquote_toml_key(part.trim()))
        .collect();
    Some(parts.join("."))
}

/// A TOML key without its quotes: a literal ('…') key as it is, and a basic ("…") one with its
/// escapes decoded, so `"shelby\u006dcp"` reads as the old key it is.
fn unquote_toml_key(key: &str) -> String {
    if let Some(literal) = key
        .strip_prefix('\'')
        .and_then(|key| key.strip_suffix('\''))
    {
        return literal.to_string();
    }
    let Some(basic) = key.strip_prefix('"').and_then(|key| key.strip_suffix('"')) else {
        return key.to_string();
    };
    let mut decoded = String::new();
    let mut chars = basic.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            decoded.push(c);
            continue;
        }
        let width = match chars.next() {
            Some('u') => 4,
            Some('U') => 8,
            Some('n') => {
                decoded.push('\n');
                continue;
            }
            Some('t') => {
                decoded.push('\t');
                continue;
            }
            Some(other) => {
                decoded.push(other);
                continue;
            }
            None => break,
        };
        let hex: String = chars.by_ref().take(width).collect();
        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
            decoded.push(c);
        }
    }
    decoded
}

/// The table Codex holds under the old key, as a JSON entry: its command and args, plus any
/// other key or sub-table, which makes it the user's.
fn codex_legacy_entry(path: &Path) -> Result<Option<serde_json::Value>> {
    let content = read_codex(path)?;
    let table = format!("mcp_servers.{LEGACY_SERVER_KEY}");
    let mut lines = content
        .lines()
        .skip_while(|line| toml_table(line).as_deref() != Some(&table));
    if lines.next().is_none() {
        return Ok(None);
    }
    let mut entry = serde_json::Map::new();
    // A sub-table of the old table (its `env`, say) can sit anywhere in the document.
    let sub_table = format!("{table}.");
    if let Some(name) = content
        .lines()
        .filter_map(toml_table)
        .find(|name| name.starts_with(&sub_table))
    {
        entry.insert("table".into(), json!(name));
    }
    for line in lines {
        let line = line.trim();
        if let Some(name) = toml_table(line) {
            if name.starts_with(&format!("{table}.")) {
                entry.insert("table".into(), json!(name));
                continue;
            }
            break;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            entry.insert("unparsed".into(), json!(line));
            continue;
        };
        // Double-quoted strings and arrays of them read as JSON; anything else stays opaque.
        let value =
            serde_json::from_str(value.trim()).unwrap_or_else(|_| json!({ "toml": value.trim() }));
        entry.insert(key.trim().to_string(), value);
    }
    Ok(Some(serde_json::Value::Object(entry)))
}

/// The entry under the old key, for clients set up through their CLI.
fn legacy_entry(client: Client, path: &Path) -> Result<Option<serde_json::Value>> {
    if json_client(client) {
        json_legacy_entry(path)
    } else {
        codex_legacy_entry(path)
    }
}

/// The server's keys in a client's config, or what the user must fix first.
fn configured_keys(client: Client, path: &Path) -> Result<std::result::Result<Keys, String>> {
    if json_client(client) {
        json_keys(path)
    } else {
        codex_keys(path).map(Ok)
    }
}

fn codex_status(path: &Path) -> Result<IntegrationStatus> {
    let configured = codex_keys(path)?.any();
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

/// The entry that launches `package` from npm.
fn portable_entry_for(is_windows: bool, package: &str) -> serde_json::Value {
    if is_windows {
        json!({ "command": "cmd", "args": ["/c", "npx", "-y", package] })
    } else {
        json!({ "command": "npx", "args": ["-y", package] })
    }
}

fn portable_entry() -> serde_json::Value {
    portable_entry_for(cfg!(windows), SERVER_KEY)
}

fn setup_command(client: Client) -> Option<(&'static str, Vec<&'static str>)> {
    match client {
        Client::ClaudeCode => Some((
            "claude",
            vec![
                "mcp",
                "add",
                "--scope",
                "user",
                "--transport",
                "stdio",
                SERVER_KEY,
                "--",
                "npx",
                "-y",
                SERVER_KEY,
            ],
        )),
        Client::Codex => Some((
            "codex",
            vec!["mcp", "add", SERVER_KEY, "--", "npx", "-y", SERVER_KEY],
        )),
        Client::Gemini => Some((
            "gemini",
            vec![
                "mcp", "add", "--scope", "user", SERVER_KEY, "npx", "-y", SERVER_KEY,
            ],
        )),
        _ => None,
    }
}

/// The client CLI command that removes the server's entry under `key`.
fn uninstall_command(
    client: Client,
    key: &'static str,
) -> Option<(&'static str, Vec<&'static str>)> {
    match client {
        Client::ClaudeCode => Some(("claude", vec!["mcp", "remove", key])),
        Client::Codex => Some(("codex", vec!["mcp", "remove", key])),
        Client::Gemini => Some(("gemini", vec!["mcp", "remove", "--scope", "user", key])),
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
        JsonChange::Changed => changed(path, format!("{DISPLAY_NAME} {action}")),
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
    let keys = match configured_keys(client, &path)? {
        Ok(keys) => keys,
        Err(message) => return Ok(Change::ManualAction { message }),
    };
    if keys.current && !keys.legacy {
        return Ok(Change::AlreadyConfigured { path });
    }

    if let Some((program, args)) = setup_command(client) {
        if let Some(executable) = runner.resolve(program) {
            // Only an entry the old setup wrote is replaced. One the user wrote is theirs: their
            // client's CLI can't move it, so it stays until they do.
            if keys.legacy
                && !legacy_entry(client, &path)?
                    .as_ref()
                    .is_some_and(is_old_setup_entry)
            {
                return Ok(Change::ManualAction {
                    message: format!(
                        "The {LEGACY_SERVER_KEY} entry in {} has settings of yours (a command, arguments or an \
                         environment) that the client's CLI can't carry over. Rename its key to {SERVER_KEY}, \
                         its {LEGACY_SERVER_KEY} package to {SERVER_KEY} if it runs one, and any SHELBY_ \
                         variables to their new names, or remove it and run setup again.",
                        path.display()
                    ),
                });
            }
            if !keys.current {
                run_command(runner, &executable, &args)?;
            }
            // The old entry goes, so the client doesn't run two servers.
            if keys.legacy
                && let Some((_, remove)) = uninstall_command(client, LEGACY_SERVER_KEY)
            {
                run_command(runner, &executable, &remove)?;
            }
            return Ok(changed(
                path,
                format!("{DISPLAY_NAME} added with {program}"),
            ));
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
            message: format!("Remove {DISPLAY_NAME} from Devin in Settings > MCP Marketplace"),
        });
    };
    let keys = match configured_keys(client, &path)? {
        Ok(keys) => keys,
        Err(message) => return Ok(Change::ManualAction { message }),
    };
    if !keys.any() {
        return Ok(Change::AlreadyAbsent { path });
    }

    if let Some((program, _)) = uninstall_command(client, SERVER_KEY) {
        if let Some(executable) = runner.resolve(program) {
            for (present, key) in [(keys.current, SERVER_KEY), (keys.legacy, LEGACY_SERVER_KEY)] {
                if present && let Some((_, args)) = uninstall_command(client, key) {
                    run_command(runner, &executable, &args)?;
                }
            }
            return Ok(changed(
                path,
                format!("{DISPLAY_NAME} removed with {program}"),
            ));
        }
        if client != Client::Gemini {
            return Ok(Change::ManualAction {
                message: format!(
                    "{} CLI is unavailable; remove the {DISPLAY_NAME} integration manually",
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
            let path = std::env::temp_dir().join(format!(
                "assistant-integrations-{}-{id}",
                std::process::id()
            ));
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
        assert_eq!(configured["mcpServers"]["advisaur-mcp"]["command"], "npx");
        assert_eq!(
            configured["mcpServers"]["advisaur-mcp"]["args"],
            json!(["-y", "advisaur-mcp"])
        );

        assert!(matches!(
            uninstall(Client::Cursor, &home.paths(), &runner).unwrap(),
            Change::Changed { .. }
        ));
        let removed = read_json(&path);
        assert_eq!(removed["theme"], "dark");
        assert_eq!(removed["mcpServers"]["other"]["command"], "other");
        assert!(removed["mcpServers"].get("advisaur-mcp").is_none());
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
                    "advisaur-mcp",
                    "--",
                    "npx",
                    "-y",
                    "advisaur-mcp",
                ],
            ),
            (
                Client::Codex,
                "codex",
                vec![
                    "mcp",
                    "add",
                    "advisaur-mcp",
                    "--",
                    "npx",
                    "-y",
                    "advisaur-mcp",
                ],
            ),
            (
                Client::Gemini,
                "gemini",
                vec![
                    "mcp",
                    "add",
                    "--scope",
                    "user",
                    "advisaur-mcp",
                    "npx",
                    "-y",
                    "advisaur-mcp",
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
            ".cursor/rules/advisaur-mcp.mdc",
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
            read_json(&home.0.join(".codeium/windsurf/mcp_config.json"))["mcpServers"]["advisaur-mcp"]
                ["command"],
            "npx"
        );
    }

    #[test]
    fn windows_json_fallback_wraps_the_npm_shim_without_shell_interpolation() {
        assert_eq!(
            portable_entry_for(true, SERVER_KEY),
            json!({ "command": "cmd", "args": ["/c", "npx", "-y", "advisaur-mcp"] })
        );
        assert_eq!(
            portable_entry_for(false, SERVER_KEY),
            json!({ "command": "npx", "args": ["-y", "advisaur-mcp"] })
        );
    }

    #[test]
    fn setup_replaces_the_old_stock_entry_and_uninstall_removes_either() {
        let home = TempHome::new();
        let path = cursor_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = portable_entry_for(cfg!(windows), LEGACY_SERVER_KEY);
        fs::write(
            &path,
            serde_json::to_vec(&json!({ "mcpServers": { "shelbymcp": old } })).unwrap(),
        )
        .unwrap();
        let runner = FakeRunner::default();

        assert!(matches!(
            status(Client::Cursor, &home.paths()).unwrap(),
            IntegrationStatus::Configured { .. }
        ));
        assert!(matches!(
            setup(Client::Cursor, &home.paths(), &runner).unwrap(),
            Change::Changed { .. }
        ));
        let configured = read_json(&path);
        assert!(configured["mcpServers"].get("shelbymcp").is_none());
        assert_eq!(configured["mcpServers"]["advisaur-mcp"], portable_entry());

        // Both keys present: uninstall takes both.
        let mut both = configured.clone();
        both["mcpServers"]["shelbymcp"] = json!({ "command": "npx" });
        fs::write(&path, serde_json::to_vec(&both).unwrap()).unwrap();
        assert!(matches!(
            uninstall(Client::Cursor, &home.paths(), &runner).unwrap(),
            Change::Changed { .. }
        ));
        assert_eq!(read_json(&path)["mcpServers"], json!({}));
    }

    #[test]
    fn a_command_the_user_wrote_under_the_old_key_moves_to_the_new_one() {
        let home = TempHome::new();
        let path = cursor_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let own =
            json!({ "command": "/opt/memory/shelby-mcp", "args": ["--db", "/data/memory.db"] });
        fs::write(
            &path,
            serde_json::to_vec(&json!({ "mcpServers": { "shelbymcp": own.clone() } })).unwrap(),
        )
        .unwrap();
        setup(Client::Cursor, &home.paths(), &FakeRunner::default()).unwrap();
        let configured = read_json(&path);
        assert_eq!(configured["mcpServers"]["advisaur-mcp"], own);
        assert!(configured["mcpServers"].get("shelbymcp").is_none());
    }

    #[test]
    fn client_clis_add_the_new_entry_and_remove_the_old_one() {
        let home = TempHome::new();
        let path = home.0.join(".claude.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({ "mcpServers": { "shelbymcp": { "command": "npx", "args": ["shelbymcp"] } } })).unwrap(),
        )
        .unwrap();
        let runner = FakeRunner::succeeds(&["claude"]);
        assert!(matches!(
            setup(Client::ClaudeCode, &home.paths(), &runner).unwrap(),
            Change::Changed { .. }
        ));
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert!(
            calls[0].1.contains(&"add".to_string())
                && calls[0].1.contains(&"advisaur-mcp".to_string())
        );
        assert_eq!(calls[1].1, vec!["mcp", "remove", "shelbymcp"]);
    }

    #[test]
    fn codex_counts_either_table() {
        let home = TempHome::new();
        let path = home.0.join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[mcp_servers.shelbymcp]\ncommand = \"npx\"\n").unwrap();
        assert_eq!(
            codex_keys(&path).unwrap(),
            Keys {
                current: false,
                legacy: true
            }
        );
        fs::write(&path, "[mcp_servers.\"advisaur-mcp\"]\ncommand = \"npx\"\n").unwrap();
        assert_eq!(
            codex_keys(&path).unwrap(),
            Keys {
                current: true,
                legacy: false
            }
        );
    }

    #[test]
    fn only_what_the_old_setup_wrote_for_a_plain_install_is_stock() {
        let stock = [
            json!({ "command": "npx", "args": ["shelbymcp"] }),
            json!({ "command": "npx", "args": ["-y", "shelbymcp"] }),
            json!({ "type": "stdio", "command": "npx", "args": ["shelbymcp"], "env": {} }),
            json!({ "command": "cmd", "args": ["/c", "npx", "-y", "shelbymcp"] }),
            json!({ "command": "/bin/bash", "args": ["-l", "-c", "npx shelbymcp"] }),
        ];
        for entry in &stock {
            assert!(crate::config::is_old_setup_entry(entry), "{entry}");
        }
        let own = [
            json!({ "command": "/opt/memory/shelby-mcp", "args": ["--db", "/data/memory.db"] }),
            json!({ "command": "npx", "args": ["-y", "shelbymcp"], "env": { "SHELBY_DB_PATH": "/data/memory.db" } }),
            json!({ "command": "npx", "args": ["-y", "shelbymcp", "--db", "/data/memory.db"] }),
            json!({ "command": "/Users/u/.nvm/versions/node/v22/bin/node",
                    "args": ["/Users/u/.nvm/versions/node/v22/bin/shelbymcp"] }),
            json!({ "type": "http", "url": "http://localhost:3100/mcp" }),
            json!({}),
        ];
        for entry in &own {
            assert!(!crate::config::is_old_setup_entry(entry), "{entry}");
        }
    }

    #[test]
    fn a_rewritten_entry_takes_the_new_variable_names_and_keeps_npx_options_before_a_separator() {
        use crate::config::renamed_entry;
        assert_eq!(
            renamed_entry(&json!({ "command": "npx", "args": ["--", "shelbymcp"] })).unwrap()["args"],
            json!(["-y", "--", "advisaur-mcp"])
        );
        let renamed = renamed_entry(&json!({
            "command": "npx",
            "args": ["shelbymcp"],
            "env": { "SHELBY_DB_PATH": "/data/team.db", "SHELBY_API_KEY": "k", "OTHER": "1" },
        }))
        .unwrap();
        assert_eq!(
            renamed["env"],
            json!({ "ADVISAUR_DB_PATH": "/data/team.db", "ADVISAUR_API_KEY": "k", "OTHER": "1" })
        );
        // A blank placeholder under the new name doesn't keep the old value out.
        let blank = renamed_entry(&json!({
            "command": "npx",
            "args": ["shelbymcp"],
            "env": { "ADVISAUR_DB_PATH": "", "SHELBY_DB_PATH": "/data/team.db" },
        }))
        .unwrap();
        assert_eq!(blank["env"], json!({ "ADVISAUR_DB_PATH": "/data/team.db" }));
    }

    #[test]
    fn an_entry_that_ran_the_old_package_through_npx_runs_the_new_one_with_its_settings() {
        use crate::config::renamed_entry;
        let nvm_npx = json!({
            "command": "/Users/u/.nvm/versions/node/v22/bin/node",
            "args": ["/Users/u/.nvm/versions/node/v22/bin/npx", "shelbymcp"],
            "env": { "PATH": "/Users/u/.nvm/versions/node/v22/bin:/usr/bin" },
        });
        assert_eq!(
            renamed_entry(&nvm_npx).unwrap(),
            json!({
                "command": "/Users/u/.nvm/versions/node/v22/bin/node",
                "args": ["/Users/u/.nvm/versions/node/v22/bin/npx", "-y", "advisaur-mcp"],
                "env": { "PATH": "/Users/u/.nvm/versions/node/v22/bin:/usr/bin" },
            })
        );
        assert_eq!(
            renamed_entry(&json!({ "command": "npx", "args": ["shelbymcp", "--db", "/data/m.db"], "env": { "A": "1" } })).unwrap(),
            json!({ "command": "npx", "args": ["-y", "advisaur-mcp", "--db", "/data/m.db"], "env": { "A": "1" } })
        );
        assert_eq!(
            renamed_entry(
                &json!({ "command": "/bin/bash", "args": ["-l", "-c", "npx shelbymcp"] })
            )
            .unwrap(),
            json!({ "command": "/bin/bash", "args": ["-l", "-c", "npx -y advisaur-mcp"] })
        );
        assert_eq!(
            renamed_entry(&json!({ "command": "cmd", "args": ["/c", "npx", "-y", "shelbymcp"] }))
                .unwrap(),
            json!({ "command": "cmd", "args": ["/c", "npx", "-y", "advisaur-mcp"] })
        );
        // The old global binary has no renamed equivalent: it stays the user's.
        assert_eq!(
            renamed_entry(
                &json!({ "command": "/Users/u/.nvm/versions/node/v22/bin/node",
                                   "args": ["/Users/u/.nvm/versions/node/v22/bin/shelbymcp"] })
            ),
            None
        );
        assert_eq!(
            renamed_entry(&json!({ "command": "/opt/shelby-mcp" })),
            None
        );
    }

    #[test]
    fn both_keys_with_a_command_the_user_wrote_under_the_old_one_is_left_for_them() {
        let home = TempHome::new();
        let path = cursor_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let before = serde_json::to_vec(&json!({ "mcpServers": {
            "advisaur-mcp": portable_entry(),
            "shelbymcp": { "command": "/opt/memory/shelby-mcp" },
        } }))
        .unwrap();
        fs::write(&path, &before).unwrap();
        assert!(matches!(
            setup(Client::Cursor, &home.paths(), &FakeRunner::default()).unwrap(),
            Change::ManualAction { .. }
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_client_cli_never_replaces_a_command_the_user_wrote() {
        let home = TempHome::new();
        let path = home.0.join(".claude.json");
        let own =
            json!({ "command": "/opt/memory/shelby-mcp", "args": ["--db", "/data/memory.db"] });
        fs::write(
            &path,
            serde_json::to_vec(&json!({ "mcpServers": { "shelbymcp": own.clone() } })).unwrap(),
        )
        .unwrap();
        let runner = FakeRunner::succeeds(&["claude"]);
        assert!(matches!(
            setup(Client::ClaudeCode, &home.paths(), &runner).unwrap(),
            Change::ManualAction { .. }
        ));
        assert!(runner.calls.borrow().is_empty());
        assert_eq!(read_json(&path)["mcpServers"]["shelbymcp"], own);
    }

    #[test]
    fn codex_tables_are_read_with_comments_spacing_and_quotes() {
        let home = TempHome::new();
        let path = home.0.join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for header in [
            "[mcp_servers.shelbymcp] # memory",
            "[ mcp_servers.shelbymcp ]",
            "[mcp_servers.'shelbymcp']",
        ] {
            fs::write(
                &path,
                format!("{header}\ncommand = \"npx\"\nargs = [\"shelbymcp\"]\n"),
            )
            .unwrap();
            assert_eq!(
                codex_keys(&path).unwrap(),
                Keys {
                    current: false,
                    legacy: true
                },
                "{header}"
            );
            assert!(
                is_old_setup_entry(&codex_legacy_entry(&path).unwrap().unwrap()),
                "{header}"
            );
        }
        fs::write(&path, "[mcp_servers.shelbymcp]\ncommand = \"npx\"\nargs = [\"shelbymcp\"]\n[mcp_servers.shelbymcp.env]\nSHELBY_DB_PATH = \"/data/memory.db\"\n").unwrap();
        assert!(!is_old_setup_entry(
            &codex_legacy_entry(&path).unwrap().unwrap()
        ));
        let runner = FakeRunner::succeeds(&["codex"]);
        assert!(matches!(
            setup(Client::Codex, &home.paths(), &runner).unwrap(),
            Change::ManualAction { .. }
        ));
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn codex_reads_escaped_keys_and_sub_tables_anywhere() {
        assert_eq!(
            toml_table(r#"[mcp_servers."shelby\u006dcp"]"#).unwrap(),
            "mcp_servers.shelbymcp"
        );
        let home = TempHome::new();
        let path = home.0.join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[mcp_servers.shelbymcp]\ncommand = \"npx\"\nargs = [\"shelbymcp\"]\n\n[mcp_servers.other]\ncommand = \"other\"\n\n[mcp_servers.shelbymcp.env]\nSHELBY_DB_PATH = \"/data/team.db\"\n",
        )
        .unwrap();
        assert!(!is_old_setup_entry(
            &codex_legacy_entry(&path).unwrap().unwrap()
        ));
    }
}

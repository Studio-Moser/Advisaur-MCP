use serde_json::{Map, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    #[error("filesystem operation failed for {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("could not execute {program}: {source}")]
    Command {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("{program} command failed")]
    CommandFailed { program: String },
}

pub type Result<T> = std::result::Result<T, IntegrationError>;

/// The server's key in a client's MCP config: its brand-visible name (ADR-0012 D4). The same as
/// `assistant_memory::brand::SERVER_NAME`; this crate depends on nothing, so a test in
/// `assistant-mcp` checks they agree.
pub const SERVER_KEY: &str = "advisaur-mcp";
/// The server's variables under their old names and their current ones (`assistant-mcp`'s
/// `config` holds the same pairs).
const LEGACY_ENV: &[(&str, &str)] = &[
    ("SHELBY_DB_PATH", "ADVISAUR_DB_PATH"),
    ("SHELBY_TRANSPORT", "ADVISAUR_TRANSPORT"),
    ("SHELBY_API_KEY", "ADVISAUR_API_KEY"),
];

/// The server in prose and messages (`assistant_memory::brand::DISPLAY_NAME`).
pub const DISPLAY_NAME: &str = "Advisaur MCP";
/// The key the server had before the rename. Status counts it, setup replaces it, and uninstall
/// removes it, so no client is left with both.
pub const LEGACY_SERVER_KEY: &str = "shelbymcp";

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
}

fn string_args(entry: &Map<String, Value>) -> Option<Vec<String>> {
    match entry.get("args") {
        None => Some(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect(),
        Some(_) => None,
    }
}

/// An old entry rewritten to launch the renamed package. An entry that runs the old npm package
/// through npx keeps its command, environment and other arguments; only the package changes,
/// with `-y` so npx never asks. That covers npx itself, `cmd /c npx`, nvm's `node …/npx` (which
/// v0.2.2 wrote, with a PATH) and a login shell's `npx shelbymcp` (v0.2.1). None for anything
/// else, such as the old global binary (v0.2.3 and later, under nvm): no rename gives the user
/// an equivalent, so the entry stays theirs.
pub(crate) fn renamed_entry(entry: &Value) -> Option<Value> {
    let mut entry = entry.as_object()?.clone();
    if entry.get("type").is_some_and(|kind| kind != "stdio") {
        return None;
    }
    let command = entry.get("command")?.as_str()?.to_string();
    let mut args = string_args(&entry)?;
    let is_npx = |path: &str| matches!(file_name(path).as_str(), "npx" | "npx.cmd");
    let npx_args = if is_npx(&command) {
        Some(0)
    } else {
        args.iter()
            .position(|arg| is_npx(arg))
            .map(|index| index + 1)
    };
    if let Some(start) = npx_args {
        let package = start
            + args[start..]
                .iter()
                .position(|arg| arg == LEGACY_SERVER_KEY)?;
        args[package] = SERVER_KEY.into();
        if !args[start..package]
            .iter()
            .any(|arg| arg == "-y" || arg == "--yes")
        {
            // Before an option separator, where npx still reads options.
            let at = args[start..package]
                .iter()
                .position(|arg| arg == "--")
                .map_or(package, |separator| start + separator);
            args.insert(at, "-y".into());
        }
    } else {
        let shell = matches!(file_name(&command).as_str(), "bash" | "sh" | "zsh");
        let (script, flags) = args.split_last_mut()?;
        let runs_old_package = matches!(
            script.split_whitespace().collect::<Vec<_>>().as_slice(),
            ["npx", LEGACY_SERVER_KEY] | ["npx", "-y", LEGACY_SERVER_KEY]
        );
        if !(shell && flags.iter().any(|flag| flag == "-c") && runs_old_package) {
            return None;
        }
        *script = format!("npx -y {SERVER_KEY}");
    }
    entry.insert("args".into(), Value::from(args));
    // The old variable names are read for one release only; the entry takes the new ones.
    if let Some(Value::Object(env)) = entry.get_mut("env") {
        for (old, new) in LEGACY_ENV {
            if !env.contains_key(*new)
                && let Some(value) = env.remove(*old)
            {
                env.insert((*new).into(), value);
            }
        }
    }
    Some(Value::Object(entry))
}

/// Whether `entry` is exactly what the old setup wrote for a plain install: `npx [-y] shelbymcp`,
/// through `cmd /c` on Windows or a login shell (v0.2.1), with at most an empty `env` and Cursor's
/// `type: "stdio"`. A client CLI can replace such an entry with its own stock one; anything more
/// (an `env`, extra arguments, nvm's paths) is the user's to change.
pub(crate) fn is_old_setup_entry(entry: &Value) -> bool {
    let Some(object) = entry.as_object() else {
        return false;
    };
    let empty_env = object
        .get("env")
        .is_none_or(|env| env.as_object().is_some_and(Map::is_empty));
    if !empty_env
        || object
            .keys()
            .any(|key| !matches!(key.as_str(), "type" | "command" | "args" | "env"))
    {
        return false;
    }
    if object.get("type").is_some_and(|kind| kind != "stdio") {
        return false;
    }
    let (Some(command), Some(args)) = (
        object.get("command").and_then(Value::as_str),
        string_args(object),
    ) else {
        return false;
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let npx = |args: &[&str]| matches!(args, [LEGACY_SERVER_KEY] | ["-y", LEGACY_SERVER_KEY]);
    match command {
        "npx" => npx(&args),
        "cmd" => matches!(args.as_slice(), ["/c", "npx", rest @ ..] if npx(rest)),
        "/bin/bash" | "bash" => matches!(
            args.as_slice(),
            ["-l", "-c", "npx shelbymcp" | "npx -y shelbymcp"]
        ),
        _ => false,
    }
}

/// The entry under the old key in a JSON config, if there is one.
pub(crate) fn json_legacy_entry(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(match load_document(path)? {
        Document::Object(document) => document
            .get("mcpServers")
            .and_then(|servers| servers.get(LEGACY_SERVER_KEY))
            .cloned(),
        Document::ManualAction(_) => None,
    })
}

/// Which of the server's keys a client's config holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Keys {
    pub current: bool,
    pub legacy: bool,
}

impl Keys {
    pub fn any(self) -> bool {
        self.current || self.legacy
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonStatus {
    Configured,
    NotConfigured,
    ManualAction(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonChange {
    Changed,
    AlreadyConfigured,
    AlreadyAbsent,
    ManualAction(String),
}

enum Document {
    Object(Map<String, Value>),
    ManualAction(String),
}

fn filesystem_error(path: &Path, source: io::Error) -> IntegrationError {
    IntegrationError::Filesystem {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    // SAFETY: both pointers reference live, null-terminated UTF-16 buffers for the call.
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn load_document(path: &Path) -> Result<Document> {
    if !path.exists() {
        return Ok(Document::Object(Map::new()));
    }
    let bytes = fs::read(path).map_err(|error| filesystem_error(path, error))?;
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return Ok(Document::ManualAction(format!(
                "Could not parse {}; no changes were written",
                path.display()
            )));
        }
    };
    match value {
        Value::Object(object) => Ok(Document::Object(object)),
        _ => Ok(Document::ManualAction(format!(
            "{} must contain a JSON object; no changes were written",
            path.display()
        ))),
    }
}

fn servers_mut<'a>(
    document: &'a mut Map<String, Value>,
    path: &Path,
) -> std::result::Result<&'a mut Map<String, Value>, String> {
    if !document.contains_key("mcpServers") {
        document.insert("mcpServers".into(), Value::Object(Map::new()));
    }
    document
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            format!(
                "mcpServers in {} must be a JSON object; no changes were written",
                path.display()
            )
        })
}

fn atomic_write_json(path: &Path, document: Map<String, Value>) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| filesystem_error(parent, error))?;
    let existing_permissions = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(filesystem_error(path, error)),
    };
    let mut bytes = serde_json::to_vec_pretty(&Value::Object(document))?;
    bytes.push(b'\n');

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    let temporary = parent.join(format!(
        ".{name}.assistant-{}-{nonce}.tmp",
        std::process::id()
    ));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| filesystem_error(&temporary, error))?;
        if let Some(permissions) = existing_permissions {
            file.set_permissions(permissions)
                .map_err(|error| filesystem_error(&temporary, error))?;
        }
        file.write_all(&bytes)
            .map_err(|error| filesystem_error(&temporary, error))?;
        file.flush()
            .map_err(|error| filesystem_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| filesystem_error(&temporary, error))?;
        drop(file);
        replace_file(&temporary, path).map_err(|error| filesystem_error(path, error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn json_status(path: &Path) -> Result<JsonStatus> {
    Ok(match json_keys(path)? {
        Ok(keys) if keys.any() => JsonStatus::Configured,
        Ok(_) => JsonStatus::NotConfigured,
        Err(message) => JsonStatus::ManualAction(message),
    })
}

/// The server's keys in a JSON config, or what the user must fix first.
pub(crate) fn json_keys(path: &Path) -> Result<std::result::Result<Keys, String>> {
    let none = Keys {
        current: false,
        legacy: false,
    };
    if !path.exists() {
        return Ok(Ok(none));
    }
    let mut document = match load_document(path)? {
        Document::Object(document) => document,
        Document::ManualAction(message) => return Ok(Err(message)),
    };
    let Some(servers) = document.get_mut("mcpServers") else {
        return Ok(Ok(none));
    };
    let Some(servers) = servers.as_object() else {
        return Ok(Err(format!(
            "mcpServers in {} must be a JSON object; no changes were written",
            path.display()
        )));
    };
    Ok(Ok(Keys {
        current: servers.contains_key(SERVER_KEY),
        legacy: servers.contains_key(LEGACY_SERVER_KEY),
    }))
}

/// Writes the server's entry. An entry under the old key moves to the new one, rewritten to run the
/// renamed package when it ran the old one through npx (`renamed_entry`), else as it is.
pub(crate) fn merge_json(path: &Path, entry: Value) -> Result<JsonChange> {
    let mut document = match load_document(path)? {
        Document::Object(document) => document,
        Document::ManualAction(message) => return Ok(JsonChange::ManualAction(message)),
    };
    let servers = match servers_mut(&mut document, path) {
        Ok(servers) => servers,
        Err(message) => return Ok(JsonChange::ManualAction(message)),
    };
    match (
        servers.contains_key(SERVER_KEY),
        servers.get(LEGACY_SERVER_KEY).cloned(),
    ) {
        (true, None) => return Ok(JsonChange::AlreadyConfigured),
        // Both: the old one goes only when the old setup wrote it; one the user wrote is theirs.
        (true, Some(old)) if !is_old_setup_entry(&old) => {
            return Ok(JsonChange::ManualAction(format!(
                "{} has both {SERVER_KEY} and an entry you set under {LEGACY_SERVER_KEY}; remove the one you don't need",
                path.display()
            )));
        }
        (true, Some(_)) => {
            servers.remove(LEGACY_SERVER_KEY);
        }
        (false, Some(old)) => {
            servers.remove(LEGACY_SERVER_KEY);
            servers.insert(SERVER_KEY.into(), renamed_entry(&old).unwrap_or(old));
        }
        (false, None) => {
            servers.insert(SERVER_KEY.into(), entry);
        }
    }
    atomic_write_json(path, document)?;
    Ok(JsonChange::Changed)
}

pub(crate) fn remove_json(path: &Path) -> Result<JsonChange> {
    if !path.exists() {
        return Ok(JsonChange::AlreadyAbsent);
    }
    let mut document = match load_document(path)? {
        Document::Object(document) => document,
        Document::ManualAction(message) => return Ok(JsonChange::ManualAction(message)),
    };
    let Some(servers) = document.get_mut("mcpServers") else {
        return Ok(JsonChange::AlreadyAbsent);
    };
    let Some(servers) = servers.as_object_mut() else {
        return Ok(JsonChange::ManualAction(format!(
            "mcpServers in {} must be a JSON object; no changes were written",
            path.display()
        )));
    };
    let current = servers.remove(SERVER_KEY).is_some();
    let legacy = servers.remove(LEGACY_SERVER_KEY).is_some();
    if !current && !legacy {
        return Ok(JsonChange::AlreadyAbsent);
    }
    atomic_write_json(path, document)?;
    Ok(JsonChange::Changed)
}

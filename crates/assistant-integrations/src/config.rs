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
    let temporary = parent.join(format!(".{name}.shelby-{}-{nonce}.tmp", std::process::id()));
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
    if !path.exists() {
        return Ok(JsonStatus::NotConfigured);
    }
    let mut document = match load_document(path)? {
        Document::Object(document) => document,
        Document::ManualAction(message) => return Ok(JsonStatus::ManualAction(message)),
    };
    let Some(servers) = document.get_mut("mcpServers") else {
        return Ok(JsonStatus::NotConfigured);
    };
    let Some(servers) = servers.as_object() else {
        return Ok(JsonStatus::ManualAction(format!(
            "mcpServers in {} must be a JSON object; no changes were written",
            path.display()
        )));
    };
    Ok(if servers.contains_key("shelbymcp") {
        JsonStatus::Configured
    } else {
        JsonStatus::NotConfigured
    })
}

pub(crate) fn merge_json(path: &Path, entry: Value) -> Result<JsonChange> {
    let mut document = match load_document(path)? {
        Document::Object(document) => document,
        Document::ManualAction(message) => return Ok(JsonChange::ManualAction(message)),
    };
    let servers = match servers_mut(&mut document, path) {
        Ok(servers) => servers,
        Err(message) => return Ok(JsonChange::ManualAction(message)),
    };
    if servers.contains_key("shelbymcp") {
        return Ok(JsonChange::AlreadyConfigured);
    }
    servers.insert("shelbymcp".into(), entry);
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
    if servers.remove("shelbymcp").is_none() {
        return Ok(JsonChange::AlreadyAbsent);
    }
    atomic_write_json(path, document)?;
    Ok(JsonChange::Changed)
}

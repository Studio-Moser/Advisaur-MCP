use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::longmemeval::{LongMemEvalError, LongMemEvalManifest, verify_cache};

#[derive(Debug, Error)]
pub enum FetchError {
    #[error(transparent)]
    Dataset(#[from] LongMemEvalError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("dataset download failed with exit code {0:?}")]
    DownloadFailed(Option<i32>),
}

pub fn fetch_dataset(
    manifest: &LongMemEvalManifest,
    cache_path: &Path,
) -> Result<PathBuf, FetchError> {
    if cache_path.exists() {
        verify_cache(manifest, cache_path)?;
        return Ok(cache_path.to_path_buf());
    }
    if manifest.dataset.license != "MIT" {
        return Err(LongMemEvalError::UnapprovedLicense(manifest.dataset.license.clone()).into());
    }

    if let Some(parent) = cache_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let file_name = cache_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("longmemeval.json");
    let partial = cache_path.with_file_name(format!("{file_name}.partial-{}", std::process::id()));
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&partial)
        .arg(&manifest.dataset.source)
        .status()?;
    if !status.success() {
        let _ = fs::remove_file(&partial);
        return Err(FetchError::DownloadFailed(status.code()));
    }
    if let Err(error) = verify_cache(manifest, &partial) {
        let _ = fs::remove_file(&partial);
        return Err(error.into());
    }
    fs::rename(&partial, cache_path)?;
    Ok(cache_path.to_path_buf())
}

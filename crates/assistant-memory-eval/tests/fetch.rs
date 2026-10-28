use std::fs;

use assistant_memory_eval::fetch::fetch_dataset;
use assistant_memory_eval::longmemeval::{Dataset, LongMemEvalManifest};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn temporary_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("assistant-memory-eval-{}.json", Uuid::new_v4()))
}

fn manifest(content: &[u8], license: &str) -> LongMemEvalManifest {
    LongMemEvalManifest {
        schema_version: 1,
        suite_version: "test".into(),
        selection_method: "test fixture".into(),
        dataset: Dataset {
            name: "test".into(),
            source: "https://invalid.example/should-not-be-read".into(),
            revision: "a".repeat(40),
            sha256: format!("{:x}", Sha256::digest(content)),
            license: license.into(),
            license_url: "https://example.test/license".into(),
        },
        cases: vec![],
    }
}

#[test]
fn fetch_reuses_a_verified_cache_without_network_access() {
    let path = temporary_path();
    fs::write(&path, b"verified").unwrap();

    let result = fetch_dataset(&manifest(b"verified", "MIT"), &path).unwrap();

    assert_eq!(result, path);
    assert_eq!(fs::read(&path).unwrap(), b"verified");
    fs::remove_file(path).unwrap();
}

#[test]
fn fetch_rejects_an_unapproved_license_before_using_the_cache() {
    let path = temporary_path();
    fs::write(&path, b"verified").unwrap();

    let error = fetch_dataset(&manifest(b"verified", "unknown"), &path).unwrap_err();

    assert!(error.to_string().contains("license"));
    fs::remove_file(path).unwrap();
}

#[test]
fn fetch_downloads_to_a_partial_file_then_verifies_the_result() {
    let directory = std::env::temp_dir().join(format!("assistant-fetch-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    let source = directory.join("source.json");
    let target = directory.join("cache.json");
    fs::write(&source, b"small pinned dataset").unwrap();
    let mut manifest = manifest(b"small pinned dataset", "MIT");
    manifest.dataset.source = format!("file://{}", source.display());

    let result = fetch_dataset(&manifest, &target).unwrap();

    assert_eq!(result, target);
    assert_eq!(fs::read(&target).unwrap(), b"small pinned dataset");
    assert_eq!(
        fs::read_dir(&directory).unwrap().count(),
        2,
        "partial download must not remain"
    );
    fs::remove_dir_all(directory).unwrap();
}

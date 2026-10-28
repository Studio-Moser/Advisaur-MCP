//! Cutover compatibility: the mandatory pre-0.4 database fixture must open,
//! query, and accept new Rust-written rows without migration or conversion.
use std::path::{Path, PathBuf};

use assistant_memory::fts::{SearchOptions, search_thoughts};
use assistant_memory::thoughts::{ThoughtInput, get_thought, insert_thought};
use assistant_memory::{Memory, migrations::CURRENT_SCHEMA_VERSION};

const SEED_ID: &str = "018f4c66-7c4e-7a4d-8e7a-6a74af7fd001";

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/TypeScript-v18.sqlite")
}

fn remove_sqlite_files(path: &Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        let _ = std::fs::remove_file(candidate);
    }
}

#[test]
fn reads_and_extends_a_typescript_created_database() {
    let source = fixture_path();
    assert!(
        source.is_file(),
        "missing TypeScript-v18 compatibility fixture: {}",
        source.display()
    );
    let path = std::env::temp_dir().join(format!(
        "assistant-typescript-v18-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    std::fs::copy(&source, &path).expect("copy compatibility fixture");

    let m = Memory::open(&path).unwrap();
    assert_eq!(
        m.schema_version().unwrap(),
        CURRENT_SCHEMA_VERSION,
        "no migration needed on a TS-created db"
    );
    let seed = get_thought(&m.conn, SEED_ID)
        .unwrap()
        .expect("fixed TS seed row visible");
    assert_eq!(
        seed.topics,
        vec!["knowledge-graph"],
        "TS canonicalized topics parse as an array"
    );
    assert_eq!(
        seed.project_identifier.as_deref(),
        Some("shelby"),
        "alias resolves through the TS-written registry"
    );
    let found = search_thoughts(
        &m.conn,
        &SearchOptions {
            query: "typescript".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        found.total_count, 1,
        "FTS index written by TS triggers is queryable"
    );
    let id = insert_thought(
        &m.conn,
        &ThoughtInput {
            content: "written by the rust engine".into(),
            summary: Some("rust row".into()),
            project_identifier: Some("shelby".into()),
            topics: Some(vec!["Cross Engine".into()]),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(m.schema_version().unwrap(), 18);
    let edge_id = assistant_memory::edges::link_thoughts(
        &m.conn,
        &assistant_memory::edges::EdgeInput {
            source_id: SEED_ID.into(),
            target_id: id.clone(),
            edge_type: "refines".into(),
            valid_from: Some("2026-09-06 12:00:00.500".into()),
            valid_until: Some("2026-09-06T05:00:00.500000001-07:00".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let edge_before = assistant_memory::edges::get_edge(&m.conn, &edge_id)
        .unwrap()
        .unwrap();
    assert!(
        assistant_memory::edges::is_active_at(&edge_before, "2026-09-06T12:00:00.500Z").unwrap()
    );
    drop(m);
    let reopened = Memory::open(&path).unwrap();
    assert_eq!(
        get_thought(&reopened.conn, &id)
            .unwrap()
            .expect("Rust-written row survives reopen")
            .summary
            .as_deref(),
        Some("rust row")
    );
    assert_eq!(reopened.schema_version().unwrap(), 18);
    let edge_after = assistant_memory::edges::get_edge(&reopened.conn, &edge_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        edge_after, edge_before,
        "Opening cannot normalize legacy bounds"
    );
    assert_eq!(
        assistant_memory::edges::get_connections_at(
            &reopened.conn,
            &id,
            None,
            "2026-09-06T12:00:00.500Z"
        )
        .unwrap()
        .len(),
        1
    );
    assert!(
        assistant_memory::edges::get_connections_at(
            &reopened.conn,
            &id,
            None,
            "2026-09-06T12:00:00.500000001Z"
        )
        .unwrap()
        .is_empty()
    );
    drop(reopened);
    remove_sqlite_files(&path);
}

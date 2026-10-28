use super::*;
use crate::{
    Memory,
    projects::{ProjectSeed, upsert_project},
};
use rusqlite::params;
fn fixture() -> (Memory, String) {
    let m = Memory::open_in_memory().unwrap();
    upsert_project(
        &m.conn,
        &ProjectSeed {
            slug: "original".into(),
            display_name: "Original".into(),
            member_repos: vec!["https://github.com/a/one".into()],
            member_paths: vec!["/preserve/path".into()],
            provisional: true,
        },
    )
    .unwrap();
    let id = crate::identity::derive_existing_project_id("original").unwrap();
    m.conn.execute("UPDATE projects SET member_repos=?1,member_paths=?2,created_at='opaque creation',updated_at='opaque update' WHERE project_id=?3",params![r#"[ "https:\/\/github.com\/a\/one" ]"#,r#"["/preserve/path", "\\u0000 literal"]"#,id]).unwrap();
    (m, id)
}
#[test]
fn strict_snapshot_noop_and_update_preserve_nonmembership_bytes() {
    let (m, id) = fixture();
    let before = inspect_project_membership(&m.conn, &id).unwrap().unwrap();
    assert_eq!(before.project_id(), id);
    assert_eq!(before.display_name(), "Original");
    assert_eq!(before.repositories(), ["https://github.com/a/one"]);
    let changes = m.conn.total_changes();
    let ProjectMembershipUpdate::Unchanged(s) =
        replace_project_repositories(&m.conn, &id, before.fingerprint(), before.repositories())
            .unwrap()
    else {
        panic!("expected no-op")
    };
    assert_eq!(s.fingerprint(), before.fingerprint());
    assert_eq!(m.conn.total_changes(), changes);
    let unchanged: (String,String,String,String,i64,String,String,String)=m.conn.query_row("SELECT slug,display_name,member_paths,created_at,provisional,project_id,current_slug,identity_state FROM projects",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).unwrap();
    let desired = vec!["https://github.com/a/two".into()];
    let ProjectMembershipUpdate::Applied(after) =
        replace_project_repositories(&m.conn, &id, before.fingerprint(), &desired).unwrap()
    else {
        panic!("expected applied")
    };
    assert_eq!(after.repositories(), desired);
    assert_ne!(after.fingerprint(), before.fingerprint());
    let current=m.conn.query_row("SELECT slug,display_name,member_paths,created_at,provisional,project_id,current_slug,identity_state FROM projects",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).unwrap();
    assert_eq!(unchanged, current);
    assert!(m.conn.is_autocommit());
}
#[test]
fn stale_full_snapshot_conflicts_even_for_an_apparent_noop_and_missing_never_creates() {
    let (m, id) = fixture();
    let before = inspect_project_membership(&m.conn, &id).unwrap().unwrap();
    m.conn
        .execute("UPDATE projects SET display_name='Changed'", [])
        .unwrap();
    assert!(matches!(
        replace_project_repositories(&m.conn, &id, before.fingerprint(), before.repositories()),
        Err(ProjectMembershipError::Conflict)
    ));
    let missing = uuid::Uuid::new_v4().to_string();
    assert!(
        inspect_project_membership(&m.conn, &missing)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        replace_project_repositories(&m.conn, &missing, before.fingerprint(), &[]),
        Err(ProjectMembershipError::NotFound)
    ));
    assert_eq!(
        m.conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

fn current(m: &Memory, id: &str) -> ProjectMembershipSnapshot {
    inspect_project_membership(&m.conn, id).unwrap().unwrap()
}
fn raw_rows(conn: &Connection) -> String {
    let tables = [("projects", "slug"), ("project_slug_aliases", "slug")];
    let mut out = Vec::new();
    for (table, order) in tables {
        let mut q = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
            .unwrap();
        let n = q.column_count();
        let values = q
            .query_map([], |r| {
                (0..n)
                    .map(|i| r.get::<_, rusqlite::types::Value>(i))
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        out.push(format!("{values:?}"));
    }
    out.join("\n")
}
#[test]
fn strict_schema_guards_reject_added_or_altered_objects_without_writes() {
    for sql in [
        "CREATE TRIGGER extra_project AFTER UPDATE ON projects BEGIN SELECT 1; END",
        "CREATE TRIGGER extra_alias AFTER UPDATE ON project_slug_aliases BEGIN SELECT 1; END",
        "ALTER TABLE projects ADD COLUMN extra TEXT",
        "ALTER TABLE project_slug_aliases ADD COLUMN extra TEXT",
        "CREATE INDEX extra_index ON projects(display_name)",
        "DROP INDEX idx_projects_provisional",
        "DROP INDEX one_current_slug_per_project;CREATE UNIQUE INDEX one_current_slug_per_project ON project_slug_aliases(project_id) WHERE status='retired'",
        "DROP INDEX idx_projects_provisional;CREATE INDEX idx_projects_provisional ON projects(provisional DESC)",
        "DROP INDEX idx_projects_provisional;CREATE INDEX idx_projects_provisional ON projects(display_name)",
        "CREATE TEMP TABLE projects(n TEXT)",
        "CREATE TEMP VIEW project_slug_aliases AS SELECT 'x' AS slug",
        "PRAGMA writable_schema=ON;UPDATE sqlite_schema SET sql=replace(sql, 'TEXT NOT NULL DEFAULT', 'TEXT DEFAULT') WHERE name='projects';PRAGMA writable_schema=RESET",
    ] {
        let (m, id) = fixture();
        let before = current(&m, &id);
        m.conn.execute_batch(sql).unwrap();
        let changes = m.conn.total_changes();
        assert!(inspect_project_membership(&m.conn, &id).is_err(), "{sql}");
        assert!(
            replace_project_repositories(&m.conn, &id, before.fingerprint(), &[]).is_err(),
            "{sql}"
        );
        assert_eq!(m.conn.total_changes(), changes);
        assert!(m.conn.is_autocommit());
    }
}
#[test]
fn strict_arrays_and_source_types_never_fall_back_to_empty() {
    let invalid = [
        "null".to_string(),
        "{}".into(),
        "[1]".into(),
        "[null]".into(),
        "[true]".into(),
        "[\"x\",]".into(),
        "[] false".into(),
        "[[]]".into(),
        serde_json::to_string(&vec!["x"; 101]).unwrap(),
        serde_json::to_string(&["x".repeat(2049)]).unwrap(),
        serde_json::to_string(&vec!["x".repeat(2048); 17]).unwrap(),
        " ".repeat(262145),
    ];
    for column in ["member_repos", "member_paths"] {
        for value in &invalid {
            let (m, id) = fixture();
            m.conn
                .execute(&format!("UPDATE projects SET {column}=?1"), [value])
                .unwrap();
            assert!(
                inspect_project_membership(&m.conn, &id).is_err(),
                "{column}: {} bytes",
                value.len()
            );
            assert!(m.conn.is_autocommit());
        }
    }
    for sql in [
        "UPDATE projects SET member_repos=X'5b5d'",
        "UPDATE projects SET member_paths=X'5b5d'",
        "UPDATE projects SET member_repos=CAST(X'ff' AS TEXT)",
        "UPDATE projects SET provisional=2",
        "UPDATE projects SET provisional=0.5",
        "UPDATE projects SET provisional=X'30'",
        "UPDATE projects SET display_name=zeroblob(1000000)",
        "UPDATE projects SET created_at=''",
        "UPDATE projects SET updated_at=zeroblob(1000000)",
        "UPDATE projects SET current_slug='INVALID'",
        "UPDATE projects SET identity_state='other'",
        "UPDATE project_slug_aliases SET claimed_at=''",
        "UPDATE project_slug_aliases SET retired_at=''",
        "UPDATE project_slug_aliases SET retired_at=zeroblob(1000000)",
    ] {
        let (m, id) = fixture();
        m.conn
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        m.conn.execute_batch(sql).unwrap();
        assert!(inspect_project_membership(&m.conn, &id).is_err(), "{sql}");
        assert!(m.conn.is_autocommit());
    }
}
#[test]
fn native_request_bounds_and_caller_transaction_are_rejected_without_cleanup() {
    let (m, id) = fixture();
    let before = current(&m, &id);
    for invalid in ["bad".to_string(), id.to_uppercase(), "a".repeat(1000000)] {
        assert!(matches!(
            inspect_project_membership(&m.conn, &invalid),
            Err(ProjectMembershipError::InvalidInput)
        ));
    }
    for fp in ["A".repeat(64), "0".repeat(63), "g".repeat(64)] {
        assert!(matches!(
            replace_project_repositories(&m.conn, &id, &fp, &[]),
            Err(ProjectMembershipError::InvalidInput)
        ));
    }
    for repos in [
        vec!["x".into(); 101],
        vec!["x".repeat(2049)],
        vec!["x".repeat(2048); 17],
    ] {
        assert!(matches!(
            replace_project_repositories(&m.conn, &id, before.fingerprint(), &repos),
            Err(ProjectMembershipError::InvalidInput)
        ));
    }
    m.conn
        .execute_batch("BEGIN IMMEDIATE;UPDATE projects SET display_name='Uncommitted caller'")
        .unwrap();
    assert!(inspect_project_membership(&m.conn, &id).is_err());
    assert!(replace_project_repositories(&m.conn, &id, before.fingerprint(), &[]).is_err());
    assert!(!m.conn.is_autocommit());
    assert_eq!(
        m.conn
            .query_row("SELECT display_name FROM projects", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "Uncommitted caller"
    );
    m.conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(current(&m, &id).fingerprint(), before.fingerprint());
}
#[test]
fn raw_json_whitespace_escapes_order_duplicates_and_nul_are_owned_exactly() {
    let (m, id) = fixture();
    let before = current(&m, &id);
    m.conn
        .execute(
            "UPDATE projects SET member_repos='[\"https://github.com/a/one\"]'",
            [],
        )
        .unwrap();
    let now = current(&m, &id);
    assert_eq!(now.repositories(), before.repositories());
    assert_ne!(now.fingerprint(), before.fingerprint());
    assert!(matches!(
        replace_project_repositories(&m.conn, &id, before.fingerprint(), now.repositories()),
        Err(ProjectMembershipError::Conflict)
    ));
    let desired = vec!["b".into(), "a\0é".into(), "b".into()];
    let ProjectMembershipUpdate::Applied(after) =
        replace_project_repositories(&m.conn, &id, now.fingerprint(), &desired).unwrap()
    else {
        panic!()
    };
    assert_eq!(after.repositories(), desired);
    let raw = raw_rows(&m.conn);
    replace_project_repositories(&m.conn, &id, after.fingerprint(), &desired).unwrap();
    assert_eq!(raw_rows(&m.conn), raw);
}
#[test]
fn every_nonmembership_field_and_alias_change_fences_old_snapshot() {
    for sql in [
        "UPDATE projects SET display_name=''",
        "UPDATE projects SET member_paths=' [ ] '",
        "UPDATE projects SET provisional=0",
        "UPDATE projects SET created_at='changed'",
        "UPDATE projects SET updated_at='changed'",
        "UPDATE projects SET current_slug='renamed'",
        "UPDATE projects SET identity_state='active'",
        "UPDATE project_slug_aliases SET claimed_at='changed'",
        "UPDATE project_slug_aliases SET retired_at='changed'",
        "UPDATE project_slug_aliases SET status='retired'",
        "UPDATE project_slug_aliases SET slug='renamed'",
        "DELETE FROM project_slug_aliases",
    ] {
        let (m, id) = fixture();
        let before = current(&m, &id);
        m.conn.execute_batch(sql).unwrap();
        let after = current(&m, &id);
        assert_ne!(before.fingerprint(), after.fingerprint(), "{sql}");
        assert!(
            matches!(
                replace_project_repositories(
                    &m.conn,
                    &id,
                    before.fingerprint(),
                    before.repositories()
                ),
                Err(ProjectMembershipError::Conflict)
            ),
            "{sql}"
        );
    }
}
#[test]
fn aliases_preserve_renamed_v4_legacy_missing_and_reject_other_owner() {
    let m = Memory::open_in_memory().unwrap();
    let p = crate::projects::create_local_only_project(
        &m.conn,
        &ProjectSeed {
            slug: "renamed".into(),
            display_name: String::new(),
            ..Default::default()
        },
    )
    .unwrap();
    m.conn
        .execute(
            "UPDATE project_slug_aliases SET status='retired',retired_at='old spelling'",
            [],
        )
        .unwrap();
    m.conn
        .execute("UPDATE projects SET current_slug='new-name'", [])
        .unwrap();
    m.conn
        .execute(
            "INSERT INTO project_slug_aliases VALUES('new-name',?1,'current','new spelling',NULL)",
            [&p.project_id],
        )
        .unwrap();
    let before = current(&m, &p.project_id);
    let aliases = before
        .aliases
        .iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>();
    let ProjectMembershipUpdate::Applied(after) = replace_project_repositories(
        &m.conn,
        &p.project_id,
        before.fingerprint(),
        &["repo".into()],
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        after
            .aliases
            .iter()
            .map(|a| format!("{a:?}"))
            .collect::<Vec<_>>(),
        aliases
    );
    m.conn
        .execute("DELETE FROM project_slug_aliases", [])
        .unwrap();
    assert!(
        inspect_project_membership(&m.conn, &p.project_id)
            .unwrap()
            .is_some()
    );
    m.conn
        .execute(
            "INSERT INTO project_slug_aliases VALUES('new-name',?1,'tentative','claimed',NULL)",
            [uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
    assert!(inspect_project_membership(&m.conn, &p.project_id).is_err());
    let (legacy, id) = fixture();
    legacy
        .conn
        .execute(
            "UPDATE projects SET project_id=?1",
            [uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
    assert!(
        inspect_project_membership(&legacy.conn, &id)
            .unwrap()
            .is_none()
    );
    let wrong: String = legacy
        .conn
        .query_row("SELECT project_id FROM projects", [], |r| r.get(0))
        .unwrap();
    assert!(inspect_project_membership(&legacy.conn, &wrong).is_err());
}
#[test]
fn maximum_arrays_and_aliases_are_complete_then_overflow_refuses() {
    let (m, id) = fixture();
    let before = current(&m, &id);
    let values = vec!["\0".repeat(2048); 16];
    let ProjectMembershipUpdate::Applied(after) =
        replace_project_repositories(&m.conn, &id, before.fingerprint(), &values).unwrap()
    else {
        panic!()
    };
    assert_eq!(after.repositories(), values);
    m.conn
        .execute("DELETE FROM project_slug_aliases", [])
        .unwrap();
    for n in (0..100).rev() {
        m.conn
            .execute(
                "INSERT INTO project_slug_aliases VALUES(?1,?2,'retired',?3,NULL)",
                params![format!("alias-{n:03}"), id, "x".repeat(128)],
            )
            .unwrap();
    }
    assert_eq!(current(&m, &id).aliases.len(), 100);
    m.conn
        .execute(
            "INSERT INTO project_slug_aliases VALUES('overflow',?1,'retired','x',NULL)",
            [&id],
        )
        .unwrap();
    assert!(inspect_project_membership(&m.conn, &id).is_err());
}

#[test]
fn stored_sql_null_membership_is_unavailable_even_when_schema_declares_notnull() {
    let (m, id) = fixture();
    let original: String = m
        .conn
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='projects'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let weakened = original.replace("member_repos    TEXT NOT NULL", "member_repos    TEXT");
    assert_ne!(weakened, original);
    m.conn.execute_batch("PRAGMA writable_schema=ON").unwrap();
    m.conn
        .execute(
            "UPDATE sqlite_schema SET sql=?1 WHERE name='projects'",
            [&weakened],
        )
        .unwrap();
    m.conn.execute_batch("PRAGMA writable_schema=RESET;UPDATE projects SET member_repos=NULL;PRAGMA writable_schema=ON").unwrap();
    m.conn
        .execute(
            "UPDATE sqlite_schema SET sql=?1 WHERE name='projects'",
            [&original],
        )
        .unwrap();
    m.conn
        .execute_batch("PRAGMA writable_schema=RESET")
        .unwrap();
    schema::check(&m.conn).unwrap();
    assert!(inspect_project_membership(&m.conn, &id).is_err());
    assert!(m.conn.is_autocommit());
}
#[test]
fn literal_fingerprint_vectors_cover_sql_types_lengths_all_columns_and_null_alias() {
    // Independent Python hashlib/uuid/struct derivation retained in the review's Vector Provenance.py.
    let (m, id) = fixture();
    m.conn.execute("UPDATE projects SET display_name=?1,member_repos='[ \"a\", \"a\" ]',member_paths='[]',created_at='c',updated_at='u'",["NUL\0Name"]).unwrap();
    m.conn
        .execute(
            "UPDATE project_slug_aliases SET claimed_at='t',retired_at=NULL",
            [],
        )
        .unwrap();
    assert_eq!(
        current(&m, &id).fingerprint(),
        "30f86170b4dee86bed9141cf2321a74b6e28bf69462383a25af89923ac9ed066"
    );
    m.conn
        .execute("UPDATE project_slug_aliases SET retired_at='r'", [])
        .unwrap();
    assert_eq!(
        current(&m, &id).fingerprint(),
        "3ae3629289a8046b8951cba46636bb0974c384e3b1ac6105bd8f0ae32bd85014"
    );
}
use rusqlite::hooks::{AuthAction, AuthContext, Authorization, TransactionOperation};
#[test]
fn read_update_and_commit_failures_preserve_original_and_clean_owned_transaction() {
    for mode in ["read", "update", "commit"] {
        let (m, id) = fixture();
        let before = current(&m, &id);
        let raw = raw_rows(&m.conn);
        match mode {
            "commit" => m.conn.commit_hook(Some(|| true)).unwrap(),
            _ => m
                .conn
                .authorizer(Some(move |ctx: AuthContext<'_>| match ctx.action {
                    AuthAction::Read {
                        table_name: "projects",
                        ..
                    } if mode == "read" => Authorization::Deny,
                    AuthAction::Update {
                        table_name: "projects",
                        ..
                    } if mode == "update" => Authorization::Deny,
                    _ => Authorization::Allow,
                }))
                .unwrap(),
        }
        assert!(
            matches!(
                replace_project_repositories(
                    &m.conn,
                    &id,
                    before.fingerprint(),
                    &["changed".into()]
                ),
                Err(ProjectMembershipError::Unavailable)
            ),
            "{mode}"
        );
        assert!(m.conn.is_autocommit());
        m.conn
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
            .unwrap();
        m.conn.commit_hook(None::<fn() -> bool>).unwrap();
        assert_eq!(raw_rows(&m.conn), raw);
        assert_eq!(current(&m, &id).fingerprint(), before.fingerprint());
        assert!(matches!(
            replace_project_repositories(&m.conn, &id, before.fingerprint(), &["changed".into()]),
            Ok(ProjectMembershipUpdate::Applied(_))
        ));
    }
}
#[test]
fn failed_cleanup_returns_unusable_and_never_rolls_back_a_later_caller_transaction() {
    for mode in ["read", "write", "noop"] {
        let (m, id) = fixture();
        let before = current(&m, &id);
        m.conn
            .authorizer(Some(move |ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Transaction {
                    operation: TransactionOperation::Rollback,
                } => Authorization::Deny,
                AuthAction::Update {
                    table_name: "projects",
                    ..
                } if mode == "write" => Authorization::Deny,
                _ => Authorization::Allow,
            }))
            .unwrap();
        let result = match mode {
            "read" => inspect_project_membership(&m.conn, &id).map(|_| ()),
            "noop" => replace_project_repositories(
                &m.conn,
                &id,
                before.fingerprint(),
                before.repositories(),
            )
            .map(|_| ()),
            _ => replace_project_repositories(
                &m.conn,
                &id,
                before.fingerprint(),
                &["changed".into()],
            )
            .map(|_| ()),
        };
        assert!(
            matches!(result, Err(ProjectMembershipError::UnusableConnection)),
            "{mode}"
        );
        assert!(!m.conn.is_autocommit());
        assert!(matches!(
            inspect_project_membership(&m.conn, &id),
            Err(ProjectMembershipError::InvalidInput)
        ));
        assert!(!m.conn.is_autocommit());
        m.conn
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
            .unwrap();
        m.conn.execute_batch("ROLLBACK").unwrap();
        assert_eq!(current(&m, &id).fingerprint(), before.fingerprint());
    }
}
fn disk_fixture() -> (Memory, String, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("shelby-membership-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let (m, id) = fixture();
    let file = dir.join("memory.db");
    m.conn
        .execute("VACUUM INTO ?1", [file.to_str().unwrap()])
        .unwrap();
    drop(m);
    (Memory::open(&file).unwrap(), id, dir)
}
#[test]
fn real_postcommit_error_returns_applied_only_from_exact_same_guard_evidence() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CALLED: AtomicBool = AtomicBool::new(false);
    let (m, id, dir) = disk_fixture();
    let before = current(&m, &id);
    let changes = m.conn.total_changes();
    m.conn.wal_hook(Some(|_, _| {
        CALLED.store(true, Ordering::SeqCst);
        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
            None,
        ))
    }));
    let ProjectMembershipUpdate::Applied(after) =
        replace_project_repositories(&m.conn, &id, before.fingerprint(), &["confirmed".into()])
            .unwrap()
    else {
        panic!()
    };
    assert!(CALLED.swap(false, Ordering::SeqCst));
    assert_eq!(after.repositories(), ["confirmed"]);
    assert_eq!(m.conn.total_changes(), changes + 1);
    assert!(matches!(
        replace_project_repositories(&m.conn, &id, after.fingerprint(), after.repositories()),
        Ok(ProjectMembershipUpdate::Unchanged(_))
    ));
    assert!(!CALLED.load(Ordering::SeqCst));
    m.conn.wal_hook(None);
    drop(m);
    let reopened = Memory::open(dir.join("memory.db")).unwrap();
    assert_eq!(current(&reopened, &id).fingerprint(), after.fingerprint());
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn unreadable_postcommit_evidence_is_unknown_without_repeating_update() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static COMMITTED: AtomicBool = AtomicBool::new(false);
    let (m, id, dir) = disk_fixture();
    let before = current(&m, &id);
    let changes = m.conn.total_changes();
    m.conn
        .authorizer(Some(|ctx: AuthContext<'_>| {
            if COMMITTED.load(Ordering::SeqCst)
                && matches!(
                    ctx.action,
                    AuthAction::Read {
                        table_name: "projects",
                        ..
                    }
                )
            {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
    m.conn.wal_hook(Some(|_, _| {
        COMMITTED.store(true, Ordering::SeqCst);
        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
            None,
        ))
    }));
    assert!(matches!(
        replace_project_repositories(&m.conn, &id, before.fingerprint(), &["committed".into()]),
        Err(ProjectMembershipError::OutcomeUnknown)
    ));
    assert_eq!(m.conn.total_changes(), changes + 1);
    assert!(m.conn.is_autocommit());
    m.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    m.conn.wal_hook(None);
    assert_eq!(current(&m, &id).repositories(), ["committed"]);
    drop(m);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn changed_or_missing_postcommit_evidence_is_unknown_without_replay() {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    static FILE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);
    static MODE: AtomicUsize = AtomicUsize::new(0);
    static MUTATED: AtomicBool = AtomicBool::new(false);
    for mode in 0..2 {
        let (m, id, dir) = disk_fixture();
        let before = current(&m, &id);
        let changes = m.conn.total_changes();
        *FILE.lock().unwrap() = Some(dir.join("memory.db"));
        MODE.store(mode, Ordering::SeqCst);
        MUTATED.store(false, Ordering::SeqCst);
        m.conn.wal_hook(Some(|_, _| {
            let file = FILE.lock().unwrap().clone().unwrap();
            let other = Connection::open(file)?;
            other.execute(
                if MODE.load(Ordering::SeqCst) == 0 {
                    "UPDATE projects SET display_name='External edit after commit'"
                } else {
                    "DELETE FROM projects"
                },
                [],
            )?;
            MUTATED.store(true, Ordering::SeqCst);
            Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
                None,
            ))
        }));
        let result =
            replace_project_repositories(&m.conn, &id, before.fingerprint(), &["committed".into()]);
        assert!(MUTATED.load(Ordering::SeqCst));
        assert!(matches!(
            result,
            Err(ProjectMembershipError::OutcomeUnknown)
        ));
        assert_eq!(m.conn.total_changes(), changes + 1);
        assert!(m.conn.is_autocommit());
        m.conn.wal_hook(None);
        drop(m);
        *FILE.lock().unwrap() = None;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn actual_immutable_typescript_fixture_supports_membership_without_conversion() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/TypeScript-v18.sqlite");
    let original = std::fs::read(&source).unwrap();
    let dir = std::env::temp_dir().join(format!("shelby-membership-ts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("memory.db");
    std::fs::write(&path, &original).unwrap();
    let m = Memory::open(&path).unwrap();
    let id = crate::identity::derive_existing_project_id("shelby").unwrap();
    let before = current(&m, &id);
    let mut desired = before.repositories().to_vec();
    desired.push("https://github.com/synthetic/local".into());
    let ProjectMembershipUpdate::Applied(after) =
        replace_project_repositories(&m.conn, &id, before.fingerprint(), &desired).unwrap()
    else {
        panic!()
    };
    assert_eq!(m.schema_version().unwrap(), 18);
    drop(m);
    let m = Memory::open(&path).unwrap();
    assert_eq!(current(&m, &id).fingerprint(), after.fingerprint());
    assert_eq!(m.schema_version().unwrap(), 18);
    drop(m);
    assert_eq!(std::fs::read(source).unwrap(), original);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn two_independent_writers_cannot_both_apply_the_same_full_snapshot() {
    use std::sync::{Arc, Barrier};
    let (m, id, dir) = disk_fixture();
    let before = current(&m, &id);
    let fingerprint = before.fingerprint().to_owned();
    drop(m);
    let barrier = Arc::new(Barrier::new(3));
    let workers = ["one", "two"].map(|value| {
        let (barrier, id, fp, path) = (
            barrier.clone(),
            id.clone(),
            fingerprint.clone(),
            dir.join("memory.db"),
        );
        std::thread::spawn(move || {
            let m = Memory::open(path).unwrap();
            barrier.wait();
            replace_project_repositories(&m.conn, &id, &fp, &[value.into()])
        })
    });
    barrier.wait();
    let results = workers.map(|w| w.join().unwrap());
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Ok(ProjectMembershipUpdate::Applied(_))))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(ProjectMembershipError::Conflict)))
            .count(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
#[ignore = "Synthetic maximum supported snapshot/CAS benchmark; run explicitly in release mode"]
fn maximum_supported_snapshot_benchmark() {
    let (m, id, dir) = disk_fixture();
    let values = vec!["\0".repeat(327); 100];
    let mut raw = serde_json::to_string(&values).unwrap();
    raw.push_str(&" ".repeat(262144 - raw.len()));
    m.conn.execute("UPDATE projects SET display_name=?1,member_repos=?2,member_paths=?2,created_at=?3,updated_at=?3",params!["x".repeat(512),raw,"t".repeat(128)]).unwrap();
    m.conn
        .execute("DELETE FROM project_slug_aliases", [])
        .unwrap();
    for n in 0..100 {
        m.conn
            .execute(
                "INSERT INTO project_slug_aliases VALUES(?1,?2,'retired',?3,?3)",
                params![format!("a{n:03}{}", "a".repeat(124)), id, "t".repeat(128)],
            )
            .unwrap();
    }
    for mode in ["warm", "reopened"] {
        let opened = if mode == "reopened" {
            Some(Memory::open(dir.join("memory.db")).unwrap())
        } else {
            None
        };
        let conn = opened.as_ref().map_or(&m.conn, |m| &m.conn);
        let mut read_ms = Vec::new();
        let mut noop_ms = Vec::new();
        for _ in 0..10 {
            let start = std::time::Instant::now();
            let s = inspect_project_membership(conn, &id).unwrap().unwrap();
            read_ms.push(start.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(s.repositories().len(), 100);
            assert_eq!(s.aliases.len(), 100);
            let start = std::time::Instant::now();
            assert!(matches!(
                replace_project_repositories(conn, &id, s.fingerprint(), s.repositories()),
                Ok(ProjectMembershipUpdate::Unchanged(_))
            ));
            noop_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let s = current(opened.as_ref().unwrap_or(&m), &id);
        let mut desired = s.repositories().to_vec();
        desired[0] = "x".repeat(327);
        let start = std::time::Instant::now();
        assert!(matches!(
            replace_project_repositories(conn, &id, s.fingerprint(), &desired),
            Ok(ProjectMembershipUpdate::Applied(_))
        ));
        let write_ms = start.elapsed().as_secs_f64() * 1000.0;
        println!(
            "MEMBERSHIP_BENCH {}",
            serde_json::json!({"mode":mode,"snapshotMaxMs":read_ms.iter().copied().fold(0.0,f64::max),"noopMaxMs":noop_ms.iter().copied().fold(0.0,f64::max),"writeMs":write_ms,"repositories":100,"aliases":100,"sourceJSONBytesPerArray":262144,"decodedBytesPerArray":32700,"note":"Reopened connection; OS cache not purged; wall time bounds caller guard hold"})
        );
        // Restore the full-width source container for the next connection measurement.
        conn.execute(
            "UPDATE projects SET member_repos=?1,updated_at=?2",
            params![raw, "t".repeat(128)],
        )
        .unwrap();
    }
    drop(m);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn postcommit_evidence_cleanup_failure_requires_retained_connection_quarantine() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static COMMITTED: AtomicBool = AtomicBool::new(false);
    let (m, id, dir) = disk_fixture();
    let before = current(&m, &id);
    m.conn
        .authorizer(Some(|ctx: AuthContext<'_>| {
            if COMMITTED.load(Ordering::SeqCst)
                && matches!(
                    ctx.action,
                    AuthAction::Transaction {
                        operation: TransactionOperation::Rollback
                    }
                )
            {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
    m.conn.wal_hook(Some(|_, _| {
        COMMITTED.store(true, Ordering::SeqCst);
        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
            None,
        ))
    }));
    assert!(matches!(
        replace_project_repositories(&m.conn, &id, before.fingerprint(), &["committed".into()]),
        Err(ProjectMembershipError::UnusableConnection)
    ));
    assert!(!m.conn.is_autocommit());
    m.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    m.conn.wal_hook(None);
    m.conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(current(&m, &id).repositories(), ["committed"]);
    drop(m);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn overbound_aliases_use_compound_index_without_unbounded_vm_work() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    for count in [101, 10_000, 100_000] {
        let (m, id) = fixture();
        let before = current(&m, &id);
        m.conn
            .execute_batch("BEGIN IMMEDIATE;DELETE FROM project_slug_aliases")
            .unwrap();
        {
            let mut insert = m
                .conn
                .prepare("INSERT INTO project_slug_aliases VALUES(?1,?2,'retired','claimed',NULL)")
                .unwrap();
            for i in (0..count).rev() {
                insert
                    .execute(params![format!("alias-{i:06}"), id])
                    .unwrap();
            }
        }
        m.conn.execute_batch("COMMIT;ANALYZE").unwrap();
        schema::check(&m.conn).unwrap();
        // Exact production SELECT projection, predicate, ordering and overflow sentinel.
        let sql = "SELECT CASE WHEN typeof(slug)='text' AND length(CAST(slug AS BLOB))<=128 THEN slug ELSE NULL END,CASE WHEN typeof(project_id)='text' AND length(CAST(project_id AS BLOB))<=36 THEN project_id ELSE NULL END,CASE WHEN typeof(status)='text' AND length(CAST(status AS BLOB))<=9 THEN status ELSE NULL END,CASE WHEN typeof(claimed_at)='text' AND length(CAST(claimed_at AS BLOB))<=128 THEN claimed_at ELSE NULL END,CASE WHEN retired_at IS NULL THEN NULL WHEN typeof(retired_at)='text' AND length(CAST(retired_at AS BLOB))<=128 THEN retired_at ELSE X'' END FROM main.project_slug_aliases WHERE project_id=?1 ORDER BY slug COLLATE BINARY LIMIT 101";
        let plan = m
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map([&id], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            plan.iter()
                .any(|s| s.contains("sqlite_autoindex_project_slug_aliases_2")),
            "{plan:?}"
        );
        assert!(!plan.iter().any(|s| s.contains("TEMP B-TREE")), "{plan:?}");
        let changes = m.conn.total_changes();
        for write in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let aborted = Arc::new(AtomicBool::new(false));
            let (n, stop) = (calls.clone(), aborted.clone());
            m.conn
                .progress_handler(
                    1,
                    Some(move || {
                        let steps = n.fetch_add(1, Ordering::SeqCst) + 1;
                        if steps > 50_000 {
                            stop.store(true, Ordering::SeqCst);
                            true
                        } else {
                            false
                        }
                    }),
                )
                .unwrap();
            let result = if write {
                replace_project_repositories(&m.conn, &id, before.fingerprint(), &[]).map(|_| ())
            } else {
                inspect_project_membership(&m.conn, &id).map(|_| ())
            };
            m.conn.progress_handler(0, None::<fn() -> bool>).unwrap();
            assert!(matches!(result, Err(ProjectMembershipError::Unavailable)));
            assert!(!aborted.load(Ordering::SeqCst), "unbounded scan at {count}");
            assert!(m.conn.is_autocommit());
            assert_eq!(m.conn.total_changes(), changes);
            println!(
                "ALIAS_BOUND {}",
                serde_json::json!({"aliases":count,"write":write,"vmSteps":calls.load(Ordering::SeqCst),"plan":plan})
            );
        }
    }
}

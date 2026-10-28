use super::*;
use serde_json::{Value, json};

const TIME: &str = "2026-09-06T12:00:00.123456789Z";
const ID: &str = "13BE542E-6271-448A-AD2B-7B5D265F080F";

fn package() -> Value {
    json!({
        "format":"shelby-swift-migration", "version":1, "sourceProduct":"shelby-swift",
        "sourceSchema":18, "sourceAppVersion":null,
        "adapterReference":"9322ac1a5e64ae2b8a9eddacddd10eddae198edd",
        "exportId":"85108372-c80d-46cf-8d99-0e9add7122d3", "exportedAt":TIME,
        "selectedScopes":["unscoped:personal"], "projects":[], "aliases":[], "edges":[],
        "memories":[{"sourceId":ID, "content":"Café\nNUL\u{0}text", "summary":null,
            "metadata":{"type":null,"topics":["Knowledge Graph","knowledge_graph"],"people":null,
                "actionItems":["Read carefully"],"dates":null,"extra":{"sensitivity":"private"}},
            "omissions":{}, "source":null, "sourceAgent":null, "sourceTrust":"trusted",
            "visibility":"personal", "projectId":null,"projectAlias":null,
            "createdAt":TIME,"updatedAt":TIME,"lastConfirmedAt":null,"reinforcementCount":3,"consolidatedInto":null}],
        "categories":{"projects":{"count":0,"status":"complete"},"memories":{"count":1,"status":"complete"},
            "edges":{"count":0,"status":"complete"},"tasks":{"count":0,"status":"notSelected"},
            "conversations":{"count":0,"status":"notSelected"}},
        "policy":{"automaticEligibility":"disabledOnImport","trust":"unverifiedExceptExternal",
            "omittedSourceFields":["embedding","accountAndHostIdentity","syncState","projectPathsAndRepositories","importAuthority"]}
    })
}

#[test]
fn prepares_explicit_personal_package_with_nullable_source_fields() {
    assert!(prepare(&serde_json::to_vec(&package()).unwrap()).is_ok());
}

#[test]
fn rejects_wrong_schema_missing_nullable_and_unknown_authority() {
    let mut value = package();
    value["sourceSchema"] = json!(17);
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
    value = package();
    value["memories"][0]
        .as_object_mut()
        .unwrap()
        .remove("summary");
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
    value = package();
    value["memories"][0]["metadata"]["extra"]["briefEligible"] = json!(true);
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
}

const SWIFT_FIXTURE: &[u8] =
    include_bytes!("../../../../tests/fixtures/Swift Import/Synthetic Swift Export.json");
#[test]
fn imports_actual_swift_fixture_and_reexports_without_duplicate_records() {
    let prepared = prepare(SWIFT_FIXTURE).unwrap();
    let mut memory = crate::Memory::open_in_memory().unwrap();
    assert_eq!(
        inspect_target(&memory.conn, &prepared).unwrap(),
        TargetState::Fresh
    );
    let receipt = apply(&mut memory.conn, &prepared).unwrap();
    assert_eq!(receipt.outcome, ImportOutcome::Created);
    let thought =
        crate::thoughts::get_thought(&memory.conn, "11111111-1111-4111-8111-111111111111")
            .unwrap()
            .unwrap();
    assert_eq!(
        thought.content,
        "Lantern meets Thursday.\nThe access phrase is KESTREL-702000. 雪"
    );
    assert_eq!(thought.trust_level, crate::thoughts::TrustLevel::Unverified);
    assert_eq!(thought.created_at, "2026-01-01T00:00:00.123456789Z");
    assert_eq!(thought.topics, vec!["launch"]);
    let metadata = thought.metadata.unwrap();
    assert_eq!(metadata["extra"]["briefEligible"], false);
    assert_eq!(metadata["extra"]["status"], "done");
    assert!(!metadata.contains_key("type"));
    assert!(!metadata.contains_key("topics"));
    assert_eq!(prepared.projects()[0].updated_at, "2026-01-02 00:00:00");
    assert!(prepared.projects()[0].pinned);
    let mut reexport: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    reexport["exportId"] = json!(ID);
    reexport["exportedAt"] = json!(TIME);
    let again = apply(
        &mut memory.conn,
        &prepare(&serde_json::to_vec(&reexport).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(again.outcome, ImportOutcome::AlreadyImported);
    assert_eq!(again.batch_id, receipt.batch_id);
}

#[test]
fn malformed_identity_scopes_duplicate_keys_and_payload_changes_fail_closed() {
    for path in ["sourceId", "legacySlug"] {
        let mut value: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
        value["projects"][0][path] = json!(ID);
        assert!(
            prepare(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{path}"
        );
    }
    let mut value = package();
    value["memories"][0]["projectAlias"] = json!("unknown");
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
    let text = String::from_utf8(serde_json::to_vec(&package()).unwrap()).unwrap();
    let duplicate = text.replacen("\"summary\":null", "\"summary\":null,\"summary\":null", 1);
    assert!(prepare(duplicate.as_bytes()).is_err());
    let nested = text.replacen(
        "\"sensitivity\":\"private\"",
        "\"sensitivity\":\"private\",\"sensitivity\":\"normal\"",
        1,
    );
    assert!(prepare(nested.as_bytes()).is_err());
    let mut memory = crate::Memory::open_in_memory().unwrap();
    let prepared = prepare(&serde_json::to_vec(&package()).unwrap()).unwrap();
    apply(&mut memory.conn, &prepared).unwrap();
    let mut changed = package();
    changed["memories"][0]["content"] = json!("changed");
    assert!(matches!(
        apply(
            &mut memory.conn,
            &prepare(&serde_json::to_vec(&changed).unwrap()).unwrap()
        ),
        Err(ImportError::Conflict)
    ));
    memory
        .conn
        .execute("UPDATE thoughts SET summary='edited'", [])
        .unwrap();
    assert!(matches!(
        apply(&mut memory.conn, &prepared),
        Err(ImportError::Conflict)
    ));
}

#[test]
fn parse_limits_and_strict_nullable_metadata_hold_before_target_access() {
    let mut cases = Vec::new();
    let mut value = package();
    value["memories"][0]["content"] = json!("x".repeat(1048577));
    cases.push(value);
    let mut value = package();
    value["memories"][0]["summary"] = json!("x".repeat(65537));
    cases.push(value);
    let mut value = package();
    value["memories"][0]["source"] = json!("x".repeat(4097));
    cases.push(value);
    let mut value = package();
    value["memories"][0]["metadata"]["topics"] = json!(vec!["x"; 257]);
    cases.push(value);
    let mut value = package();
    value["memories"][0]["metadata"]["topics"] = json!(vec!["x".repeat(4096); 256]);
    cases.push(value);
    let mut value = package();
    value["memories"][0]["metadata"]["extra"]["status"] = Value::Null;
    cases.push(value);
    let mut value = package();
    value["memories"][0]["metadata"]["people"] = json!([1]);
    cases.push(value);
    let mut value = package();
    value["memories"][0]["reinforcementCount"] = json!(u64::MAX);
    cases.push(value);
    let mut value = package();
    value["memories"][0]["createdAt"] = json!("2026-02-30T00:00:00Z");
    cases.push(value);
    let mut value = package();
    value["memories"][0]["sourceId"] = json!("00000000-0000-0000-0000-000000000000");
    cases.push(value);
    let mut value = package();
    value["memories"][0]["omissions"]["metadata.unknown"] = json!(1);
    cases.push(value);
    let mut value = package();
    value["categories"]["tasks"] = json!({"status":"complete","count":1});
    cases.push(value);
    let mut value = package();
    value["selectedScopes"] = json!(["unscoped:shared"]);
    cases.push(value);
    let mut value = package();
    value["selectedScopes"] = json!(["unscoped:personal", "unscoped:personal"]);
    cases.push(value);
    for (i, value) in cases.iter().enumerate() {
        assert!(
            prepare(&serde_json::to_vec(value).unwrap()).is_err(),
            "case {i}"
        );
    }
    assert!(prepare(&vec![b' '; MAX_BYTES + 1]).is_err());
    let at_limit = json!({"values":vec!["x";256]});
    #[derive(serde::Deserialize)]
    struct Bound {
        values: wire::List<wire::Label, 256>,
    }
    assert_eq!(
        serde_json::from_value::<Bound>(at_limit)
            .unwrap()
            .values
            .len(),
        256
    );
    assert!(serde_json::from_value::<Bound>(json!({"values":vec!["x";257]})).is_err());
}

#[test]
fn complete_closure_and_exact_edge_semantics_survive_reopen() {
    let mut value: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    value["memories"][0]["consolidatedInto"] = value["memories"][1]["sourceId"].clone();
    value["memories"][0]["reinforcementCount"] = json!(9);
    value["memories"][0]["lastConfirmedAt"] = json!("2026-01-01T04:00:00.000000001-07:00");
    value["edges"][0]["validFrom"] = json!("2027-01-01T00:00:00Z");
    value["edges"][0]["validUntil"] = json!("2026-01-01T00:00:00Z");
    let prepared = prepare(&serde_json::to_vec(&value).unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!("swift-import-{}", uuid::Uuid::new_v4()));
    let path = dir.join("memory.db");
    {
        let mut memory = crate::Memory::open(&path).unwrap();
        apply(&mut memory.conn, &prepared).unwrap();
    }
    let memory = crate::Memory::open(&path).unwrap();
    assert_eq!(memory.schema_version().unwrap(), 18);
    assert_eq!(
        inspect_target(&memory.conn, &prepared).unwrap(),
        TargetState::AlreadyImported
    );
    let thought =
        crate::thoughts::get_thought(&memory.conn, "11111111-1111-4111-8111-111111111111")
            .unwrap()
            .unwrap();
    assert_eq!(thought.reinforcement_count, 9);
    assert_eq!(
        thought.last_confirmed_at.as_deref(),
        Some("2026-01-01T04:00:00.000000001-07:00")
    );
    let edge = crate::edges::get_edge(
        &memory.conn,
        value["edges"][0]["sourceId"].as_str().unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(edge.metadata.as_ref().unwrap()["claim"], "Meeting weekday");
    assert!(!crate::edges::is_active_at(&edge, TIME).unwrap());
    let payload: String = memory
        .conn
        .query_row(
            "SELECT source_payload FROM shelby_swift_import_entities WHERE entity_kind='edge'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap()["sourceWeight"],
        7.5
    );
    drop(memory);
    std::fs::remove_dir_all(dir).unwrap();
    value["memories"][1]["consolidatedInto"] = value["memories"][0]["sourceId"].clone();
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
    value["memories"][1]["consolidatedInto"] = Value::Null;
    value["memories"][0]["consolidatedInto"] = json!(ID);
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn uuid_spelling_is_equivalent_but_alias_and_source_facts_conflict() {
    let mut value: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    let prepared = prepare(&serde_json::to_vec(&value).unwrap()).unwrap();
    let mut memory = crate::Memory::open_in_memory().unwrap();
    apply(&mut memory.conn, &prepared).unwrap();
    value["memories"][0]["sourceId"] = json!(
        value["memories"][0]["sourceId"]
            .as_str()
            .unwrap()
            .to_uppercase()
    );
    assert_eq!(
        apply(
            &mut memory.conn,
            &prepare(&serde_json::to_vec(&value).unwrap()).unwrap()
        )
        .unwrap()
        .outcome,
        ImportOutcome::AlreadyImported
    );
    memory
        .conn
        .execute(
            "UPDATE project_slug_aliases SET claimed_at='2026-02-01T00:00:00Z'",
            [],
        )
        .unwrap();
    assert!(matches!(
        inspect_target(&memory.conn, &prepared),
        Err(ImportError::Conflict)
    ));
}

#[test]
fn used_and_mixed_profiles_never_receive_a_partial_merge() {
    let prepared = prepare(SWIFT_FIXTURE).unwrap();
    let mut memory = crate::Memory::open_in_memory().unwrap();
    crate::thoughts::insert_thought(
        &memory.conn,
        &crate::thoughts::ThoughtInput {
            content: "Existing".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        apply(&mut memory.conn, &prepared),
        Err(ImportError::Conflict)
    ));
    assert_eq!(count(&memory.conn, "thoughts").unwrap(), 1);
    assert!(!table_exists(&memory.conn, BATCHES).unwrap());
    let mut memory = crate::Memory::open_in_memory().unwrap();
    apply(&mut memory.conn, &prepared).unwrap();
    let mut extended: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    let mut new = extended["memories"][0].clone();
    new["sourceId"] = json!(ID);
    extended["memories"].as_array_mut().unwrap().push(new);
    extended["categories"]["memories"]["count"] = json!(3);
    assert!(matches!(
        apply(
            &mut memory.conn,
            &prepare(&serde_json::to_vec(&extended).unwrap()).unwrap()
        ),
        Err(ImportError::Conflict)
    ));
    assert_eq!(count(&memory.conn, "thoughts").unwrap(), 2);
}

#[test]
fn independent_connections_serialize_freshness_before_insertion() {
    let dir = std::env::temp_dir().join(format!("swift-race-{}", uuid::Uuid::new_v4()));
    let path = dir.join("memory.db");
    drop(crate::Memory::open(&path).unwrap());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut memory = crate::Memory::open(path).unwrap();
                memory
                    .conn
                    .busy_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                let mut value = package();
                if i == 1 {
                    value["memories"][0]["sourceId"] =
                        json!("11111111-1111-4111-8111-111111111111");
                }
                let prepared = prepare(&serde_json::to_vec(&value).unwrap()).unwrap();
                assert_eq!(
                    inspect_target(&memory.conn, &prepared).unwrap(),
                    TargetState::Fresh
                );
                barrier.wait();
                apply(&mut memory.conn, &prepared)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(ImportError::Conflict)))
            .count(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}

fn remove_authorizer(conn: &Connection) {
    conn.authorizer(None::<fn(rusqlite::hooks::AuthContext<'_>) -> rusqlite::hooks::Authorization>)
        .unwrap();
}
#[test]
fn every_create_insert_and_ledger_fault_restores_empty_v18_without_optional_tables() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let prepared = prepare(SWIFT_FIXTURE).unwrap();
    for (action, table) in [
        ("create", BATCHES),
        ("create", ENTITIES),
        ("insert", BATCHES),
        ("insert", "projects"),
        ("insert", "project_slug_aliases"),
        ("insert", "thoughts"),
        ("insert", "edges"),
        ("insert", ENTITIES),
    ] {
        let mut memory = crate::Memory::open_in_memory().unwrap();
        memory.conn.authorizer(Some(move |ctx:AuthContext<'_>| {
            if matches!(ctx.action,AuthAction::CreateTable {table_name} if action=="create" && table_name==table)
                || matches!(ctx.action,AuthAction::Insert {table_name} if action=="insert" && table_name==table) {Authorization::Deny} else {Authorization::Allow}
        })).unwrap();
        assert!(
            apply(&mut memory.conn, &prepared).is_err(),
            "{action} {table}"
        );
        assert!(memory.conn.is_autocommit());
        remove_authorizer(&memory.conn);
        assert_eq!(count(&memory.conn, "thoughts").unwrap(), 0);
        assert_eq!(count(&memory.conn, "projects").unwrap(), 0);
        assert_eq!(count(&memory.conn, "edges").unwrap(), 0);
        assert_eq!(count(&memory.conn, "project_slug_aliases").unwrap(), 0);
        assert!(!table_exists(&memory.conn, BATCHES).unwrap());
        assert!(!table_exists(&memory.conn, ENTITIES).unwrap());
        assert_eq!(memory.schema_version().unwrap(), 18);
    }
}

#[test]
fn commit_rejection_rolls_back_and_cleanup_failure_quarantines_connection() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization, TransactionOperation};
    let prepared = prepare(SWIFT_FIXTURE).unwrap();
    let mut memory = crate::Memory::open_in_memory().unwrap();
    memory.conn.commit_hook(Some(|| true)).unwrap();
    assert!(matches!(
        apply(&mut memory.conn, &prepared),
        Err(ImportError::Database)
    ));
    assert!(memory.conn.is_autocommit());
    assert!(!table_exists(&memory.conn, BATCHES).unwrap());
    assert_eq!(count(&memory.conn, "thoughts").unwrap(), 0);
    memory.conn.commit_hook(None::<fn() -> bool>).unwrap();
    memory
        .conn
        .authorizer(Some(|ctx: AuthContext<'_>| {
            if matches!(
                ctx.action,
                AuthAction::Insert {
                    table_name: "edges"
                } | AuthAction::Transaction {
                    operation: TransactionOperation::Rollback
                }
            ) {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
    assert!(matches!(
        apply(&mut memory.conn, &prepared),
        Err(ImportError::UnusableConnection)
    ));
    assert!(!memory.conn.is_autocommit());
    remove_authorizer(&memory.conn);
    memory.conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        inspect_target(&memory.conn, &prepared).unwrap(),
        TargetState::Fresh
    );
}

#[test]
fn failed_postcommit_readback_never_claims_rollback_and_retry_reads_durable_batch() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let prepared = prepare(SWIFT_FIXTURE).unwrap();
    let mut memory = crate::Memory::open_in_memory().unwrap();
    let committed = Arc::new(AtomicBool::new(false));
    let hook_flag = committed.clone();
    memory
        .conn
        .commit_hook(Some(move || {
            hook_flag.store(true, Ordering::SeqCst);
            false
        }))
        .unwrap();
    memory
        .conn
        .authorizer(Some(move |ctx: AuthContext<'_>| {
            if committed.load(Ordering::SeqCst)
                && matches!(
                    ctx.action,
                    AuthAction::Read {
                        table_name: "thoughts",
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
    assert!(matches!(
        apply(&mut memory.conn, &prepared),
        Err(ImportError::Database)
    ));
    assert!(memory.conn.is_autocommit());
    remove_authorizer(&memory.conn);
    assert_eq!(count(&memory.conn, "thoughts").unwrap(), 2);
    let batch: String = memory
        .conn
        .query_row(
            "SELECT batch_id FROM shelby_swift_import_batches",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let retry = apply(&mut memory.conn, &prepared).unwrap();
    assert_eq!(retry.outcome, ImportOutcome::AlreadyImported);
    assert_eq!(retry.batch_id, batch);
    assert_eq!(count(&memory.conn, "thoughts").unwrap(), 2);
}

#[test]
fn inspection_does_not_create_tables_and_never_steals_callers_transaction() {
    let prepared = prepare(SWIFT_FIXTURE).unwrap();
    let mut memory = crate::Memory::open_in_memory().unwrap();
    let schema: String = memory
        .conn
        .query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        inspect_target(&memory.conn, &prepared).unwrap(),
        TargetState::Fresh
    );
    assert_eq!(
        memory
            .conn
            .query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        schema
    );
    memory.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(matches!(
        apply(&mut memory.conn, &prepared),
        Err(ImportError::Invalid(_))
    ));
    assert!(matches!(
        inspect_target(&memory.conn, &prepared),
        Err(ImportError::Invalid(_))
    ));
    assert!(!memory.conn.is_autocommit());
    memory.conn.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn canonical_retrieval_keeps_imported_text_fenced_and_refutation_claims_exact() {
    let mut value: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    for memory in value["memories"].as_array_mut().unwrap() {
        memory["visibility"] = json!("personal");
    }
    value["memories"][0]["sourceTrust"] = json!("external");
    let mut memory = crate::Memory::open_in_memory().unwrap();
    apply(
        &mut memory.conn,
        &prepare(&serde_json::to_vec(&value).unwrap()).unwrap(),
    )
    .unwrap();
    let found = crate::fts::search_thoughts(
        &memory.conn,
        &crate::fts::SearchOptions {
            query: "KESTREL".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(found.total_count, 1);
    let scope = crate::brief::BriefScopeInput {
        all_projects: true,
        ..Default::default()
    };
    let candidates = crate::brief::load_brief_candidates(&memory.conn, TIME, &scope).unwrap();
    let source = candidates
        .iter()
        .find(|t| t.id == "22222222-2222-4222-8222-222222222222")
        .unwrap();
    assert!(!source.actively_refuted);
    assert_eq!(source.refuted_claims, vec!["Meeting weekday"]);
    let target = candidates
        .iter()
        .find(|t| t.id == "11111111-1111-4111-8111-111111111111")
        .unwrap();
    assert_eq!(
        target.trust_level,
        Some(crate::thoughts::TrustLevel::External)
    );
    assert_eq!(
        target.metadata.as_ref().unwrap()["extra"]["briefEligible"],
        false
    );
    assert!(
        crate::edges::get_connections_at(&memory.conn, &source.id, None, TIME)
            .unwrap()
            .len()
            == 1
    );
    memory
        .conn
        .execute("UPDATE edges SET metadata='{\"claim\":\"  \"}'", [])
        .unwrap();
    let candidates = crate::brief::load_brief_candidates(&memory.conn, TIME, &scope).unwrap();
    let source = candidates
        .iter()
        .find(|t| t.id == "22222222-2222-4222-8222-222222222222")
        .unwrap();
    assert!(source.actively_refuted);
    assert!(source.refuted_claims.is_empty());
    assert!(
        crate::brief::select_brief_items(&candidates, crate::brief::BriefScope::Full, &scope, TIME)
            .items
            .is_empty()
    );
    let tool = crate::tools::get_thought_tool(
        &memory,
        &json!({"id":"11111111-1111-4111-8111-111111111111"}),
    );
    assert!(tool.text.contains("untrusted_memory"));
}

#[test]
fn kept_annotation_is_validated_as_identity_without_becoming_review_authority() {
    let mut value = package();
    value["memories"][0]["metadata"]["extra"]["contradiction_kept"] = json!("invented-id");
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
    value["memories"][0]["metadata"]["extra"]["contradiction_kept"] = json!(ID);
    value["memories"][0]["metadata"]["extra"]["contradiction_resolved"] = json!("true");
    let mut memory = crate::Memory::open_in_memory().unwrap();
    apply(
        &mut memory.conn,
        &prepare(&serde_json::to_vec(&value).unwrap()).unwrap(),
    )
    .unwrap();
    let thought = crate::thoughts::get_thought(&memory.conn, &ID.to_lowercase())
        .unwrap()
        .unwrap();
    assert_eq!(thought.metadata.unwrap()["extra"]["contradiction_kept"], ID);
    assert_eq!(count(&memory.conn, "edges").unwrap(), 0);
}

#[test]
fn actual_wire_arrays_enforce_all_record_count_limits_during_deserialization() {
    for (field, limit) in [
        ("projects", 200),
        ("aliases", 2000),
        ("memories", 1000),
        ("edges", 5000),
        ("selectedScopes", 202),
    ] {
        let mut value: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
        let item = value[field][0].clone();
        value[field] = Value::Array(vec![item.clone(); limit]);
        assert!(
            serde_json::from_value::<wire::Package>(value.clone()).is_ok(),
            "{field} at limit"
        );
        value[field].as_array_mut().unwrap().push(item);
        assert!(
            serde_json::from_value::<wire::Package>(value).is_err(),
            "{field} over limit"
        );
    }
}

#[test]
fn nulls_and_uuid_case_preserve_provenance_while_exact_source_changes_conflict() {
    let mut memory = crate::Memory::open_in_memory().unwrap();
    let prepared = prepare(&serde_json::to_vec(&package()).unwrap()).unwrap();
    apply(&mut memory.conn, &prepared).unwrap();
    let thought = crate::thoughts::get_thought(&memory.conn, &ID.to_lowercase())
        .unwrap()
        .unwrap();
    assert_eq!(thought.content, "Café\nNUL\u{0}text");
    assert_eq!(thought.r#type, "note");
    assert_eq!(thought.source, "unknown");
    assert_eq!(thought.visibility, "personal");
    assert_eq!(thought.reinforcement_count, 3);
    assert_eq!(thought.topics, vec!["knowledge-graph"]);
    let mut value = package();
    value["memories"][0]["sourceId"] = json!(ID.to_lowercase());
    assert_eq!(
        apply(
            &mut memory.conn,
            &prepare(&serde_json::to_vec(&value).unwrap()).unwrap()
        )
        .unwrap()
        .outcome,
        ImportOutcome::AlreadyImported
    );
    let provenance: String = memory
        .conn
        .query_row(
            "SELECT source_payload FROM shelby_swift_import_entities WHERE entity_kind='memory'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let source: Value = serde_json::from_str(&provenance).unwrap();
    assert_eq!(source["sourceId"], ID);
    assert_eq!(source["metadata"]["type"], Value::Null);
    assert_eq!(source["metadata"]["people"], Value::Null);
    value["memories"][0]["metadata"]["type"] = json!("note");
    assert!(matches!(
        apply(
            &mut memory.conn,
            &prepare(&serde_json::to_vec(&value).unwrap()).unwrap()
        ),
        Err(ImportError::Conflict)
    ));
    memory.conn.execute("UPDATE shelby_swift_import_entities SET source_payload=replace(source_payload,'Café','Changed')",[]).unwrap();
    assert!(matches!(
        inspect_target(&memory.conn, &prepared),
        Err(ImportError::Conflict)
    ));
}

#[test]
fn canonical_project_alias_rules_accept_legacy_and_reject_collisions() {
    let mut value: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    let legacy = identity::derive_existing_project_id("lantern").unwrap();
    let old = "33333333-3333-4333-8333-333333333333";
    value = serde_json::from_str(&serde_json::to_string(&value).unwrap().replace(old, &legacy))
        .unwrap();
    value["projects"][0]["legacySlug"] = json!("lantern");
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_ok());
    let mut retired = value.clone();
    retired["aliases"][0]["status"] = json!("retired");
    retired["aliases"][0]["retiredAt"] = json!(TIME);
    assert!(prepare(&serde_json::to_vec(&retired).unwrap()).is_err());
    let mut wrong = value.clone();
    wrong["aliases"][0]["projectId"] = json!(ID);
    assert!(prepare(&serde_json::to_vec(&wrong).unwrap()).is_err());
    let mut long = value.clone();
    long["projects"][0]["currentSlug"] = json!("a".repeat(129));
    assert!(prepare(&serde_json::to_vec(&long).unwrap()).is_err());
    let mut second = value["projects"][0].clone();
    second["sourceId"] = json!(ID);
    second["legacySlug"] = json!(ID);
    second["currentSlug"] = json!("second-lantern");
    value["projects"].as_array_mut().unwrap().push(second);
    value["aliases"].as_array_mut().unwrap().push(json!({"slug":"second-lantern","projectId":ID,"status":"tentative","claimedAt":TIME,"retiredAt":null}));
    value["selectedScopes"]
        .as_array_mut()
        .unwrap()
        .push(json!(ID));
    value["categories"]["projects"]["count"] = json!(2);
    let mut memory = crate::Memory::open_in_memory().unwrap();
    apply(
        &mut memory.conn,
        &prepare(&serde_json::to_vec(&value).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(count(&memory.conn, "projects").unwrap(), 2);
}

#[test]
fn precommit_validation_read_failure_rolls_back_the_whole_import() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let inserted = Arc::new(AtomicBool::new(false));
    let flag = inserted.clone();
    let mut memory = crate::Memory::open_in_memory().unwrap();
    memory
        .conn
        .authorizer(Some(move |ctx: AuthContext<'_>| {
            if matches!(
                ctx.action,
                AuthAction::Insert {
                    table_name: "edges"
                }
            ) {
                flag.store(true, Ordering::SeqCst);
            }
            if flag.load(Ordering::SeqCst)
                && matches!(
                    ctx.action,
                    AuthAction::Read {
                        table_name: "thoughts",
                        column_name: "id"
                    }
                )
            {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
    assert!(apply(&mut memory.conn, &prepare(SWIFT_FIXTURE).unwrap()).is_err());
    assert!(inserted.load(Ordering::SeqCst));
    assert!(memory.conn.is_autocommit());
    remove_authorizer(&memory.conn);
    assert_eq!(count(&memory.conn, "thoughts").unwrap(), 0);
    assert!(!table_exists(&memory.conn, BATCHES).unwrap());
}

#[test]
fn existing_connected_clients_and_history_are_preserved_by_refusing_activation_candidate() {
    for sql in [
        "INSERT INTO oauth_clients VALUES ('synthetic-client','Existing client','[]',1)",
        "INSERT INTO feedback (id,created_at,feature,label) VALUES ('f','2026-01-01T00:00:00Z','test','keep')",
        "INSERT INTO search_telemetry (id,created_at) VALUES ('s','2026-01-01T00:00:00Z')",
    ] {
        let mut memory = crate::Memory::open_in_memory().unwrap();
        let prepared = prepare(SWIFT_FIXTURE).unwrap();
        memory.conn.execute(sql, []).unwrap();
        assert!(inspect_target(&memory.conn, &prepared).is_err());
        assert!(apply(&mut memory.conn, &prepared).is_err());
        assert!(!table_exists(&memory.conn, BATCHES).unwrap());
        assert_eq!(
            count(&memory.conn, "oauth_clients").unwrap()
                + count(&memory.conn, "feedback").unwrap()
                + count(&memory.conn, "search_telemetry").unwrap(),
            1
        );
    }
}

#[test]
fn legacy_frozen_alias_cannot_disappear_or_transfer_ownership() {
    let mut value: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    let legacy = identity::derive_existing_project_id("lantern").unwrap();
    value = serde_json::from_str(
        &serde_json::to_string(&value)
            .unwrap()
            .replace("33333333-3333-4333-8333-333333333333", &legacy),
    )
    .unwrap();
    value["projects"][0]["legacySlug"] = json!("lantern");
    value["projects"][0]["currentSlug"] = json!("renamed-lantern");
    value["aliases"][0]["slug"] = json!("renamed-lantern");
    for t in value["memories"].as_array_mut().unwrap() {
        t["projectAlias"] = json!("renamed-lantern");
    }
    value["edges"][0]["projectAlias"] = json!("renamed-lantern");
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
    value["aliases"].as_array_mut().unwrap().push(json!({"slug":"lantern","projectId":legacy,"status":"retired","claimedAt":"2025-01-01T00:00:00Z","retiredAt":TIME}));
    assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_ok());
}

#[test]
fn changed_batch_provenance_is_not_accepted_as_an_unchanged_retry() {
    let mut memory = crate::Memory::open_in_memory().unwrap();
    let prepared = prepare(SWIFT_FIXTURE).unwrap();
    apply(&mut memory.conn, &prepared).unwrap();
    memory
        .conn
        .execute(
            "UPDATE shelby_swift_import_batches SET source_manifest='corrupted'",
            [],
        )
        .unwrap();
    assert!(matches!(
        inspect_target(&memory.conn, &prepared),
        Err(ImportError::Conflict)
    ));
}

#[test]
fn target_projection_budget_is_checked_independently_of_input_size() {
    let mut value = package();
    value["memories"][0]["metadata"]["actionItems"] = json!(vec!["\"".repeat(4096); 31]);
    let template = value["memories"][0].clone();
    let mut records = Vec::new();
    for _ in 0..110 {
        let mut row = template.clone();
        row["sourceId"] = json!(uuid::Uuid::new_v4().to_string());
        records.push(row);
    }
    value["memories"] = Value::Array(records);
    value["categories"]["memories"]["count"] = json!(110);
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() < MAX_BYTES);
    assert!(matches!(
        prepare(&bytes),
        Err(ImportError::Invalid("prepared size"))
    ));
}

#[test]
fn imported_maximum_byte_edge_ids_remain_pageable_and_addressable() {
    for id in ["a".repeat(512), "é".repeat(256), "\0".repeat(512)] {
        let mut value: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
        value["edges"][0]["sourceId"] = json!(id);
        let mut second = value["edges"][0].clone();
        second["sourceId"] = json!("🦀-last");
        second["sourceMemoryId"] = value["edges"][0]["targetMemoryId"].clone();
        second["targetMemoryId"] = value["edges"][0]["sourceMemoryId"].clone();
        value["edges"].as_array_mut().unwrap().push(second);
        value["categories"]["edges"]["count"] = json!(2);
        let prepared = prepare(&serde_json::to_vec(&value).unwrap()).unwrap();
        let mut memory = crate::Memory::open_in_memory().unwrap();
        apply(&mut memory.conn, &prepared).unwrap();
        let first =
            crate::edges::active_edges_page(&memory.conn, "refuted_by", TIME, None, 1).unwrap();
        assert_eq!(first.edges[0].id, id);
        assert_eq!(first.next_after_id.as_deref(), Some(id.as_str()));
        let second = crate::edges::active_edges_page(
            &memory.conn,
            "refuted_by",
            TIME,
            first.next_after_id.as_deref(),
            1,
        )
        .unwrap();
        assert_eq!(second.edges[0].id, "🦀-last");
        assert!(second.next_after_id.is_none());
        assert_eq!(
            crate::edges::get_edge(&memory.conn, &id)
                .unwrap()
                .unwrap()
                .id,
            id
        );
        value["edges"][0]["sourceId"] = json!("a".repeat(513));
        assert!(prepare(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(
            crate::edges::active_edges_page(
                &memory.conn,
                "refuted_by",
                TIME,
                Some(&"a".repeat(513)),
                1
            )
            .is_err()
        );
    }
}

fn v2_package() -> Value {
    let mut v: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    v["version"] = json!(2);
    v["localTasks"] = json!([{"sourceKey":"workItem:local:44444444-4444-4444-8444-444444444444","tracker":"local","externalId":"44444444-4444-4444-8444-444444444444","projectId":"33333333-3333-4333-8333-333333333333","projectAlias":"lantern","kind":"todo","title":"Review the launch agenda","state":"open","origin":"manual","triageState":"accepted","updatedAt":"2026-01-02 00:00:00","snoozeUntil":null,"labels":[],"relations":[],"parentExternalId":null,"omissions":{"url":0,"originSessionId":1,"lastSeenAt":0}}]);
    v["taskPolicy"] = json!({"dueDates":"notInferred","sessions":"notRead","omittedSourceFields":["url","originSessionId","lastSeenAt","sessionLinks"]});
    v["categories"]["tasks"] = json!({"count":1,"status":"selectedSubset","supportedCount":2,"unselectedCount":1,"excluded":{"externalTracker":0,"epic":0,"dismissed":0,"snoozed":0,"parent":0,"dependencies":0,"labels":0,"externalOrigin":0}});
    v
}
#[test]
fn v2_tasks_are_validated_projections_and_receipt_bound_without_canonical_rows() {
    let p = prepare(&serde_json::to_vec(&v2_package()).unwrap()).unwrap();
    assert_eq!(p.wire_version(), 2);
    assert_eq!(p.local_tasks().len(), 1);
    assert_eq!(
        p.local_tasks()[0].task_id,
        "44444444-4444-4444-8444-444444444444"
    );
    assert_eq!(p.task_selection().unwrap().omissions.origin_session_id, 1);
    let mut memory = crate::Memory::open_in_memory().unwrap();
    apply(&mut memory.conn, &p).unwrap();
    let mut changed = v2_package();
    changed["localTasks"][0]["title"] = json!("Changed");
    let changed = prepare(&serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(inspect_target(&memory.conn, &changed).is_err());
    assert_eq!(
        memory
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='app_tasks'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn task_version_presence_shape_and_count_boundaries_are_closed() {
    let original = v2_package();
    for field in ["localTasks", "taskPolicy"] {
        let mut v = original.clone();
        v.as_object_mut().unwrap().remove(field);
        assert!(
            prepare(&serde_json::to_vec(&v).unwrap()).is_err(),
            "missing {field}"
        );
        v = original.clone();
        v[field] = Value::Null;
        assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
        v = original.clone();
        v["version"] = json!(1);
        assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    for (field, value) in [
        ("count", json!(0)),
        ("count", json!(-1)),
        ("count", json!(1.0)),
        ("supportedCount", json!(2001)),
        ("unselectedCount", json!(2)),
        ("status", json!("complete")),
        ("extra", json!(0)),
    ] {
        let mut v = original.clone();
        v["categories"]["tasks"][field] = value;
        assert!(
            prepare(&serde_json::to_vec(&v).unwrap()).is_err(),
            "{field}"
        );
    }
    let mut v = original.clone();
    v["categories"]["tasks"]["excluded"]["epic"] = json!(2000);
    assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
    for field in ["supportedCount", "unselectedCount", "excluded"] {
        let mut v = original.clone();
        v["categories"]["tasks"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    let mut v = original.clone();
    v["localTasks"] = json!([]);
    v["categories"]["tasks"]["count"] = json!(0);
    v["categories"]["tasks"]["unselectedCount"] = json!(2);
    let empty = prepare(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(empty.local_tasks().len(), 0);
    assert_ne!(
        empty.batch_fingerprint(),
        prepare(SWIFT_FIXTURE).unwrap().batch_fingerprint()
    );
}
#[test]
fn task_intrinsic_validation_rejects_unsupported_or_ambiguous_values() {
    let base = v2_package()["localTasks"][0].clone();
    for (field, value) in [
        ("title", json!(" \n")),
        ("title", json!("é".repeat(501))),
        ("title", json!("😀".repeat(501))),
        ("state", json!("running")),
        ("state", json!("ópen")),
        ("origin", json!("external")),
        ("triageState", json!("dismissed")),
        ("tracker", json!("github")),
        ("kind", json!("epic")),
        ("externalId", json!("not-uuid")),
        ("sourceKey", json!("workItem:local:wrong")),
        ("snoozeUntil", json!("")),
        ("parentExternalId", json!("")),
        ("labels", Value::Null),
        ("labels", json!(["a"])),
        ("relations", json!([{}])),
        ("updatedAt", json!("2026-02-30")),
        ("unknown", json!(true)),
    ] {
        let mut t = base.clone();
        t[field] = value;
        assert!(
            inspect_local_task_source(&t.to_string()).is_err(),
            "{field}"
        );
    }
    for field in base.as_object().unwrap().keys() {
        let mut t = base.clone();
        t.as_object_mut().unwrap().remove(field);
        assert!(
            inspect_local_task_source(&t.to_string()).is_err(),
            "missing {field}"
        );
    }
    for value in [json!(2), json!(-1), json!(1.0), json!("1"), Value::Null] {
        let mut t = base.clone();
        t["omissions"]["url"] = value;
        assert!(inspect_local_task_source(&t.to_string()).is_err());
    }
    for t in [
        base.to_string()
            .replacen("\"title\":", "\"title\":\"first\",\"tit\\u006ce\":", 1),
        base.to_string()
            .replacen("\"url\":0", "\"url\":0,\"u\\u0072l\":1", 1),
    ] {
        assert!(inspect_local_task_source(&t).is_err());
    }
    let mut accepted = base;
    accepted["title"] = json!("😀".repeat(500));
    accepted["state"] = json!("CLOSED");
    accepted["updatedAt"] = json!("1969-12-31T23:59:59.999500Z");
    assert_eq!(
        inspect_local_task_source(&accepted.to_string())
            .unwrap()
            .updated_at,
        "1969-12-31T23:59:59.999500Z"
    );
    assert!(inspect_local_task_source(&" ".repeat(32769)).is_err());
}
#[test]
fn tasks_bind_selected_project_aliases_and_normalize_only_uuid_identity() {
    let mut v = v2_package();
    v["localTasks"][0]["externalId"] = json!("ABCDEFAB-4444-4444-8444-444444444444");
    v["localTasks"][0]["sourceKey"] = json!("workItem:local:abcdefab-4444-4444-8444-444444444444");
    let upper = prepare(&serde_json::to_vec(&v).unwrap()).unwrap();
    v["localTasks"][0]["externalId"] = json!("abcdefab-4444-4444-8444-444444444444");
    let lower = prepare(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(upper.batch_fingerprint(), lower.batch_fingerprint());
    assert_ne!(
        upper.local_tasks()[0].source_payload,
        lower.local_tasks()[0].source_payload
    );
    v["localTasks"][0]["state"] = json!("OPEN");
    assert_ne!(
        prepare(&serde_json::to_vec(&v).unwrap())
            .unwrap()
            .batch_fingerprint(),
        lower.batch_fingerprint()
    );
    for alias in ["missing", "other-project"] {
        v["localTasks"][0]["projectAlias"] = json!(alias);
        assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    v["localTasks"][0]["projectAlias"] = v["localTasks"][0]["projectId"].clone();
    assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_ok());
    v["localTasks"][0]["projectId"] = json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
}
#[test]
fn task_batch_order_policy_and_selected_omission_identity() {
    let mut v = v2_package();
    let first = v["localTasks"][0].clone();
    let mut second = first.clone();
    second["externalId"] = json!("55555555-5555-4555-8555-555555555555");
    second["sourceKey"] = json!("workItem:local:55555555-5555-4555-8555-555555555555");
    v["localTasks"] = json!([first, second]);
    v["categories"]["tasks"]["count"] = json!(2);
    v["categories"]["tasks"]["unselectedCount"] = json!(0);
    let p = prepare(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(p.task_selection().unwrap().omissions.origin_session_id, 2);
    v["localTasks"].as_array_mut().unwrap().reverse();
    v["taskPolicy"]["omittedSourceFields"]
        .as_array_mut()
        .unwrap()
        .reverse();
    v["exportId"] = json!("55555555-5555-4555-8555-555555555555");
    assert_eq!(
        p.batch_fingerprint(),
        prepare(&serde_json::to_vec(&v).unwrap())
            .unwrap()
            .batch_fingerprint()
    );
    for mutate in 0..3 {
        let mut changed = v.clone();
        match mutate {
            0 => changed["localTasks"][0]["omissions"]["url"] = json!(1),
            1 => changed["categories"]["tasks"]["excluded"]["epic"] = json!(1),
            _ => {
                changed["categories"]["tasks"]["supportedCount"] = json!(3);
                changed["categories"]["tasks"]["unselectedCount"] = json!(1);
            }
        }
        assert_ne!(
            p.batch_fingerprint(),
            prepare(&serde_json::to_vec(&changed).unwrap())
                .unwrap()
                .batch_fingerprint()
        );
    }
    v["localTasks"][1] = v["localTasks"][0].clone();
    assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
}

#[test]
fn task_catalog_ceiling_and_duplicate_decoded_package_keys_are_rejected() {
    let mut v = v2_package();
    let task = v["localTasks"][0].clone();
    v["localTasks"] = json!(
        (0..2000)
            .map(|n| {
                let mut t = task.clone();
                let id = format!("{n:08x}-4444-4444-8444-444444444444");
                t["externalId"] = json!(id);
                t["sourceKey"] = json!(format!("workItem:local:{id}"));
                t
            })
            .collect::<Vec<_>>()
    );
    v["categories"]["tasks"]["count"] = json!(2000);
    v["categories"]["tasks"]["supportedCount"] = json!(2000);
    v["categories"]["tasks"]["unselectedCount"] = json!(0);
    assert_eq!(
        prepare(&serde_json::to_vec(&v).unwrap())
            .unwrap()
            .local_tasks()
            .len(),
        2000
    );
    v["localTasks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"unread":"over-limit element"}));
    assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
    let text = v2_package().to_string();
    for text in [
        text.replacen(
            "\"localTasks\":",
            "\"localTasks\":[],\"local\\u0054asks\":",
            1,
        ),
        text.replacen(
            "\"dueDates\":",
            "\"dueDates\":\"notInferred\",\"due\\u0044ates\":",
            1,
        ),
        text.replacen(
            "\"externalTracker\":0",
            "\"externalTracker\":0,\"external\\u0054racker\":1",
            1,
        ),
    ] {
        assert!(prepare(text.as_bytes()).is_err());
    }
    let mut v = v2_package();
    v["taskPolicy"]["omittedSourceFields"] = json!(["url", "url", "lastSeenAt", "sessionLinks"]);
    assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
    let mut v: Value = serde_json::from_slice(SWIFT_FIXTURE).unwrap();
    for field in ["localTasks", "taskPolicy"] {
        v[field] = Value::Null;
        assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
        v.as_object_mut().unwrap().remove(field);
    }
    v["localTasks"] = json!([]);
    assert!(prepare(&serde_json::to_vec(&v).unwrap()).is_err());
}

#[test]
fn actual_swift_v2_bytes_prepare_apply_reopen_and_bind_tasks_without_task_entities() {
    let bytes =
        include_bytes!("../../../../tests/fixtures/Swift Import/Synthetic Swift Task Export.json");
    let prepared = prepare(bytes).unwrap();
    assert_eq!(prepared.local_tasks().len(), 3);
    let selection = prepared.task_selection().unwrap();
    assert_eq!(
        (
            selection.count,
            selection.supported_count,
            selection.unselected_count
        ),
        (3, 4, 1)
    );
    assert_eq!(
        selection.omissions,
        PreparedTaskOmissions {
            url: 1,
            origin_session_id: 1,
            last_seen_at: 1
        }
    );
    assert_eq!(
        selection.excluded,
        PreparedTaskExcluded {
            external_tracker: 1,
            epic: 1,
            dismissed: 1,
            snoozed: 1,
            parent: 1,
            dependencies: 1,
            labels: 1,
            external_origin: 1
        }
    );
    assert!(
        prepared
            .local_tasks()
            .iter()
            .any(|t| t.origin == "agent_spawned" && t.triage_state == "inbox")
    );
    assert!(prepared.local_tasks().iter().any(|t| t.state == "CLOSED"));
    for t in prepared.local_tasks() {
        assert_eq!(&inspect_local_task_source(&t.source_payload).unwrap(), t);
    }
    let dir = std::env::temp_dir().join(format!("swift-task-import-{}", uuid::Uuid::new_v4()));
    let path = dir.join("memory.db");
    let mut memory = crate::Memory::open(&path).unwrap();
    let first = apply(&mut memory.conn, &prepared).unwrap();
    assert_eq!(
        inspect_target(&memory.conn, &prepared).unwrap(),
        TargetState::AlreadyImported
    );
    assert_eq!(
        apply(&mut memory.conn, &prepared).unwrap().batch_id,
        first.batch_id
    );
    assert_eq!(memory.conn.query_row("SELECT count(*) FROM shelby_swift_import_entities WHERE entity_kind NOT IN ('project','alias','memory','edge')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    drop(memory);
    let memory = crate::Memory::open(&path).unwrap();
    assert_eq!(
        inspect_target(&memory.conn, &prepared).unwrap(),
        TargetState::AlreadyImported
    );
    assert_eq!(memory.schema_version().unwrap(), 18);
    drop(memory);
    std::fs::remove_dir_all(dir).unwrap();
    let mut v1: Value = serde_json::from_slice(bytes).unwrap();
    v1["version"] = json!(1);
    v1.as_object_mut().unwrap().remove("localTasks");
    v1.as_object_mut().unwrap().remove("taskPolicy");
    v1["categories"]["tasks"] = json!({"count":0,"status":"notSelected"});
    let original = prepare(&serde_json::to_vec(&v1).unwrap()).unwrap();
    assert_eq!(prepared.entities.len(), original.entities.len());
    for e in &original.entities {
        let other = prepared
            .entities
            .iter()
            .find(|v| v.kind == e.kind && v.id == e.id)
            .unwrap();
        assert_eq!(e.fingerprint, other.fingerprint);
        assert_eq!(e.source_payload, other.source_payload);
    }
}

#[test]
fn v1_committed_fixture_preserves_pre_s3_exact_source_manifest_and_batch_identity() {
    // Independently captured by running 8c9c75816d67a0b98f05ed556f77a666be8319ed.
    let p = prepare(SWIFT_FIXTURE).unwrap();
    assert_eq!(p.wire_version(), 1);
    assert!(p.local_tasks().is_empty());
    assert!(p.task_selection().is_none());
    assert_eq!(
        p.batch_fingerprint(),
        "96668e9e536561af30373fe72caa18a4a3a11b5a65182972283f9c500490780f"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(p.manifest.as_bytes())),
        "fcfcf363b3fcccd6464e4cfddeb89285173f332c71293c98a1f10117c1ee7dc0"
    );
    let sources = p
        .entities
        .iter()
        .map(|e| json!([e.kind, e.id, e.fingerprint, e.source_payload]))
        .collect::<Vec<_>>();
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(encoded(&sources).unwrap().as_bytes())
        ),
        "2cc482b8cfa0c252915e2afc742778866a434dcf1a54c40967fef6cec5f441cc"
    );
    assert_eq!(POLICY_VERSION, 1);
}

#[path = "chats/tests.rs"]
mod chat_tests;

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

fn v3() -> Value {
    let mut v = v2_package();
    v["version"] = json!(3);
    v["categories"]["conversations"] = json!({"count":0,"status":"selectedSubset","catalogCount":1,"unselectedCount":1,"itemCount":0});
    v["chatArchive"] = json!({"projectionVersion":1,"sourceStoreID":"00000000-1111-4111-8111-111111111111",
        "policy":{"content":"visibleTextAndInertAnnotations","execution":"neverReplay","continuation":"newChatOnly","reasoning":"omitted","toolDetails":"omitted","sourceVersions":"swift9322-projection1","attachments":"normalizedJPEGWithRegeneratedPreview","imagePolicyVersion":1},
        "conversations":[],"images":[],"imageDisclosure":{"normalizedImages":0,"regeneratedPreviews":0}});
    v
}
fn bits(n: f64) -> Value {
    json!({"referenceSecondsBits":format!("{:016x}",n.to_bits())})
}
fn rid(n: u64) -> String {
    format!("019f0000-0000-7000-8000-{n:012x}")
}
fn legacy_item() -> Value {
    json!({"ordinal":0,"source":{"kind":"legacy","messageUUID":"00000001-4444-4444-8444-444444444444","createdAt":bits(0.0),"sequence":0,"turnID":"exact legacy turn\u{0}"},"body":{"kind":"user","text":"雪e\u{301}\nNUL\u{0}tail"},"status":{"kind":"complete"},"attachmentCount":0,"attachmentIDs":[]})
}
fn conversation(item: Value) -> Value {
    json!({"sourceSessionID":"default-migrated","title":"","projectRegistryID":null,"projectEntityID":"inert\u{0}","sourceState":"closed","createdAt":bits(-0.0),"lastActiveAt":bits(0.0),"pinnedAt":null,"closedAt":null,"hiddenAt":null,"origin":{"kind":"legacy"},"omissions":{"hiddenRows":0,"reasoningRows":0,"toolDetails":0,"usageFields":0,"sessionDetails":0,"attachmentReferences":0,"knownRecords":0,"privateRecords":0,"pendingInputs":0,"whitespaceRows":0},"items":[item]})
}
fn one_chat() -> Value {
    let mut v = v3();
    v["chatArchive"]["conversations"] = json!([conversation(legacy_item())]);
    v["categories"]["conversations"] = json!({"count":1,"status":"selectedSubset","catalogCount":1,"unselectedCount":0,"itemCount":1});
    v
}
fn runtime_chat() -> Value {
    let mut v = one_chat();
    let c = &mut v["chatArchive"]["conversations"][0];
    c["origin"] = json!({"kind":"runtime","runtimeSessionID":rid(1),"lastSequence":i64::MAX});
    c["items"][0]["source"] = json!({"kind":"runtime","eventID":rid(2),"itemID":rid(3),"toolInvocationID":null,"sessionSequence":1,"timestampUnixMilliseconds":i64::MIN,"turnID":rid(4),"runID":rid(5),"branchID":rid(6),"sensitivity":"standard"});
    v
}
fn parsed(v: &Value) -> Result<PreparedImport> {
    prepare(&serde_json::to_vec(v).unwrap())
}
#[test]
fn chat_matches_frozen_swift_zero_width_whitespace_rule() {
    let mut v = runtime_chat();
    v["chatArchive"]["conversations"][0]["items"][0]["body"] =
        json!({"kind":"assistant","text":"\u{200b}"});
    assert!(parsed(&v).is_err());
}

fn image(id: &str, bytes: &[u8]) -> Value {
    let hash = format!("{:x}", Sha256::digest(bytes));
    let derived = json!({"mediaType":"image/jpeg","byteCount":bytes.len(),"sha256":hash,"width":1,"height":1,"dataBase64":STANDARD.encode(bytes)});
    json!({"sourceAttachmentID":id,"sourceFilename":"original\u{0}/雪.jpg","sourceMediaType":"inert/untrusted",
        "sourceData":{"byteCount":10,"sha256":format!("{:x}",Sha256::digest(b"original!!"))},"sourceThumbnail":{"byteCount":0,"sha256":format!("{:x}",Sha256::digest([]))},"jpeg":derived,"preview":derived})
}
fn with_image() -> Value {
    let mut v = one_chat();
    let id = "00000009-4444-4444-8444-444444444444";
    v["chatArchive"]["images"] = json!([image(id, &[255, 216, 255, 217])]);
    v["chatArchive"]["imageDisclosure"] = json!({"normalizedImages":1,"regeneratedPreviews":1});
    let c = &mut v["chatArchive"]["conversations"][0];
    c["items"][0]["attachmentCount"] = json!(1);
    c["items"][0]["attachmentIDs"] = json!([id]);
    c["omissions"]["attachmentReferences"] = json!(1);
    v
}
#[test]
fn chat_v3_zero_selection_and_existing_v1_v2_hashes_are_unchanged() {
    for v in [package(), v2_package()] {
        let first = parsed(&v).unwrap();
        assert!(first.chat_archive().is_none());
        for value in [Value::Null, json!({}), json!([])] {
            let mut malformed = v.clone();
            malformed["chatArchive"] = value;
            assert!(parsed(&malformed).is_err());
        }
        let mut malformed = v.clone();
        malformed["categories"]["conversations"]["catalogCount"] = json!(0);
        assert!(parsed(&malformed).is_err());
    }
    let value = v3();
    let p = parsed(&value).unwrap();
    let a = p.chat_archive().unwrap();
    assert!(a.conversations.is_empty());
    assert!(a.images.is_empty());
    let mut changed = value.clone();
    changed["categories"]["conversations"]["catalogCount"] = json!(2);
    changed["categories"]["conversations"]["unselectedCount"] = json!(2);
    assert_ne!(
        p.batch_fingerprint(),
        parsed(&changed).unwrap().batch_fingerprint()
    );
    let mut memory = crate::Memory::open_in_memory().unwrap();
    let receipt = apply(&mut memory.conn, &p).unwrap();
    assert_eq!(receipt.outcome, ImportOutcome::Created);
    assert_eq!(
        apply(&mut memory.conn, &p).unwrap().outcome,
        ImportOutcome::AlreadyImported
    );
    assert_eq!(memory.schema_version().unwrap(), 18);
    assert!(
        !memory
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE name LIKE 'shelby_swift_chat_%'")
            .unwrap()
            .exists([])
            .unwrap()
    );
}
#[test]
fn chat_required_nullable_and_closed_shapes_reject_missing_null_and_extra_keys() {
    let v = one_chat();
    assert!(parsed(&v).is_ok());
    for key in ["chatArchive", "localTasks", "taskPolicy"] {
        let mut bad = v.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(parsed(&bad).is_err(), "{key}");
    }
    for key in [
        "images",
        "imageDisclosure",
        "conversations",
        "projectionVersion",
        "sourceStoreID",
        "policy",
    ] {
        let mut bad = v.clone();
        bad["chatArchive"].as_object_mut().unwrap().remove(key);
        assert!(parsed(&bad).is_err(), "{key}");
    }
    for key in [
        "projectRegistryID",
        "projectEntityID",
        "pinnedAt",
        "closedAt",
        "hiddenAt",
        "items",
    ] {
        let mut bad = v.clone();
        bad["chatArchive"]["conversations"][0]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(parsed(&bad).is_err(), "{key}");
    }
    for path in [
        "/chatArchive",
        "/chatArchive/conversations/0",
        "/chatArchive/conversations/0/items/0",
        "/chatArchive/conversations/0/items/0/source",
        "/chatArchive/conversations/0/items/0/body",
        "/chatArchive/conversations/0/items/0/status",
    ] {
        let mut bad = v.clone();
        bad.pointer_mut(path).unwrap()["extra"] = json!(null);
        assert!(parsed(&bad).is_err(), "{path}");
    }
    let json = serde_json::to_string(&v).unwrap().replace(
        "\"sourceSessionID\":",
        "\"sourceSession\\u0049D\":\"shadow\",\"sourceSessionID\":",
    );
    assert!(prepare(json.as_bytes()).is_err());
}
#[test]
fn chat_source_bits_unicode_and_full_runtime_int64_are_intrinsic() {
    let v = one_chat();
    let p = parsed(&v).unwrap();
    let c = &p.chat_archive().unwrap().conversations[0];
    assert_eq!(c.created_at.reference_seconds_bits, "8000000000000000");
    let item = inspect_chat_item_source(&c.archive_id, &c.items[0].source_payload).unwrap();
    assert_eq!(item, c.items[0]);
    assert!(item.source_payload.contains("\\u0000"));
    assert!(item.source_payload.contains("雪e\u{301}"));
    assert_eq!(
        inspect_chat_conversation_source(&c.source_payload, vec![item]).unwrap(),
        *c
    );
    assert!(!c.source_payload.contains("items"));
    assert!(!c.source_payload.contains("NUL"));
    let mut changed = v.clone();
    changed["chatArchive"]["conversations"][0]["createdAt"] = bits(0.0);
    assert_ne!(
        p.batch_fingerprint(),
        parsed(&changed).unwrap().batch_fingerprint()
    );
    changed["chatArchive"]["conversations"][0]["createdAt"] = bits(-978_307_200.000_5);
    assert_eq!(
        parsed(&changed)
            .unwrap()
            .chat_archive()
            .unwrap()
            .conversations[0]
            .created_at
            .unix_milliseconds,
        -1
    );
    for n in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        -f64::MAX,
    ] {
        let mut bad = v.clone();
        bad["chatArchive"]["conversations"][0]["createdAt"] = bits(n);
        assert!(parsed(&bad).is_err());
    }
    let mut r = runtime_chat();
    let p = parsed(&r).unwrap();
    assert!(
        p.chat_archive().unwrap().conversations[0].items[0]
            .source_payload
            .contains(&i64::MIN.to_string())
    );
    r["chatArchive"]["conversations"][0]["items"][0]["source"]["timestampUnixMilliseconds"] =
        json!(i64::MAX);
    assert!(parsed(&r).is_ok());
    let bad = serde_json::to_string(&r).unwrap().replace(
        &format!("\"timestampUnixMilliseconds\":{}", i64::MAX),
        "\"timestampUnixMilliseconds\":9223372036854775808",
    );
    assert!(prepare(bad.as_bytes()).is_err());
}
#[test]
fn chat_metadata_item_order_and_structured_identity_are_bound() {
    let v = one_chat();
    let p = parsed(&v).unwrap();
    let mut moved = v.clone();
    moved["exportId"] = json!(ID);
    moved["exportedAt"] = json!(TIME);
    assert_eq!(
        p.batch_fingerprint(),
        parsed(&moved).unwrap().batch_fingerprint()
    );
    let mut second = v["chatArchive"]["conversations"][0].clone();
    second["sourceSessionID"] = json!("another:session");
    second["items"][0]["source"]["messageUUID"] = json!("00000002-4444-4444-8444-444444444444");
    moved["chatArchive"]["conversations"]
        .as_array_mut()
        .unwrap()
        .push(second);
    moved["categories"]["conversations"] = json!({"count":2,"status":"selectedSubset","catalogCount":2,"unselectedCount":0,"itemCount":2});
    let ordered = parsed(&moved).unwrap();
    moved["chatArchive"]["conversations"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        ordered.batch_fingerprint(),
        parsed(&moved).unwrap().batch_fingerprint()
    );
    for field in ["sourceSessionID", "title", "projectEntityID"] {
        let mut bad = v.clone();
        bad["chatArchive"]["conversations"][0][field] = json!("changed");
        assert_ne!(
            p.batch_fingerprint(),
            parsed(&bad).unwrap().batch_fingerprint()
        );
    }
    let mut bad = v.clone();
    bad["chatArchive"]["conversations"][0]["projectRegistryID"] =
        json!("ffffffff-ffff-4fff-8fff-ffffffffffff");
    assert!(parsed(&bad).is_err());
    bad = v.clone();
    bad["chatArchive"]["conversations"][0]["items"][0]["ordinal"] = json!(1);
    assert!(parsed(&bad).is_err());
}
#[test]
fn chat_runtime_source_presence_body_and_lineage_are_closed() {
    let v = runtime_chat();
    assert!(parsed(&v).is_ok());
    for key in [
        "itemID",
        "toolInvocationID",
        "turnID",
        "runID",
        "branchID",
        "eventID",
        "sessionSequence",
        "sensitivity",
    ] {
        let mut bad = v.clone();
        bad["chatArchive"]["conversations"][0]["items"][0]["source"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(parsed(&bad).is_err(), "{key}");
    }
    for (field, value) in [
        ("runID", Value::Null),
        ("turnID", json!("legacy turn")),
        ("branchID", json!("019f0000-0000-7000-0000-000000000006")),
        ("sessionSequence", json!(0)),
        ("messageUUID", json!("00000002-4444-4444-8444-444444444444")),
    ] {
        let mut bad = v.clone();
        bad["chatArchive"]["conversations"][0]["items"][0]["source"][field] = value;
        assert!(parsed(&bad).is_err(), "{field}");
    }
    for status in [
        json!({"kind":"queued"}),
        json!({"kind":"failed","reason":""}),
        json!({"kind":"complete","reason":null}),
    ] {
        let mut bad = v.clone();
        bad["chatArchive"]["conversations"][0]["items"][0]["status"] = status;
        assert!(parsed(&bad).is_err());
    }
    let mut tool = v.clone();
    let i = &mut tool["chatArchive"]["conversations"][0]["items"][0];
    i["source"]["itemID"] = Value::Null;
    i["source"]["toolInvocationID"] = json!(rid(7));
    i["body"] = json!({"kind":"tool","name":"search_thoughts","status":"unknown","durationMilliseconds":null});
    i["status"] = json!({"kind":"incomplete"});
    assert!(parsed(&tool).is_ok());
    for name in ["bad/name", "雪", &"x".repeat(129)] {
        let mut bad = tool.clone();
        bad["chatArchive"]["conversations"][0]["items"][0]["body"]["name"] = json!(name);
        assert!(parsed(&bad).is_err());
    }
    tool["chatArchive"]["conversations"][0]["items"][0]["body"]["durationMilliseconds"] = json!(1);
    assert!(parsed(&tool).is_err());
}
#[test]
fn chat_omissions_and_source_origin_associations_reject_forged_counts() {
    let v = one_chat();
    for (key, n) in [
        ("hiddenRows", 70001),
        ("reasoningRows", 70001),
        ("toolDetails", 9),
        ("usageFields", 5),
        ("sessionDetails", 4),
        ("attachmentReferences", 1),
        ("knownRecords", 1),
        ("privateRecords", 1),
        ("pendingInputs", 1),
        ("whitespaceRows", 1),
    ] {
        let mut bad = v.clone();
        bad["chatArchive"]["conversations"][0]["omissions"][key] = json!(n);
        assert!(parsed(&bad).is_err(), "{key}");
    }
    let mut bad = runtime_chat();
    bad["chatArchive"]["conversations"][0]["omissions"]["usageFields"] = json!(1);
    assert!(parsed(&bad).is_err());
    bad = one_chat();
    bad["chatArchive"]["policy"]["reasoning"] = json!("included");
    assert!(parsed(&bad).is_err());
    bad = one_chat();
    bad["chatArchive"]["conversations"][0]["items"][0]["body"] =
        json!({"kind":"tool","name":"legacy 雪","status":"denied","durationMilliseconds":i64::MAX});
    assert!(parsed(&bad).is_ok());
    bad["chatArchive"]["conversations"][0]["items"][0]["status"] = json!({"kind":"incomplete"});
    assert!(parsed(&bad).is_err());
}
#[test]
fn chat_images_decode_once_and_intrinsic_storage_omits_base64() {
    let v = with_image();
    let p = parsed(&v).unwrap();
    let a = p.chat_archive().unwrap();
    let i = &a.images[0];
    assert_eq!(i.jpeg, vec![255, 216, 255, 217]);
    assert!(!i.source_payload.contains("dataBase64"));
    assert!(!a.conversations[0].source_payload.contains("dataBase64"));
    assert_eq!(
        inspect_chat_image_source(&i.source_payload, i.jpeg.clone(), i.preview.clone()).unwrap(),
        *i
    );
    assert!(inspect_chat_image_source(&i.source_payload, vec![0], i.preview.clone()).is_err());
    let mut legacy_duplicate = v.clone();
    let c = &mut legacy_duplicate["chatArchive"]["conversations"][0];
    let id = c["items"][0]["attachmentIDs"][0].clone();
    c["items"][0]["attachmentIDs"] = json!([id, id]);
    c["items"][0]["attachmentCount"] = json!(2);
    c["omissions"]["attachmentReferences"] = json!(2);
    assert!(parsed(&legacy_duplicate).is_err());
    let mut repeated = runtime_chat();
    let id = rid(19);
    repeated["chatArchive"]["images"] = json!([image(&id, &[255, 216, 255, 217])]);
    repeated["chatArchive"]["imageDisclosure"] =
        json!({"normalizedImages":1,"regeneratedPreviews":1});
    let c = &mut repeated["chatArchive"]["conversations"][0];
    c["items"][0]["attachmentIDs"] = json!([id, id]);
    c["items"][0]["attachmentCount"] = json!(2);
    c["omissions"]["attachmentReferences"] = json!(2);
    assert_eq!(
        parsed(&repeated)
            .unwrap()
            .chat_archive()
            .unwrap()
            .images
            .len(),
        1
    );
    let mut second = repeated["chatArchive"]["images"][0].clone();
    second["sourceAttachmentID"] = json!(rid(18));
    repeated["chatArchive"]["images"]
        .as_array_mut()
        .unwrap()
        .push(second.clone());
    repeated["chatArchive"]["conversations"][0]["items"][0]["attachmentIDs"][1] =
        second["sourceAttachmentID"].clone();
    repeated["chatArchive"]["imageDisclosure"] =
        json!({"normalizedImages":2,"regeneratedPreviews":2});
    assert_eq!(
        parsed(&repeated)
            .unwrap()
            .chat_archive()
            .unwrap()
            .images
            .len(),
        2
    );
    repeated["chatArchive"]["images"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert!(parsed(&repeated).is_err());
}
#[test]
fn chat_image_hash_shape_base64_and_reference_corruption_refuse() {
    let v = with_image();
    for (path, value) in [
        ("/chatArchive/images/0/jpeg/dataBase64", json!("/9j/2Q==\n")),
        ("/chatArchive/images/0/jpeg/byteCount", json!(5)),
        ("/chatArchive/images/0/jpeg/sha256", json!("0".repeat(64))),
        ("/chatArchive/images/0/jpeg/mediaType", json!("image/png")),
        ("/chatArchive/images/0/jpeg/width", json!(1569)),
        ("/chatArchive/images/0/preview/width", json!(2)),
        ("/chatArchive/images/0/sourceData/byteCount", json!(0)),
        (
            "/chatArchive/images/0/sourceThumbnail/sha256",
            json!("0".repeat(64)),
        ),
        ("/chatArchive/imageDisclosure/normalizedImages", json!(0)),
        (
            "/chatArchive/conversations/0/items/0/attachmentCount",
            json!(0),
        ),
    ] {
        let mut bad = v.clone();
        *bad.pointer_mut(path).unwrap() = value;
        assert!(parsed(&bad).is_err(), "{path}");
    }
    let mut bad = v.clone();
    bad["chatArchive"]["images"] = json!([]);
    bad["chatArchive"]["imageDisclosure"] = json!({"normalizedImages":0,"regeneratedPreviews":0});
    assert!(parsed(&bad).is_err());
    bad = v.clone();
    bad["chatArchive"]["images"][0]["jpeg"]
        .as_object_mut()
        .unwrap()
        .remove("dataBase64");
    assert!(parsed(&bad).is_err());
    // Framing-only bytes above deliberately pass canonical structural checks; native JPEG decode is required separately.
}
#[test]
fn chat_normalized_slash_escaped_source_budget_applies_before_image_decoding() {
    let mut v = one_chat();
    let mut bytes = vec![255; 524_288];
    bytes[1] = 216;
    bytes[524_287] = 217;
    let ids = (0..19)
        .map(|n| format!("00000009-4444-4444-8444-{n:012x}"))
        .collect::<Vec<_>>();
    v["chatArchive"]["images"] = json!(ids.iter().map(|id| image(id, &bytes)).collect::<Vec<_>>());
    v["chatArchive"]["imageDisclosure"] = json!({"normalizedImages":19,"regeneratedPreviews":19});
    v["chatArchive"]["conversations"][0]["items"][0]["attachmentIDs"] = json!(ids);
    v["chatArchive"]["conversations"][0]["items"][0]["attachmentCount"] = json!(19);
    v["chatArchive"]["conversations"][0]["omissions"]["attachmentReferences"] = json!(19);
    let wire = serde_json::to_vec(&v).unwrap();
    assert!(wire.len() < MAX_BYTES);
    assert!(matches!(
        prepare(&wire),
        Err(ImportError::Invalid("v3 normalized source size"))
    ));
}

#[test]
fn chat_maximum_manifest_stays_small_and_does_not_retain_child_content() {
    let mut v = one_chat();
    let mut conversations = Vec::new();
    for n in 0..100 {
        let mut c = conversation(legacy_item());
        c["sourceSessionID"] = json!(format!("session-{n}"));
        c["items"] = json!([]);
        conversations.push(c);
    }
    v["chatArchive"]["conversations"] = json!(conversations);
    v["categories"]["conversations"] = json!({"count":100,"status":"selectedSubset","catalogCount":1000,"unselectedCount":900,"itemCount":0});
    let p = parsed(&v).unwrap();
    assert!(p.manifest.len() <= 32 * 1024);
    assert!(!p.manifest.contains("session-99"));
    assert_eq!(p.chat_archive().unwrap().conversations.len(), 100);
}
#[test]
fn chat_order_lineage_duplicate_global_identity_and_intrinsic_corruption_refuse() {
    let mut v = runtime_chat();
    let mut second = v["chatArchive"]["conversations"][0]["items"][0].clone();
    second["ordinal"] = json!(1);
    second["source"]["eventID"] = json!(rid(8));
    second["source"]["itemID"] = json!(rid(9));
    second["source"]["sessionSequence"] = json!(2);
    v["chatArchive"]["conversations"][0]["items"]
        .as_array_mut()
        .unwrap()
        .push(second);
    v["categories"]["conversations"]["itemCount"] = json!(2);
    let p = parsed(&v).unwrap();
    let c = &p.chat_archive().unwrap().conversations[0];
    for (field, value) in [
        ("eventID", json!(rid(2))),
        ("itemID", json!(rid(3))),
        ("sessionSequence", json!(1)),
        ("turnID", json!(rid(10))),
        ("branchID", json!(rid(11))),
    ] {
        let mut bad = v.clone();
        bad["chatArchive"]["conversations"][0]["items"][1]["source"][field] = value;
        assert!(parsed(&bad).is_err(), "{field}");
    }
    let mut children = c.items.clone();
    children.reverse();
    assert!(inspect_chat_conversation_source(&c.source_payload, children).is_err());
    let mut metadata: Value = serde_json::from_str(&c.source_payload).unwrap();
    metadata["items"] = json!([]);
    assert!(inspect_chat_conversation_source(&metadata.to_string(), c.items.clone()).is_err());
    let mut source: Value = serde_json::from_str(&c.items[0].source_payload).unwrap();
    source["attachmentCount"] = json!(1);
    assert!(inspect_chat_item_source(&c.archive_id, &source.to_string()).is_err());
    let mut two = v.clone();
    let mut other = two["chatArchive"]["conversations"][0].clone();
    other["sourceSessionID"] = json!("other");
    other["origin"]["runtimeSessionID"] = json!(rid(99));
    two["chatArchive"]["conversations"]
        .as_array_mut()
        .unwrap()
        .push(other);
    two["categories"]["conversations"] = json!({"count":2,"status":"selectedSubset","catalogCount":2,"unselectedCount":0,"itemCount":4});
    assert!(parsed(&two).is_err());
}
#[test]
fn chat_wire_string_integer_and_uuid_boundaries_reject_before_projection() {
    let v = one_chat();
    for (path, value) in [
        (
            "/chatArchive/sourceStoreID",
            json!("00000000-1111-4111-8111-11111111111A"),
        ),
        (
            "/chatArchive/conversations/0/title",
            json!("x".repeat(4097)),
        ),
        (
            "/chatArchive/conversations/0/items/0/body/text",
            json!("x".repeat(1_048_577)),
        ),
        (
            "/chatArchive/conversations/0/items/0/source/sequence",
            json!(1.0),
        ),
        (
            "/chatArchive/conversations/0/createdAt/referenceSecondsBits",
            json!("000000000000000A"),
        ),
    ] {
        let mut bad = v.clone();
        *bad.pointer_mut(path).unwrap() = value;
        assert!(parsed(&bad).is_err(), "{path}");
    }
    let p = parsed(&with_image()).unwrap();
    let a = p.chat_archive().unwrap();
    assert!(validate_chat_archive_rows(&a.conversations, &[]).is_err());
    let mut duplicate = a.images.clone();
    duplicate.push(duplicate[0].clone());
    assert!(validate_chat_archive_rows(&a.conversations, &duplicate).is_err());
}

#[test]
fn chat_actual_swift_emitted_bundle_prepares_applies_and_reopens() {
    let bytes = include_bytes!(
        "../../../../../tests/fixtures/Swift Chat Import/Synthetic Swift Chat Export.json"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "67bfe29b1a941d88eb35289f279789ddef12a137b17abdd1e256e003d5fa8bee"
    );
    let prepared = prepare(bytes).unwrap();
    let archive = prepared.chat_archive().unwrap();
    assert_eq!(prepared.wire_version(), 3);
    assert_eq!(
        (
            prepared.preview().project_count,
            prepared.preview().memory_count,
            prepared.local_tasks().len()
        ),
        (1, 1, 1)
    );
    assert_eq!(
        (
            archive.selection.count,
            archive.selection.item_count,
            archive.images.len()
        ),
        (2, 10, 1)
    );
    let image = &archive.images[0];
    assert!(!image.source_payload.contains("dataBase64"));
    assert_eq!(
        inspect_chat_image_source(
            &image.source_payload,
            image.jpeg.clone(),
            image.preview.clone()
        )
        .unwrap(),
        *image
    );
    for c in &archive.conversations {
        let items = c
            .items
            .iter()
            .map(|i| inspect_chat_item_source(&c.archive_id, &i.source_payload).unwrap())
            .collect();
        assert_eq!(
            inspect_chat_conversation_source(&c.source_payload, items).unwrap(),
            *c
        );
    }
    let dir = std::env::temp_dir().join(format!("swift-chat-import-{}", uuid::Uuid::new_v4()));
    let path = dir.join("memory.db");
    let mut memory = crate::Memory::open(&path).unwrap();
    let first = apply(&mut memory.conn, &prepared).unwrap();
    assert_eq!(
        apply(&mut memory.conn, &prepared).unwrap().batch_id,
        first.batch_id
    );
    drop(memory);
    let memory = crate::Memory::open(&path).unwrap();
    assert_eq!(
        inspect_target(&memory.conn, &prepared).unwrap(),
        TargetState::AlreadyImported
    );
    assert_eq!(memory.schema_version().unwrap(), 18);
    assert!(
        !memory
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE name LIKE 'shelby_swift_chat_%'")
            .unwrap()
            .exists([])
            .unwrap()
    );
    let mut changed: Value = serde_json::from_slice(bytes).unwrap();
    changed["exportId"] = json!(ID);
    changed["exportedAt"] = json!(TIME);
    assert_eq!(
        prepared.batch_fingerprint(),
        parsed(&changed).unwrap().batch_fingerprint()
    );
    changed["chatArchive"]["images"][0]["sourceFilename"] = json!("changed.jpg");
    assert!(inspect_target(&memory.conn, &parsed(&changed).unwrap()).is_err());
    drop(memory);
    std::fs::remove_dir_all(dir).unwrap();
}

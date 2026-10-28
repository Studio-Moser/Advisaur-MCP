//! Inert archive projection. Structural image checks never claim native JPEG decoding.
pub(super) mod wire;
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedImageDisclosure {
    pub normalized_images: u32,
    pub regenerated_previews: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedChatOmissions {
    pub hidden_rows: u32,
    pub reasoning_rows: u32,
    pub tool_details: u32,
    pub usage_fields: u32,
    pub session_details: u32,
    pub attachment_references: u32,
    pub known_records: u32,
    pub private_records: u32,
    pub pending_inputs: u32,
    pub whitespace_rows: u32,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedChatSelection {
    pub count: u32,
    pub catalog_count: u32,
    pub unselected_count: u32,
    pub item_count: u32,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedChatPolicy {
    pub content: String,
    pub execution: String,
    pub continuation: String,
    pub reasoning: String,
    pub tool_details: String,
    pub source_versions: String,
    pub attachments: String,
    pub image_policy_version: u32,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedChatDate {
    pub reference_seconds_bits: String,
    pub unix_milliseconds: i64,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum PreparedChatItemSource {
    Legacy {
        message_uuid: String,
        created_at: PreparedChatDate,
        sequence: i64,
        turn_id: String,
    },
    Runtime {
        event_id: String,
        item_id: Option<String>,
        tool_invocation_id: Option<String>,
        session_sequence: i64,
        timestamp_unix_milliseconds: i64,
        turn_id: String,
        run_id: String,
        branch_id: String,
        sensitivity: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedChatItem {
    pub item_id: String,
    pub archive_id: String,
    pub ordinal: u32,
    pub fingerprint: String,
    pub source_payload: String,
    pub source: PreparedChatItemSource,
    pub attachment_ids: Vec<String>,
    pub duration_milliseconds: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedChatConversation {
    pub archive_id: String,
    pub source_store_id: String,
    pub source_session_id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub runtime_session_id: Option<String>,
    pub last_sequence: Option<i64>,
    pub created_at: PreparedChatDate,
    pub last_active_at: PreparedChatDate,
    pub pinned_at: Option<PreparedChatDate>,
    pub closed_at: Option<PreparedChatDate>,
    pub hidden_at: Option<PreparedChatDate>,
    pub omissions: PreparedChatOmissions,
    pub fingerprint: String,
    pub source_payload: String,
    pub items: Vec<PreparedChatItem>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedChatImage {
    pub source_attachment_id: String,
    pub fingerprint: String,
    pub source_payload: String,
    pub jpeg: Vec<u8>,
    pub preview: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub preview_width: u32,
    pub preview_height: u32,
}
#[derive(Debug)]
pub struct PreparedChatArchive {
    pub source_store_id: String,
    pub selection: PreparedChatSelection,
    pub policy: PreparedChatPolicy,
    pub conversations: Vec<PreparedChatConversation>,
    pub images: Vec<PreparedChatImage>,
    pub image_disclosure: PreparedImageDisclosure,
}

fn canonical_uuid(value: &str) -> Result<String> {
    let id = uuid::Uuid::parse_str(value).map_err(|_| invalid("chat UUID"))?;
    if id.hyphenated().to_string() != value {
        return Err(invalid("chat UUID spelling"));
    }
    Ok(value.to_owned())
}
fn runtime_id(value: &str) -> Result<String> {
    let id = canonical_uuid(value)?;
    let bytes = uuid::Uuid::parse_str(&id)
        .expect("validated UUID")
        .into_bytes();
    if bytes[6] >> 4 != 7 || bytes[8] >> 6 != 2 {
        return Err(invalid("runtime UUID"));
    }
    Ok(id)
}
fn hex(value: &str, n: usize) -> Result<()> {
    if value.len() != n
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("chat hex"));
    }
    Ok(())
}
fn chat_date(value: &wire::Date) -> Result<PreparedChatDate> {
    hex(&value.reference_seconds_bits, 16)?;
    let number = f64::from_bits(
        u64::from_str_radix(&value.reference_seconds_bits, 16).map_err(|_| invalid("chat date"))?,
    );
    let millis = ((number + 978_307_200.0) * 1000.0).floor();
    if !number.is_finite() || !(-62_135_596_800_000.0..=253_402_300_799_999.0).contains(&millis) {
        return Err(invalid("chat date range"));
    }
    Ok(PreparedChatDate {
        reference_seconds_bits: value.reference_seconds_bits.0.clone(),
        unix_milliseconds: millis as i64,
    })
}
fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| invalid("chat required field"))
}
fn value<T: Serialize>(v: &T) -> Result<Value> {
    serde_json::to_value(v).map_err(|_| invalid("chat serialization"))
}
fn archive_id(store: &str, session: &str) -> Result<String> {
    Ok(format!(
        "swift-chat:{}",
        hash(&json!(["shelby-swift-chat-id-v1", PRODUCT, store, session]))?
    ))
}
fn valid_archive_id(value: &str) -> Result<()> {
    hex(
        value
            .strip_prefix("swift-chat:")
            .ok_or_else(|| invalid("archive ID"))?,
        64,
    )
}

fn source(s: &wire::Source) -> Result<PreparedChatItemSource> {
    let turn = required(required(s.turn_id.as_ref())?.as_ref())?.0.clone();
    if &*s.kind == "legacy" {
        if s.event_id.is_some()
            || s.item_id.is_some()
            || s.tool_invocation_id.is_some()
            || s.session_sequence.is_some()
            || s.timestamp_unix_milliseconds.is_some()
            || s.run_id.is_some()
            || s.branch_id.is_some()
            || s.sensitivity.is_some()
        {
            return Err(invalid("legacy source shape"));
        }
        Ok(PreparedChatItemSource::Legacy {
            message_uuid: canonical_uuid(required(s.message_uuid.as_ref())?)?,
            created_at: chat_date(required(s.created_at.as_ref())?)?,
            sequence: required(s.sequence)?,
            turn_id: turn,
        })
    } else if &*s.kind == "runtime" {
        if s.message_uuid.is_some() || s.created_at.is_some() || s.sequence.is_some() {
            return Err(invalid("runtime source shape"));
        }
        let item = required(s.item_id.as_ref())?
            .as_ref()
            .map(|v| runtime_id(v))
            .transpose()?;
        let invocation = required(s.tool_invocation_id.as_ref())?
            .as_ref()
            .map(|v| runtime_id(v))
            .transpose()?;
        let sensitivity = required(s.sensitivity.as_ref())?;
        one_of(
            sensitivity,
            &["standard", "private", "sensitive", "restricted"],
            "chat sensitivity",
        )?;
        let sequence = required(s.session_sequence)?;
        if sequence <= 0 {
            return Err(invalid("runtime sequence"));
        }
        Ok(PreparedChatItemSource::Runtime {
            event_id: runtime_id(required(s.event_id.as_ref())?)?,
            item_id: item,
            tool_invocation_id: invocation,
            session_sequence: sequence,
            timestamp_unix_milliseconds: required(s.timestamp_unix_milliseconds)?,
            turn_id: runtime_id(&turn)?,
            run_id: runtime_id(required(required(s.run_id.as_ref())?.as_ref())?)?,
            branch_id: runtime_id(required(required(s.branch_id.as_ref())?.as_ref())?)?,
            sensitivity: sensitivity.0.clone(),
        })
    } else {
        Err(invalid("chat source kind"))
    }
}
// Frozen Foundation.whitespacesAndNewlines includes U+200B in addition to Unicode White_Space.
fn swift_blank(value: &str) -> bool {
    value.chars().all(|c| c.is_whitespace() || c == '\u{200b}')
}
fn item_body(item: &wire::Item, source: &PreparedChatItemSource) -> Result<Option<i64>> {
    let b = &item.body;
    let s = &item.status;
    let failed = &*s.kind == "failed";
    one_of(
        &s.kind,
        &["complete", "failed", "cancelled", "incomplete", "queued"],
        "chat item status",
    )?;
    if s.reason.is_some() != failed {
        return Err(invalid("chat status shape"));
    }
    let tool = &*b.kind == "tool";
    let duration = if tool {
        if b.text.is_some() || swift_blank(required(b.name.as_ref())?) {
            return Err(invalid("chat tool shape"));
        }
        one_of(
            required(b.status.as_ref())?,
            &[
                "running",
                "needsApproval",
                "success",
                "error",
                "denied",
                "unknown",
            ],
            "chat tool status",
        )?;
        let duration = required(b.duration_milliseconds)?;
        if duration.is_some_and(|v| v < 0) {
            return Err(invalid("chat duration"));
        }
        duration
    } else {
        one_of(&b.kind, &["user", "assistant", "notice"], "chat body kind")?;
        required(b.text.as_ref())?;
        if b.name.is_some() || b.status.is_some() || b.duration_milliseconds.is_some() {
            return Err(invalid("chat text shape"));
        }
        None
    };
    let empty_reason = s.reason.as_ref().is_none_or(|v| v.is_empty());
    match source {
        PreparedChatItemSource::Runtime {
            item_id,
            tool_invocation_id,
            ..
        } => {
            if tool != tool_invocation_id.is_some() || tool == item_id.is_some() {
                return Err(invalid("runtime item identity"));
            }
            if &*b.kind != "user" && item.attachment_count != 0 {
                return Err(invalid("runtime attachment owner"));
            }
            if item.attachment_count > 64 {
                return Err(invalid("runtime attachment count"));
            }
            match &*b.kind {
                "user" if &*s.kind == "complete" => {}
                "assistant"
                    if &*s.kind == "complete" && !swift_blank(required(b.text.as_ref())?) => {}
                "notice"
                    if required(b.text.as_ref())?.is_empty()
                        && ((failed && empty_reason) || &*s.kind == "cancelled") => {}
                "tool" => {
                    let name = required(b.name.as_ref())?;
                    if name.len() > 128
                        || !name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                    {
                        return Err(invalid("runtime tool name"));
                    }
                    match (&**required(b.status.as_ref())?, &*s.kind) {
                        ("success", "complete") | ("error", "failed") if empty_reason => {}
                        ("unknown", "incomplete") if duration.is_none() => {}
                        _ => return Err(invalid("runtime tool relation")),
                    }
                }
                _ => return Err(invalid("runtime body relation")),
            }
        }
        PreparedChatItemSource::Legacy { .. } => {
            if tool && &*s.kind != "queued" {
                match (&**required(b.status.as_ref())?, &*s.kind) {
                    ("success" | "denied", "complete") | ("error", "failed") if empty_reason => {}
                    ("running" | "needsApproval" | "unknown", "incomplete") => {}
                    _ => return Err(invalid("legacy tool relation")),
                }
            }
        }
    }
    Ok(duration)
}
fn project_item(archive: &str, item: wire::Item) -> Result<PreparedChatItem> {
    valid_archive_id(archive)?;
    if item.ordinal >= 20_000 || item.attachment_count as usize != item.attachment_ids.len() {
        return Err(invalid("chat item counts"));
    }
    let source = source(&item.source)?;
    let duration = item_body(&item, &source)?;
    let mut refs = Vec::new();
    let mut seen = BTreeSet::new();
    for id in item.attachment_ids.iter() {
        let id = match &source {
            PreparedChatItemSource::Legacy { .. } => {
                let id = canonical_uuid(id)?;
                if !seen.insert(id.clone()) {
                    return Err(invalid("legacy duplicate attachment"));
                }
                id
            }
            PreparedChatItemSource::Runtime { .. } => runtime_id(id)?,
        };
        refs.push(id);
    }
    let (kind, id) = match &source {
        PreparedChatItemSource::Legacy { message_uuid, .. } => ("legacy", message_uuid),
        PreparedChatItemSource::Runtime { event_id, .. } => ("runtime", event_id),
    };
    let item_id = format!(
        "swift-chat-item:{}",
        hash(&json!(["shelby-swift-chat-item-id-v1", archive, kind, id]))?
    );
    let fingerprint = hash(&json!([
        "shelby-swift-chat-item-v1",
        archive,
        value(&item)?
    ]))?;
    Ok(PreparedChatItem {
        item_id,
        archive_id: archive.to_owned(),
        ordinal: item.ordinal,
        fingerprint,
        source_payload: encoded(&item)?,
        source,
        attachment_ids: refs,
        duration_milliseconds: duration,
    })
}
/// Intrinsic validation only. Does not authorize a parent/project/import or replay content.
pub fn inspect_chat_item_source(archive_id: &str, payload: &str) -> Result<PreparedChatItem> {
    if payload.len() > 13 * 1_048_576 {
        return Err(invalid("chat item source size"));
    }
    let item = serde_json::from_str(payload).map_err(|_| invalid("chat item source shape"))?;
    project_item(archive_id, item)
}

impl PreparedChatOmissions {
    fn values(&self) -> [u32; 10] {
        [
            self.hidden_rows,
            self.reasoning_rows,
            self.tool_details,
            self.usage_fields,
            self.session_details,
            self.attachment_references,
            self.known_records,
            self.private_records,
            self.pending_inputs,
            self.whitespace_rows,
        ]
    }
    fn validate(&self, item_count: usize, runtime: bool) -> Result<()> {
        if self
            .values()
            .iter()
            .zip([
                70_000, 70_000, 160_000, 80_000, 300, 2_661_753, 50_000, 50_000, 50_000, 50_000,
            ])
            .any(|(a, b)| *a > b)
            || self.session_details > 3
            || self.tool_details as usize > 8 * item_count
            || self.usage_fields as usize > 4 * item_count
            || (runtime && self.usage_fields != 0)
            || (!runtime
                && [
                    self.known_records,
                    self.private_records,
                    self.pending_inputs,
                    self.whitespace_rows,
                ]
                .iter()
                .any(|v| *v != 0))
        {
            return Err(invalid("chat omissions"));
        }
        Ok(())
    }
}
fn metadata(
    mut c: wire::Conversation,
    items: Vec<PreparedChatItem>,
) -> Result<PreparedChatConversation> {
    if c.items.is_some() || items.len() > 20_000 {
        return Err(invalid("conversation metadata shape"));
    }
    let store = canonical_uuid(required(c.source_store_id.as_ref())?)?;
    let archive_id = archive_id(&store, &c.source_session_id)?;
    let project = c
        .project_registry_id
        .as_ref()
        .map(|id| uuid(id))
        .transpose()?;
    if let Some(project) = &project {
        c.project_registry_id = Some(super::wire::Text(project.clone()));
    }
    one_of(
        &c.source_state,
        &[
            "active",
            "pinnedOpen",
            "warmDormant",
            "coldDormant",
            "closed",
        ],
        "chat state",
    )?;
    let runtime = match &*c.origin.kind {
        "legacy" if c.origin.runtime_session_id.is_none() && c.origin.last_sequence.is_none() => {
            None
        }
        "runtime" if required(c.origin.last_sequence)? >= 0 => {
            Some(runtime_id(required(c.origin.runtime_session_id.as_ref())?)?)
        }
        _ => return Err(invalid("chat origin")),
    };
    c.omissions.validate(items.len(), runtime.is_some())?;
    let mut previous_legacy: Option<(f64, i64)> = None;
    let mut previous_runtime = 0;
    let mut references = 0usize;
    let mut ids = BTreeSet::new();
    let mut runs = BTreeMap::new();
    let mut branch = None;
    for (ordinal, item) in items.iter().enumerate() {
        if item.ordinal as usize != ordinal
            || item.archive_id != archive_id
            || !ids.insert(&item.item_id)
        {
            return Err(invalid("chat item parent/order"));
        }
        references += item.attachment_ids.len();
        match (&item.source, runtime.as_ref()) {
            (
                PreparedChatItemSource::Legacy {
                    created_at,
                    sequence,
                    ..
                },
                None,
            ) => {
                let n = f64::from_bits(
                    u64::from_str_radix(&created_at.reference_seconds_bits, 16)
                        .map_err(|_| invalid("chat date"))?,
                );
                if previous_legacy.is_some_and(|(p, s)| n < p || (n == p && *sequence <= s)) {
                    return Err(invalid("legacy item order"));
                }
                previous_legacy = Some((n, *sequence));
            }
            (
                PreparedChatItemSource::Runtime {
                    session_sequence,
                    run_id,
                    turn_id,
                    branch_id,
                    ..
                },
                Some(_),
            ) => {
                if *session_sequence <= previous_runtime
                    || *session_sequence > required(c.origin.last_sequence)?
                {
                    return Err(invalid("runtime item order"));
                }
                previous_runtime = *session_sequence;
                if branch.is_some_and(|b| b != branch_id)
                    || runs.insert(run_id, turn_id).is_some_and(|t| t != turn_id)
                {
                    return Err(invalid("runtime lineage"));
                }
                branch = Some(branch_id);
            }
            _ => return Err(invalid("chat source/origin")),
        }
    }
    if references != c.omissions.attachment_references as usize {
        return Err(invalid("chat reference omissions"));
    }
    let fingerprint = hash(&json!([
        "shelby-swift-chat-conversation-v1",
        value(&c)?,
        items
            .iter()
            .map(|i| (&i.item_id, &i.fingerprint))
            .collect::<Vec<_>>()
    ]))?;
    Ok(PreparedChatConversation {
        archive_id,
        source_store_id: store,
        source_session_id: c.source_session_id.0.clone(),
        title: c.title.0.clone(),
        project_id: project,
        runtime_session_id: runtime,
        last_sequence: c.origin.last_sequence,
        created_at: chat_date(&c.created_at)?,
        last_active_at: chat_date(&c.last_active_at)?,
        pinned_at: c.pinned_at.as_ref().map(chat_date).transpose()?,
        closed_at: c.closed_at.as_ref().map(chat_date).transpose()?,
        hidden_at: c.hidden_at.as_ref().map(chat_date).transpose()?,
        omissions: c.omissions.clone(),
        fingerprint,
        source_payload: encoded(&c)?,
        items,
    })
}
/// Recompute metadata plus ordered-child fingerprint; no project/import authority is granted.
pub fn inspect_chat_conversation_source(
    payload: &str,
    items: Vec<PreparedChatItem>,
) -> Result<PreparedChatConversation> {
    if payload.len() > 64 * 1024 {
        return Err(invalid("chat metadata size"));
    }
    let c = serde_json::from_str(payload).map_err(|_| invalid("chat metadata shape"))?;
    metadata(c, items)
}

fn image_metadata(i: &wire::Image) -> Result<()> {
    canonical_uuid(&i.source_attachment_id)?;
    for e in [&i.source_data, &i.source_thumbnail] {
        hex(&e.sha256, 64)?;
    }
    if !(1..=10 * 1_048_576).contains(&i.source_data.byte_count)
        || i.source_thumbnail.byte_count > 512 * 1_048_576
        || (i.source_thumbnail.byte_count == 0
            && *i.source_thumbnail.sha256 != format!("{:x}", Sha256::digest([])))
    {
        return Err(invalid("image source evidence"));
    }
    for (b, edge) in [(&i.jpeg, 1568), (&i.preview, 160)] {
        hex(&b.sha256, 64)?;
        if &*b.media_type != "image/jpeg"
            || !(1..=524_288).contains(&b.byte_count)
            || !(1..=edge).contains(&b.width)
            || !(1..=edge).contains(&b.height)
        {
            return Err(invalid("image metadata"));
        }
    }
    if i.preview.width > i.jpeg.width
        || i.preview.height > i.jpeg.height
        || i.jpeg.byte_count + i.preview.byte_count > 1_048_576
    {
        return Err(invalid("image preview bounds"));
    }
    Ok(())
}
fn image_bytes(metadata: &wire::ImageBytes, bytes: &[u8]) -> Result<()> {
    if bytes.len() != metadata.byte_count as usize
        || format!("{:x}", Sha256::digest(bytes)) != *metadata.sha256
        || !bytes.starts_with(&[0xff, 0xd8])
        || !bytes.ends_with(&[0xff, 0xd9])
    {
        return Err(invalid("image bytes"));
    }
    Ok(())
}
fn project_image(i: wire::Image, jpeg: Vec<u8>, preview: Vec<u8>) -> Result<PreparedChatImage> {
    image_metadata(&i)?;
    if i.jpeg.data_base64.is_some() || i.preview.data_base64.is_some() {
        return Err(invalid("image stored metadata shape"));
    }
    image_bytes(&i.jpeg, &jpeg)?;
    image_bytes(&i.preview, &preview)?;
    Ok(PreparedChatImage {
        source_attachment_id: i.source_attachment_id.0.clone(),
        fingerprint: hash(&json!(["shelby-swift-chat-image-v1", 1, value(&i)?]))?,
        source_payload: encoded(&i)?,
        width: i.jpeg.width,
        height: i.jpeg.height,
        preview_width: i.preview.width,
        preview_height: i.preview.height,
        jpeg,
        preview,
    })
}
/// Structural framing/digest checks only. The native candidate must independently decode both JPEGs.
/// Takes owned BLOBs so stored-row inspection does not retain another payload copy.
pub fn inspect_chat_image_source(
    payload: &str,
    jpeg: Vec<u8>,
    preview: Vec<u8>,
) -> Result<PreparedChatImage> {
    if payload.len() > 32 * 1024 || jpeg.len() > 524_288 || preview.len() > 524_288 {
        return Err(invalid("image source size"));
    }
    let i = serde_json::from_str(payload).map_err(|_| invalid("image source shape"))?;
    project_image(i, jpeg, preview)
}
fn decode_image(mut i: wire::Image) -> Result<PreparedChatImage> {
    image_metadata(&i)?;
    fn decode(b: &mut wire::ImageBytes) -> Result<Vec<u8>> {
        let encoded = required(b.data_base64.take())?;
        if encoded.len() != 4 * ((b.byte_count as usize).div_ceil(3)) {
            return Err(invalid("image base64 size"));
        }
        let bytes = STANDARD
            .decode(encoded.as_bytes())
            .map_err(|_| invalid("image base64"))?;
        if STANDARD.encode(&bytes) != *encoded {
            return Err(invalid("image base64 spelling"));
        }
        Ok(bytes)
    }
    let jpeg = decode(&mut i.jpeg)?;
    let preview = decode(&mut i.preview)?;
    project_image(i, jpeg, preview)
}

/// Validate the complete intrinsic archive row set. Image row order is irrelevant in SQLite;
/// wire preparation separately checks first-reference order. No project/import authority is granted.
pub fn validate_chat_archive_rows(
    conversations: &[PreparedChatConversation],
    images: &[PreparedChatImage],
) -> Result<()> {
    if conversations.len() > 100 || images.len() > 2048 {
        return Err(invalid("chat row limits"));
    }
    let mut stores = BTreeSet::new();
    let mut sessions = BTreeSet::new();
    let mut runtime_sessions = BTreeSet::new();
    let mut archive_ids = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut source_items = BTreeSet::new();
    let mut refs = BTreeSet::new();
    let mut totals = [0u64; 10];
    let mut count = 0usize;
    for c in conversations {
        stores.insert(&c.source_store_id);
        if !sessions.insert(&c.source_session_id)
            || !archive_ids.insert(&c.archive_id)
            || c.runtime_session_id
                .as_ref()
                .is_some_and(|id| !runtime_sessions.insert(id))
        {
            return Err(invalid("chat duplicate conversation"));
        }
        count += c.items.len();
        for (n, v) in c.omissions.values().iter().enumerate() {
            totals[n] += u64::from(*v);
        }
        for item in &c.items {
            let key = match &item.source {
                PreparedChatItemSource::Legacy { message_uuid, .. } => ("legacy", message_uuid),
                PreparedChatItemSource::Runtime {
                    event_id,
                    item_id,
                    tool_invocation_id,
                    ..
                } => {
                    let source = item_id
                        .as_ref()
                        .or(tool_invocation_id.as_ref())
                        .ok_or_else(|| invalid("runtime identity"))?;
                    if !source_items.insert(source) {
                        return Err(invalid("runtime duplicate item"));
                    }
                    ("runtime", event_id)
                }
            };
            if !identities.insert(key) {
                return Err(invalid("chat duplicate source item"));
            }
            refs.extend(item.attachment_ids.iter());
        }
    }
    if stores.len() > 1
        || count > 20_000
        || totals
            .iter()
            .zip([
                70_000, 70_000, 160_000, 80_000, 300, 2_661_753, 50_000, 50_000, 50_000, 50_000,
            ])
            .any(|(a, b)| *a > b)
    {
        return Err(invalid("chat aggregate counts"));
    }
    let image_ids: BTreeSet<_> = images.iter().map(|i| &i.source_attachment_id).collect();
    if image_ids.len() != images.len() || refs != image_ids {
        return Err(invalid("chat image references"));
    }
    Ok(())
}

pub(super) fn prepare(
    package: &mut super::wire::Package,
    projects: &BTreeMap<String, String>,
) -> Result<Option<PreparedChatArchive>> {
    let category = &package.categories.conversations;
    if package.version != 3 {
        if package.chat_archive.is_some()
            || category.count != 0
            || &*category.status != "notSelected"
            || category.catalog_count.is_some()
            || category.unselected_count.is_some()
            || category.item_count.is_some()
        {
            return Err(invalid("old chat fields"));
        }
        return Ok(None);
    }
    let archive = required(package.chat_archive.take())?;
    let store = canonical_uuid(&archive.source_store_id)?;
    let catalog = required(category.catalog_count)?;
    let unselected = required(category.unselected_count)?;
    let item_count = required(category.item_count)?;
    if category.count > 100
        || catalog > 1000
        || unselected > 1000
        || category.count + unselected != catalog
        || item_count > 20_000
        || &*category.status != "selectedSubset"
        || category.count as usize != archive.conversations.len()
        || archive.projection_version != 1
    {
        return Err(invalid("chat selection"));
    }
    let p = &archive.policy;
    if &*p.content != "visibleTextAndInertAnnotations"
        || &*p.execution != "neverReplay"
        || &*p.continuation != "newChatOnly"
        || &*p.reasoning != "omitted"
        || &*p.tool_details != "omitted"
        || &*p.source_versions != "swift9322-projection1"
        || &*p.attachments != "normalizedJPEGWithRegeneratedPreview"
        || p.image_policy_version != 1
    {
        return Err(invalid("chat policy"));
    }
    let policy = PreparedChatPolicy {
        content: p.content.0.clone(),
        execution: p.execution.0.clone(),
        continuation: p.continuation.0.clone(),
        reasoning: p.reasoning.0.clone(),
        tool_details: p.tool_details.0.clone(),
        source_versions: p.source_versions.0.clone(),
        attachments: p.attachments.0.clone(),
        image_policy_version: 1,
    };
    let mut conversations = Vec::new();
    let mut refs = Vec::new();
    let mut seen = BTreeSet::new();
    let mut count = 0usize;
    for mut c in archive.conversations.0 {
        if c.source_store_id.is_some() {
            return Err(invalid("wire conversation shape"));
        }
        let id = archive_id(&store, &c.source_session_id)?;
        let items = required(c.items.take())?;
        count += items.len();
        if count > 20_000 {
            return Err(invalid("chat item count"));
        }
        let items = items
            .0
            .into_iter()
            .map(|i| project_item(&id, i))
            .collect::<Result<Vec<_>>>()?;
        c.source_store_id = Some(super::wire::Text(store.clone()));
        let projected = metadata(c, items)?;
        if projected
            .project_id
            .as_ref()
            .is_some_and(|id| !projects.contains_key(id))
        {
            return Err(invalid("chat project ownership"));
        }
        for item in &projected.items {
            for id in &item.attachment_ids {
                if seen.insert(id.clone()) {
                    refs.push(id.clone());
                }
            }
        }
        conversations.push(projected);
    }
    if count != item_count as usize
        || archive.image_disclosure.normalized_images as usize != archive.images.len()
        || archive.image_disclosure.regenerated_previews as usize != archive.images.len()
        || refs
            != archive
                .images
                .iter()
                .map(|i| i.source_attachment_id.0.clone())
                .collect::<Vec<_>>()
    {
        return Err(invalid("chat image order/disclosure"));
    }
    let images = archive
        .images
        .0
        .into_iter()
        .map(decode_image)
        .collect::<Result<Vec<_>>>()?;
    validate_chat_archive_rows(&conversations, &images)?;
    Ok(Some(PreparedChatArchive {
        source_store_id: store,
        selection: PreparedChatSelection {
            count: category.count,
            catalog_count: catalog,
            unselected_count: unselected,
            item_count,
        },
        policy,
        conversations,
        images,
        image_disclosure: archive.image_disclosure,
    }))
}

impl PreparedChatArchive {
    pub(super) fn binding(&self) -> Value {
        let conversations: BTreeMap<_, _> = self
            .conversations
            .iter()
            .map(|c| (&c.archive_id, &c.fingerprint))
            .collect();
        let images: BTreeMap<_, _> = self
            .images
            .iter()
            .map(|i| (&i.source_attachment_id, &i.fingerprint))
            .collect();
        json!([
            self.source_store_id,
            self.selection,
            self.policy,
            conversations.into_iter().collect::<Vec<_>>(),
            images.into_iter().collect::<Vec<_>>(),
            self.image_disclosure
        ])
    }
    pub(super) fn manifest(&self) -> Value {
        json!({"projectionVersion":1,"sourceStoreID":self.source_store_id,"selection":self.selection,"policy":self.policy,"imageDisclosure":self.image_disclosure})
    }
    pub(super) fn prepared_bytes(&self) -> Result<usize> {
        let mut bytes = encoded(&self.manifest())?.len();
        for c in &self.conversations {
            bytes += c.source_payload.len() + c.archive_id.len() + c.fingerprint.len() + 64; // Primitive row/derived-time overhead.
            for i in &c.items {
                bytes += i.source_payload.len()
                    + i.item_id.len()
                    + i.archive_id.len()
                    + i.fingerprint.len()
                    + 8;
            }
        }
        for i in &self.images {
            bytes += i.source_payload.len()
                + i.source_attachment_id.len()
                + i.fingerprint.len()
                + i.jpeg.len()
                + i.preview.len()
                + 64; // Stored batch fingerprint.
        }
        if bytes > MAX_BYTES {
            return Err(invalid("chat prepared size"));
        }
        Ok(bytes)
    }
}

/// Count normalized JSON using Swift's default slash escaping, without materializing it.
pub(super) fn source_size(package: &super::wire::Package) -> Result<()> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len() + bytes.iter().filter(|b| **b == b'/').count();
            if self.0 > MAX_BYTES {
                return Err(std::io::Error::other("size"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(0), package).map_err(|_| invalid("v3 normalized source size"))
}

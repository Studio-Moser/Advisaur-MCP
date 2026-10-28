//! Direct bounded structs avoid internally tagged enum buffering before field bounds.
use super::super::wire::{List, Text, nullable, present};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Archive {
    pub projection_version: u32,
    #[serde(rename = "sourceStoreID")]
    pub source_store_id: Text<36>,
    pub policy: Policy,
    pub conversations: List<Conversation, 100>,
    pub images: List<Image, 2048>,
    pub image_disclosure: super::PreparedImageDisclosure,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Policy {
    pub content: Text<64>,
    pub execution: Text<32>,
    pub continuation: Text<32>,
    pub reasoning: Text<32>,
    pub tool_details: Text<32>,
    pub source_versions: Text<64>,
    pub attachments: Text<64>,
    pub image_policy_version: u32,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Date {
    pub reference_seconds_bits: Text<16>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Conversation {
    // Present only in stored metadata, never in a wire conversation.
    #[serde(
        rename = "sourceStoreID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_store_id: Option<Text<36>>,
    #[serde(rename = "sourceSessionID")]
    pub source_session_id: Text<512>,
    pub title: Text<4096>,
    #[serde(rename = "projectRegistryID", deserialize_with = "nullable")]
    pub project_registry_id: Option<Text<36>>,
    #[serde(rename = "projectEntityID", deserialize_with = "nullable")]
    pub project_entity_id: Option<Text<512>>,
    pub source_state: Text<16>,
    pub created_at: Date,
    pub last_active_at: Date,
    #[serde(deserialize_with = "nullable")]
    pub pinned_at: Option<Date>,
    #[serde(deserialize_with = "nullable")]
    pub closed_at: Option<Date>,
    #[serde(deserialize_with = "nullable")]
    pub hidden_at: Option<Date>,
    pub origin: Origin,
    pub omissions: super::PreparedChatOmissions,
    // Required for wire; absent for stored metadata, so bodies are never duplicated.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub items: Option<List<Item, 20000>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Origin {
    pub kind: Text<16>,
    #[serde(
        rename = "runtimeSessionID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub runtime_session_id: Option<Text<36>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_sequence: Option<i64>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Item {
    pub ordinal: u32,
    pub source: Source,
    pub body: Body,
    pub status: Status,
    pub attachment_count: u32,
    #[serde(rename = "attachmentIDs")]
    pub attachment_ids: List<Text<36>, 2048>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Source {
    pub kind: Text<16>,
    #[serde(
        rename = "messageUUID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub message_uuid: Option<Text<36>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub created_at: Option<Date>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub sequence: Option<i64>,
    #[serde(
        rename = "turnID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub turn_id: Option<Option<Text<512>>>,
    #[serde(
        rename = "eventID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub event_id: Option<Text<36>>,
    #[serde(
        rename = "itemID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub item_id: Option<Option<Text<36>>>,
    #[serde(
        rename = "toolInvocationID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_invocation_id: Option<Option<Text<36>>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub session_sequence: Option<i64>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub timestamp_unix_milliseconds: Option<i64>,
    #[serde(
        rename = "runID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub run_id: Option<Option<Text<36>>>,
    #[serde(
        rename = "branchID",
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub branch_id: Option<Option<Text<36>>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub sensitivity: Option<Text<16>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Body {
    pub kind: Text<16>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub text: Option<Text<1048576>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<Text<4096>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub status: Option<Text<16>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_milliseconds: Option<Option<i64>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::swift_import) struct Status {
    pub kind: Text<16>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<Text<1048576>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Evidence {
    pub byte_count: u64,
    pub sha256: Text<64>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct Image {
    #[serde(rename = "sourceAttachmentID")]
    pub source_attachment_id: Text<36>,
    pub source_filename: Text<4096>,
    pub source_media_type: Text<256>,
    pub source_data: Evidence,
    pub source_thumbnail: Evidence,
    pub jpeg: ImageBytes,
    pub preview: ImageBytes,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::swift_import) struct ImageBytes {
    pub media_type: Text<16>,
    pub byte_count: u32,
    pub sha256: Text<64>,
    pub width: u32,
    pub height: u32,
    // Required for wire, absent from stored image metadata.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub data_base64: Option<Text<699052>>,
}

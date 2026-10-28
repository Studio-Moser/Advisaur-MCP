//! Explicit identity-preserving Swift migration into a private, fresh candidate.
//!
//! This API only consumes JSON bytes. It never opens a Swift database, enables
//! automation, merges authored content, or registers a model-accessible tool.
mod chats;
mod tasks;
mod wire;
use crate::{identity, temporal, topics};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub use tasks::{
    PreparedLocalTask, PreparedTaskExcluded, PreparedTaskOmissions, PreparedTaskPolicy,
    PreparedTaskSelection, inspect_local_task_source,
};

pub use chats::{
    PreparedChatArchive, PreparedChatConversation, PreparedChatDate, PreparedChatImage,
    PreparedChatItem, PreparedChatItemSource, PreparedChatOmissions, PreparedChatPolicy,
    PreparedChatSelection, PreparedImageDisclosure, inspect_chat_conversation_source,
    inspect_chat_image_source, inspect_chat_item_source, validate_chat_archive_rows,
};

const MAX_BYTES: usize = 50 * 1024 * 1024;
const POLICY_VERSION: i64 = 1;
const PRODUCT: &str = "shelby-swift";

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("Unsupported or invalid Swift migration package ({0}); no import was applied")]
    Invalid(&'static str),
    #[error(
        "The target does not match a fresh or unchanged imported profile; no merge was applied"
    )]
    Conflict,
    #[error(
        "The target has existing connected clients or history; this import requires a fresh profile"
    )]
    ExistingHistory(TargetHistory),
    #[error("The candidate database operation failed; no successful import was confirmed")]
    Database,
    #[error(
        "The candidate connection could not be restored to a known state; discard or quarantine it"
    )]
    UnusableConnection,
}
pub type Result<T> = std::result::Result<T, ImportError>;
/// Aggregate closed reasons only; never client IDs, queries, or feedback contents.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetHistory {
    pub connected_clients: bool,
    pub feedback: bool,
    pub search_history: bool,
}
fn invalid(code: &'static str) -> ImportError {
    ImportError::Invalid(code)
}
impl From<rusqlite::Error> for ImportError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Database
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub project_count: usize,
    pub alias_count: usize,
    pub memory_count: usize,
    pub edge_count: usize,
    pub omission_count: u64,
    pub null_type_count: usize,
    pub null_source_count: usize,
    pub normalized_topics_count: usize,
    pub source_app_version: Option<String>,
    pub adapter_reference: String,
    pub export_id: String,
    pub exported_at: String,
    pub selected_scopes: Vec<String>,
}
/// A read-only projection for the App's separate candidate curation transaction.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedProject {
    pub project_id: String,
    pub display_name: String,
    pub current_slug: String,
    pub display_name_override: Option<String>,
    pub pinned: bool,
    pub archived: bool,
    pub updated_at: String,
}
#[derive(Debug)]
pub struct PreparedImport {
    wire_version: u32,
    chat_archive: Option<PreparedChatArchive>,
    local_tasks: Vec<PreparedLocalTask>,
    task_selection: Option<PreparedTaskSelection>,
    preview: ImportPreview,
    projects: Vec<PreparedProject>,
    entities: Vec<Entity>,
    batch_fingerprint: String,
    manifest: String,
}
impl PreparedImport {
    pub fn chat_archive(&self) -> Option<&PreparedChatArchive> {
        self.chat_archive.as_ref()
    }
    pub fn wire_version(&self) -> u32 {
        self.wire_version
    }
    pub fn batch_fingerprint(&self) -> &str {
        &self.batch_fingerprint
    }
    pub fn local_tasks(&self) -> &[PreparedLocalTask] {
        &self.local_tasks
    }
    pub fn task_selection(&self) -> Option<&PreparedTaskSelection> {
        self.task_selection.as_ref()
    }

    pub fn preview(&self) -> &ImportPreview {
        &self.preview
    }
    pub fn projects(&self) -> &[PreparedProject] {
        &self.projects
    }
}
#[derive(Debug)]
struct Entity {
    kind: &'static str,
    id: String,
    target_id: String,
    fingerprint: String,
    source_payload: String,
    target: Target,
}
#[derive(Debug)]
struct Target {
    table: &'static str,
    columns: &'static [&'static str],
    values: Vec<Value>,
}
const PROJECT_COLUMNS: &[&str] = &[
    "slug",
    "display_name",
    "member_repos",
    "member_paths",
    "provisional",
    "created_at",
    "updated_at",
    "project_id",
    "current_slug",
    "identity_state",
];
const ALIAS_COLUMNS: &[&str] = &["slug", "project_id", "status", "claimed_at", "retired_at"];
const THOUGHT_COLUMNS: &[&str] = &[
    "id",
    "content",
    "summary",
    "type",
    "source",
    "project",
    "topics",
    "people",
    "visibility",
    "metadata",
    "embedding",
    "created_at",
    "updated_at",
    "consolidated_into",
    "reinforcement_count",
    "project_identifier",
    "project_id",
    "source_agent",
    "trust_level",
    "last_confirmed_at",
];
const EDGE_COLUMNS: &[&str] = &[
    "id",
    "source_id",
    "target_id",
    "edge_type",
    "metadata",
    "created_at",
    "valid_from",
    "valid_until",
];

fn uuid(value: &str) -> Result<String> {
    let normalized = value.to_ascii_lowercase();
    if !identity::is_canonical_project_id(&normalized) {
        return Err(invalid("UUID"));
    }
    Ok(normalized)
}
fn slug(value: &str) -> Result<()> {
    if identity::is_valid_project_slug(value) {
        Ok(())
    } else {
        Err(invalid("target alias"))
    }
}
fn date(value: &str) -> Result<()> {
    temporal::parse_bound(value)
        .map(|_| ())
        .map_err(|_| invalid("timestamp"))
}
fn optional_date(value: Option<&wire::Date>) -> Result<()> {
    if let Some(value) = value {
        date(value)?;
    }
    Ok(())
}
fn one_of(value: &str, allowed: &[&str], code: &'static str) -> Result<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(invalid(code))
    }
}
fn strings(value: Option<&wire::Labels>) -> Vec<String> {
    value
        .map(|v| v.iter().map(|x| x.0.clone()).collect())
        .unwrap_or_default()
}
fn encoded<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| invalid("serialization"))
}
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), sorted(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        _ => value.clone(),
    }
}
fn hash(value: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(encoded(&sorted(value))?.as_bytes())
    ))
}
fn entity<T: Serialize>(
    kind: &'static str,
    id: String,
    target_id: String,
    source: &T,
    target: Target,
) -> Result<Entity> {
    let fingerprint_value = serde_json::to_value(source).map_err(|_| invalid("serialization"))?;
    let fingerprint = source_hash(kind, fingerprint_value)?;
    Ok(Entity {
        kind,
        id,
        target_id,
        fingerprint,
        source_payload: encoded(source)?,
        target,
    })
}
fn source_hash(kind: &str, mut fingerprint_value: Value) -> Result<String> {
    // UUID spelling is equivalent for identity and references. Other source text,
    // array order, and null/empty distinctions are deliberately fingerprinted.
    for field in [
        "sourceId",
        "projectId",
        "sourceMemoryId",
        "targetMemoryId",
        "consolidatedInto",
        "legacySlug",
    ] {
        if kind == "edge" && field == "sourceId" {
            continue;
        }
        if let Some(Value::String(text)) = fingerprint_value.get_mut(field)
            && let Ok(normalized) = uuid(text)
        {
            *text = normalized;
        }
    }
    hash(&json!([PRODUCT, kind, POLICY_VERSION, fingerprint_value]))
}
fn target(table: &'static str, columns: &'static [&'static str], values: Value) -> Target {
    Target {
        table,
        columns,
        values: values.as_array().expect("constant array").clone(),
    }
}

pub fn prepare(bytes: &[u8]) -> Result<PreparedImport> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("package size"));
    }
    let mut package: wire::Package =
        serde_json::from_slice(bytes).map_err(|_| invalid("JSON shape or bounds"))?;
    if &*package.format != "shelby-swift-migration"
        || !matches!(package.version, 1..=3)
        || &*package.source_product != PRODUCT
        || package.source_schema != 18
    {
        return Err(invalid("format version"));
    }
    uuid(&package.export_id)?;
    if package.version == 3 {
        chats::source_size(&package)?;
    }
    date(&package.exported_at)?;
    if package.adapter_reference.is_empty() {
        return Err(invalid("adapter reference"));
    }
    let p = &package.policy;
    if &*p.automatic_eligibility != "disabledOnImport" || &*p.trust != "unverifiedExceptExternal" {
        return Err(invalid("policy"));
    }
    let omissions: BTreeSet<&str> = p.omitted_source_fields.iter().map(|s| &**s).collect();
    if p.omitted_source_fields.len() != 5
        || omissions
            != BTreeSet::from([
                "embedding",
                "accountAndHostIdentity",
                "syncState",
                "projectPathsAndRepositories",
                "importAuthority",
            ])
    {
        return Err(invalid("omission policy"));
    }
    let mut scopes = BTreeSet::new();
    for scope in package.selected_scopes.iter() {
        let s = if scope.starts_with("unscoped:") {
            one_of(scope, &["unscoped:personal", "unscoped:shared"], "scope")?;
            scope.0.clone()
        } else {
            uuid(scope)?
        };
        if !scopes.insert(s) {
            return Err(invalid("duplicate scope"));
        }
    }
    if scopes.is_empty() {
        return Err(invalid("selection"));
    }
    let mut preview = ImportPreview {
        project_count: package.projects.len(),
        alias_count: package.aliases.len(),
        memory_count: package.memories.len(),
        edge_count: package.edges.len(),
        omission_count: 0,
        null_type_count: 0,
        null_source_count: 0,
        normalized_topics_count: 0,
        source_app_version: package.source_app_version.as_ref().map(|s| s.0.clone()),
        adapter_reference: package.adapter_reference.0.clone(),
        export_id: package.export_id.0.clone(),
        exported_at: package.exported_at.0.clone(),
        selected_scopes: scopes.iter().cloned().collect(),
    };
    let mut entities = Vec::new();
    let mut projects = Vec::new();
    let mut project_slugs = BTreeMap::new();
    let mut project_keys = BTreeSet::new();
    let mut current_slugs = BTreeSet::new();
    for p in package.projects.iter() {
        let id = uuid(&p.source_id)?;
        slug(&p.current_slug)?;
        let key = if let Ok(key) = uuid(&p.legacy_slug) {
            if key != id {
                return Err(invalid("frozen project identity"));
            }
            key
        } else {
            if identity::derive_existing_project_id(&p.legacy_slug).as_ref() != Some(&id) {
                return Err(invalid("frozen project identity"));
            }
            p.legacy_slug.0.clone()
        };
        if !scopes.contains(&id)
            || project_slugs
                .insert(id.clone(), p.current_slug.0.clone())
                .is_some()
            || !project_keys.insert(key.clone())
            || !current_slugs.insert(p.current_slug.0.clone())
        {
            return Err(invalid("project identity"));
        }
        one_of(
            &p.source_identity_state,
            &["local_only", "pending", "active", "collision"],
            "source identity state",
        )?;
        date(&p.created_at)?;
        date(&p.updated_at)?;
        projects.push(PreparedProject {
            project_id: id.clone(),
            display_name: p.display_name.0.clone(),
            current_slug: p.current_slug.0.clone(),
            display_name_override: p.display_name_override.as_ref().map(|v| v.0.clone()),
            pinned: p.pinned,
            archived: p.archived,
            updated_at: p.updated_at.0.clone(),
        });
        entities.push(entity(
            "project",
            id.clone(),
            id.clone(),
            p,
            target(
                "projects",
                PROJECT_COLUMNS,
                json!([
                    key,
                    p.display_name,
                    "[]",
                    "[]",
                    i64::from(p.provisional),
                    p.created_at,
                    p.updated_at,
                    id,
                    p.current_slug,
                    "local_only"
                ]),
            ),
        )?);
    }
    if scopes
        .iter()
        .filter(|s| !s.starts_with("unscoped:"))
        .any(|id| !project_slugs.contains_key(id))
    {
        return Err(invalid("selected project"));
    }
    let mut alias_owners = BTreeMap::new();
    for a in package.aliases.iter() {
        slug(&a.slug)?;
        let owner = uuid(&a.project_id)?;
        if !project_slugs.contains_key(&owner)
            || alias_owners
                .insert(a.slug.0.clone(), (owner.clone(), a.status.0.clone()))
                .is_some()
        {
            return Err(invalid("alias ownership"));
        }
        one_of(
            &a.status,
            &["tentative", "current", "retired"],
            "alias status",
        )?;
        date(&a.claimed_at)?;
        optional_date(a.retired_at.as_ref())?;
        if (&*a.status == "retired") != a.retired_at.is_some()
            || (&*a.status == "current" && project_slugs.get(&owner) != Some(&a.slug.0))
        {
            return Err(invalid("alias state"));
        }
        entities.push(entity(
            "alias",
            a.slug.0.clone(),
            a.slug.0.clone(),
            a,
            target(
                "project_slug_aliases",
                ALIAS_COLUMNS,
                json!([
                    a.slug,
                    owner,
                    if &*a.status == "retired" {
                        "retired"
                    } else {
                        "tentative"
                    },
                    a.claimed_at,
                    a.retired_at
                ]),
            ),
        )?);
    }
    for (id, slug) in &project_slugs {
        if !matches!(alias_owners.get(slug),Some((owner,status)) if owner==id && status!="retired")
        {
            return Err(invalid("current alias"));
        }
    }
    for project in package.projects.iter() {
        if uuid(&project.legacy_slug).is_err() {
            let id = uuid(&project.source_id)?;
            if !matches!(alias_owners.get(&project.legacy_slug.0),Some((owner,_)) if owner==&id) {
                return Err(invalid("frozen legacy alias ownership"));
            }
        }
    }
    let resolve_scope =
        |project: Option<&wire::Id>, alias: Option<&wire::Id>| -> Result<Option<String>> {
            match project {
                Some(value) => {
                    let id = uuid(value)?;
                    if !project_slugs.contains_key(&id)
                        || alias.is_some_and(
                            |a| !matches!(alias_owners.get(&a.0),Some((owner,_)) if owner==&id),
                        )
                    {
                        return Err(invalid("project scope"));
                    }
                    Ok(Some(id))
                }
                None if alias.is_none() => Ok(None),
                _ => Err(invalid("unresolved alias")),
            }
        };
    let mut memory_scopes = BTreeMap::new();
    let mut consolidations = BTreeMap::new();
    for t in package.memories.iter() {
        let id = uuid(&t.source_id)?;
        let project = resolve_scope(t.project_id.as_ref(), t.project_alias.as_ref())?;
        one_of(&t.visibility, &["personal", "shared"], "visibility")?;
        let scope = project
            .clone()
            .unwrap_or_else(|| format!("unscoped:{}", t.visibility.0));
        if !scopes.contains(&scope) || memory_scopes.insert(id.clone(), scope).is_some() {
            return Err(invalid("memory identity or selection"));
        }
        one_of(
            &t.source_trust,
            &["trusted", "unverified", "external"],
            "source trust",
        )?;
        date(&t.created_at)?;
        date(&t.updated_at)?;
        optional_date(t.last_confirmed_at.as_ref())?;
        if t.reinforcement_count > i64::MAX as u64 {
            return Err(invalid("reinforcement count"));
        }
        let consolidated = t.consolidated_into.as_ref().map(|s| uuid(s)).transpose()?;
        if let Some(target) = &consolidated {
            consolidations.insert(id.clone(), target.clone());
        }
        if encoded(&t.metadata)?.len() > 256 * 1024 {
            return Err(invalid("metadata size"));
        }
        if let Some(kept) = &t.metadata.extra.contradiction_kept {
            uuid(kept)?;
        }
        for count in [
            t.omissions.unknown,
            t.omissions.authority,
            t.omissions.unsupported,
        ]
        .into_iter()
        .flatten()
        {
            if count == 0 || count > 256 {
                return Err(invalid("omission count"));
            }
            preview.omission_count += u64::from(count);
        }
        let source_topics = strings(t.metadata.topics.as_ref());
        let normalized_topics = topics::canonicalize_topics(&source_topics);
        preview.normalized_topics_count += usize::from(source_topics != normalized_topics);
        preview.null_type_count += usize::from(t.metadata.r#type.is_none());
        preview.null_source_count += usize::from(t.source.is_none());
        let mut extra = serde_json::to_value(&t.metadata.extra).map_err(|_| invalid("metadata"))?;
        extra["briefEligible"] = Value::Bool(false);
        let metadata =
            json!({"actionItems":t.metadata.action_items,"dates":t.metadata.dates,"extra":extra});
        let current_slug = project.as_ref().and_then(|id| project_slugs.get(id));
        entities.push(entity(
            "memory",
            id.clone(),
            id.clone(),
            t,
            target(
                "thoughts",
                THOUGHT_COLUMNS,
                json!([
                    id,
                    t.content,
                    t.summary,
                    t.metadata.r#type.as_deref().unwrap_or("note"),
                    t.source.as_deref().unwrap_or("unknown"),
                    null,
                    encoded(&normalized_topics)?,
                    encoded(&strings(t.metadata.people.as_ref()))?,
                    t.visibility,
                    encoded(&metadata)?,
                    null,
                    t.created_at,
                    t.updated_at,
                    consolidated,
                    t.reinforcement_count,
                    current_slug,
                    project,
                    t.source_agent,
                    if &*t.source_trust == "external" {
                        "external"
                    } else {
                        "unverified"
                    },
                    t.last_confirmed_at
                ]),
            ),
        )?);
    }
    for start in consolidations.keys() {
        let mut seen = BTreeSet::from([start.as_str()]);
        let mut cursor = start;
        while let Some(next) = consolidations.get(cursor) {
            if !memory_scopes.contains_key(next) || !seen.insert(next.as_str()) {
                return Err(invalid("consolidation closure"));
            }
            cursor = next;
        }
    }
    let mut edge_ids = BTreeSet::new();
    let mut edge_tuples = BTreeSet::new();
    for e in package.edges.iter() {
        let from = uuid(&e.source_memory_id)?;
        let to = uuid(&e.target_memory_id)?;
        if e.source_id.is_empty()
            || !edge_ids.insert(e.source_id.0.clone())
            || !edge_tuples.insert((from.clone(), to.clone(), e.r#type.0.clone()))
            || !memory_scopes.contains_key(&from)
            || !memory_scopes.contains_key(&to)
        {
            return Err(invalid("edge identity or closure"));
        }
        if !crate::edges::is_valid_edge_type(&e.r#type)
            || !e.source_weight.is_finite()
            || !(0.0..=10.0).contains(&e.source_weight)
        {
            return Err(invalid("edge type or weight"));
        }
        let project = resolve_scope(e.project_id.as_ref(), e.project_alias.as_ref())?;
        if let Some(id) = project
            && (memory_scopes.get(&from) != Some(&id) || memory_scopes.get(&to) != Some(&id))
        {
            return Err(invalid("edge scope"));
        }
        date(&e.created_at)?;
        date(&e.updated_at)?;
        optional_date(e.valid_from.as_ref())?;
        optional_date(e.valid_until.as_ref())?;
        entities.push(entity(
            "edge",
            e.source_id.0.clone(),
            e.source_id.0.clone(),
            e,
            target(
                "edges",
                EDGE_COLUMNS,
                json!([
                    e.source_id,
                    from,
                    to,
                    e.r#type,
                    encoded(&e.metadata)?,
                    e.created_at,
                    e.valid_from,
                    e.valid_until
                ]),
            ),
        )?);
    }
    let categories = &package.categories;
    for (category, count, status) in [
        (&categories.projects, package.projects.len(), "complete"),
        (
            &categories.memories,
            package.memories.len(),
            if preview.omission_count > 0 {
                "completeWithAcknowledgedOmissions"
            } else {
                "complete"
            },
        ),
        (&categories.edges, package.edges.len(), "complete"),
    ] {
        if category.count as usize != count || &*category.status != status {
            return Err(invalid("category completeness"));
        }
    }
    let (local_tasks, task_selection) = tasks::prepare(&package, &project_slugs, &alias_owners)?;
    let chat_archive = chats::prepare(&mut package, &project_slugs)?;
    // Two separately bounded projections are retained: source provenance and
    // canonical rows. Each stays <=50 MiB; wire parsing also has a 50 MiB ceiling.
    let mut source_bytes = 0usize;
    let mut target_bytes = 0usize;
    for e in &entities {
        source_bytes += e.source_payload.len();
        target_bytes += encoded(&e.target.values)?.len();
        if source_bytes > MAX_BYTES || target_bytes > MAX_BYTES {
            return Err(invalid("prepared size"));
        }
    }
    for task in &local_tasks {
        source_bytes += task.source_payload.len();
        target_bytes += encoded(task)?.len();
        if source_bytes > MAX_BYTES || target_bytes > MAX_BYTES {
            return Err(invalid("prepared size"));
        }
    }
    if let Some(archive) = &chat_archive {
        let bytes = archive.prepared_bytes()?;
        if source_bytes + bytes > MAX_BYTES || target_bytes + bytes > MAX_BYTES {
            return Err(invalid("prepared size"));
        }
    }
    let fingerprints: BTreeMap<_, _> = entities
        .iter()
        .map(|e| ((e.kind, e.id.as_str()), e.fingerprint.as_str()))
        .collect();
    let fingerprints = fingerprints
        .into_iter()
        .map(|((kind, id), fingerprint)| json!([kind, id, fingerprint]))
        .collect::<Vec<_>>();
    let batch_fingerprint = if package.version == 1 {
        hash(&json!([PRODUCT, POLICY_VERSION, fingerprints]))?
    } else if package.version == 2 {
        let task_fingerprints: BTreeMap<_, _> = local_tasks
            .iter()
            .map(|t| (&t.source_key, &t.fingerprint))
            .collect();
        hash(&json!([
            "shelby-swift-batch-v2",
            PRODUCT,
            POLICY_VERSION,
            fingerprints,
            task_fingerprints.into_iter().collect::<Vec<_>>(),
            task_selection
        ]))?
    } else {
        let task_fingerprints: BTreeMap<_, _> = local_tasks
            .iter()
            .map(|t| (&t.source_key, &t.fingerprint))
            .collect();
        hash(&json!([
            "shelby-swift-batch-v3",
            PRODUCT,
            POLICY_VERSION,
            fingerprints,
            task_fingerprints.into_iter().collect::<Vec<_>>(),
            task_selection,
            chat_archive
                .as_ref()
                .ok_or_else(|| invalid("chat archive"))?
                .binding()
        ]))?
    };
    let mut manifest = json!({"format":package.format,"version":package.version,"sourceProduct":package.source_product,"sourceSchema":package.source_schema,"sourceAppVersion":package.source_app_version,"adapterReference":package.adapter_reference,"exportId":package.export_id,"exportedAt":package.exported_at,"selectedScopes":package.selected_scopes,"categories":package.categories,"policy":package.policy});
    if let Some(selection) = &task_selection {
        manifest["taskPolicy"] =
            serde_json::to_value(&selection.task_policy).map_err(|_| invalid("serialization"))?;
    }
    if let Some(archive) = &chat_archive {
        manifest["chatArchive"] = archive.manifest();
    }
    let manifest = encoded(&manifest)?;
    if manifest.len() > 32 * 1024 {
        return Err(invalid("manifest size"));
    }
    Ok(PreparedImport {
        wire_version: package.version,
        chat_archive,
        local_tasks,
        task_selection,
        preview,
        projects,
        entities,
        batch_fingerprint,
        manifest,
    })
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum TargetState {
    Fresh,
    AlreadyImported,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ImportOutcome {
    Created,
    AlreadyImported,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportReceipt {
    pub batch_id: String,
    pub outcome: ImportOutcome,
}
const BATCHES: &str = "shelby_swift_import_batches";
const ENTITIES: &str = "shelby_swift_import_entities";

fn require_canonical(conn: &Connection) -> Result<()> {
    if crate::migrations::schema_version(conn).map_err(|_| ImportError::Database)? != 18
        || conn.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))? != 1
    {
        return Err(invalid("candidate schema or foreign keys"));
    }
    Ok(())
}
fn clean_connection(conn: &Connection) -> Result<()> {
    if !conn.is_autocommit() {
        conn.execute_batch("ROLLBACK")
            .map_err(|_| ImportError::UnusableConnection)?;
        if !conn.is_autocommit() {
            return Err(ImportError::UnusableConnection);
        }
    }
    Ok(())
}
/// Read-only eligibility in one SQLite snapshot. The caller owns a clean
/// canonical candidate connection; no tables or rows are created by inspection.
pub fn inspect_target(conn: &Connection, prepared: &PreparedImport) -> Result<TargetState> {
    if !conn.is_autocommit() {
        return Err(invalid("active candidate transaction"));
    }
    require_canonical(conn)?;
    conn.execute_batch("BEGIN DEFERRED")?;
    let result = inspect_locked(conn, prepared).map(|receipt| {
        if receipt.is_some() {
            TargetState::AlreadyImported
        } else {
            TargetState::Fresh
        }
    });
    clean_connection(conn)?;
    result
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let kind: Option<String> = conn
        .query_row(
            "SELECT type FROM sqlite_master WHERE name=?1",
            [name],
            |r| r.get(0),
        )
        .optional()?;
    match kind.as_deref() {
        None => Ok(false),
        Some("table") => Ok(true),
        _ => Err(ImportError::Conflict),
    }
}
fn count(conn: &Connection, table: &str) -> Result<usize> {
    // Table identifiers come only from module constants.
    Ok(
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
            r.get::<_, i64>(0)
        })? as usize,
    )
}
fn target_matches(conn: &Connection, e: &Entity) -> Result<bool> {
    let key = if e.kind == "project" {
        "project_id"
    } else if e.kind == "alias" {
        "slug"
    } else {
        "id"
    };
    let sql = format!(
        "SELECT {} FROM {} WHERE {key}=?1",
        e.target.columns.join(","),
        e.target.table
    );
    let result = conn
        .query_row(&sql, [&e.target_id], |row| {
            for (index, expected) in e.target.values.iter().enumerate() {
                let matches = match (row.get_ref(index)?, expected) {
                    (rusqlite::types::ValueRef::Null, Value::Null) => true,
                    (rusqlite::types::ValueRef::Text(bytes), Value::String(text)) => {
                        bytes == text.as_bytes()
                    }
                    (rusqlite::types::ValueRef::Integer(n), Value::Number(v)) => {
                        v.as_i64() == Some(n)
                    }
                    _ => false,
                };
                if !matches {
                    return Ok(false);
                }
            }
            Ok(true)
        })
        .optional()?;
    Ok(result.unwrap_or(false))
}
fn stored_source_hash(kind: &str, source: &str) -> Result<String> {
    if source.len() > MAX_BYTES {
        return Err(ImportError::Conflict);
    }
    // Re-validate the bounded typed stored projection, including duplicate and
    // unknown keys, before accepting its provenance as the same source record.
    fn parse<T: serde::de::DeserializeOwned + Serialize>(source: &str) -> Result<Value> {
        let value: T = serde_json::from_str(source).map_err(|_| ImportError::Conflict)?;
        serde_json::to_value(value).map_err(|_| ImportError::Conflict)
    }
    let value = match kind {
        "project" => parse::<wire::Project>(source)?,
        "alias" => parse::<wire::Alias>(source)?,
        "memory" => parse::<wire::Thought>(source)?,
        "edge" => parse::<wire::Edge>(source)?,
        _ => return Err(ImportError::Conflict),
    };
    source_hash(kind, value)
}
fn inspect_locked(conn: &Connection, prepared: &PreparedImport) -> Result<Option<ImportReceipt>> {
    let history = TargetHistory {
        connected_clients: count(conn, "oauth_clients")? > 0,
        feedback: count(conn, "feedback")? > 0,
        search_history: count(conn, "search_telemetry")? > 0,
    };
    if history.connected_clients || history.feedback || history.search_history {
        return Err(ImportError::ExistingHistory(history));
    }
    let expected = [
        ("projects", prepared.preview.project_count),
        ("project_slug_aliases", prepared.preview.alias_count),
        ("thoughts", prepared.preview.memory_count),
        ("edges", prepared.preview.edge_count),
    ];
    let actual = expected
        .iter()
        .map(|(table, _)| count(conn, table))
        .collect::<Result<Vec<_>>>()?;
    let batches = table_exists(conn, BATCHES)?;
    let entities = table_exists(conn, ENTITIES)?;
    if !batches && !entities {
        return if actual.iter().all(|n| *n == 0) {
            Ok(None)
        } else {
            Err(ImportError::Conflict)
        };
    }
    if !batches
        || !entities
        || count(conn, BATCHES)? != 1
        || count(conn, ENTITIES)? != prepared.entities.len()
        || actual
            .iter()
            .zip(&expected)
            .any(|(actual, (_, expected))| actual != expected)
    {
        return Err(ImportError::Conflict);
    }
    let (batch_id, fingerprint, version, manifest_valid): (String, String, i64, bool) = conn.query_row(
        "SELECT batch_id,batch_fingerprint,policy_version,source_manifest,manifest_fingerprint FROM shelby_swift_import_batches",
        [],
        |r| {
            let manifest=r.get_ref(3)?.as_str()?;
            let expected=r.get_ref(4)?.as_str()?;
            let valid=manifest.len()<=32*1024 && format!("{:x}",Sha256::digest(manifest.as_bytes()))==expected;
            Ok((r.get(0)?,r.get(1)?,r.get(2)?,valid))
        },
    )?;
    if uuid(&batch_id).is_err()
        || fingerprint != prepared.batch_fingerprint
        || version != POLICY_VERSION
        || !manifest_valid
    {
        return Err(ImportError::Conflict);
    }
    for e in &prepared.entities {
        let ledger_matches=conn.query_row("SELECT fingerprint,target_id,batch_id,policy_version,source_payload FROM shelby_swift_import_entities WHERE source_product=?1 AND entity_kind=?2 AND source_id=?3",params![PRODUCT,e.kind,e.id],|row| {
            let fp=row.get_ref(0)?.as_str()?;
            let target=row.get_ref(1)?.as_str()?;
            let batch=row.get_ref(2)?.as_str()?;
            let version=row.get::<_,i64>(3)?;
            let source=row.get_ref(4)?.as_str()?;
            Ok(fp==e.fingerprint && target==e.target_id && batch==batch_id && version==POLICY_VERSION && stored_source_hash(e.kind,source).is_ok_and(|h|h==e.fingerprint))
        }).optional()?.unwrap_or(false);
        if !ledger_matches || !target_matches(conn, e)? {
            return Err(ImportError::Conflict);
        }
    }
    Ok(Some(ImportReceipt {
        batch_id,
        outcome: ImportOutcome::AlreadyImported,
    }))
}
fn sql_value(v: &Value) -> rusqlite::types::Value {
    match v {
        Value::Null => rusqlite::types::Value::Null,
        Value::String(s) => rusqlite::types::Value::Text(s.clone()),
        Value::Number(n) => rusqlite::types::Value::Integer(n.as_i64().expect("validated integer")),
        _ => unreachable!("private scalar target projection"),
    }
}
fn create_import(conn: &Connection, prepared: &PreparedImport) -> Result<ImportReceipt> {
    let batch_id = uuid::Uuid::new_v4().to_string();
    conn.execute_batch(
        "CREATE TABLE shelby_swift_import_batches (
        batch_id TEXT PRIMARY KEY NOT NULL, batch_fingerprint TEXT NOT NULL UNIQUE,
        policy_version INTEGER NOT NULL, source_manifest TEXT NOT NULL, committed_at TEXT NOT NULL,
        manifest_fingerprint TEXT NOT NULL
    ); CREATE TABLE shelby_swift_import_entities (
        source_product TEXT NOT NULL, entity_kind TEXT NOT NULL, source_id TEXT NOT NULL,
        fingerprint TEXT NOT NULL, target_id TEXT NOT NULL, batch_id TEXT NOT NULL,
        policy_version INTEGER NOT NULL, source_payload TEXT NOT NULL,
        PRIMARY KEY(source_product,entity_kind,source_id),
        FOREIGN KEY(batch_id) REFERENCES shelby_swift_import_batches(batch_id)
    );",
    )?;
    conn.execute(
        "INSERT INTO shelby_swift_import_batches VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            batch_id,
            prepared.batch_fingerprint,
            POLICY_VERSION,
            prepared.manifest,
            crate::now_iso(),
            format!("{:x}", Sha256::digest(prepared.manifest.as_bytes()))
        ],
    )?;
    for e in &prepared.entities {
        let placeholders = vec!["?"; e.target.values.len()].join(",");
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({placeholders})",
            e.target.table,
            e.target.columns.join(",")
        );
        conn.execute(
            &sql,
            rusqlite::params_from_iter(e.target.values.iter().map(sql_value)),
        )?;
        conn.execute(
            "INSERT INTO shelby_swift_import_entities VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                PRODUCT,
                e.kind,
                e.id,
                e.fingerprint,
                e.target_id,
                batch_id,
                POLICY_VERSION,
                e.source_payload
            ],
        )?;
    }
    let violation = conn.prepare("PRAGMA foreign_key_check")?.exists([])?;
    if violation {
        return Err(ImportError::Database);
    }
    for e in &prepared.entities {
        if e.kind == "memory"
            && crate::thoughts::get_thought(conn, &e.id)
                .map_err(|_| ImportError::Database)?
                .is_none()
        {
            return Err(ImportError::Database);
        }
        if e.kind == "edge" {
            let record = crate::edges::get_edge(conn, &e.id)
                .map_err(|_| ImportError::Database)?
                .ok_or(ImportError::Database)?;
            crate::edges::is_active_at(&record, &crate::now_iso())
                .map_err(|_| ImportError::Database)?;
        }
    }
    if inspect_locked(conn, prepared)?.is_none() {
        return Err(ImportError::Database);
    }
    Ok(ImportReceipt {
        batch_id,
        outcome: ImportOutcome::Created,
    })
}

/// Apply only to an exclusively owned canonical candidate connection. Errors
/// never authorize activation; an UnusableConnection must be disposed of. The
/// returned receipt is the sole success authority, including commit reconciliation.
pub fn apply(conn: &mut Connection, prepared: &PreparedImport) -> Result<ImportReceipt> {
    if !conn.is_autocommit() {
        return Err(invalid("active candidate transaction"));
    }
    require_canonical(conn)?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let operation = (|| {
        let receipt = match inspect_locked(conn, prepared)? {
            Some(receipt) => receipt,
            None => create_import(conn, prepared)?,
        };
        conn.execute_batch("COMMIT")?;
        Ok(receipt)
    })();
    clean_connection(conn)?;
    // Do not equate a commit error with rollback. Re-open one read snapshot and
    // accept only the complete exact durable batch; otherwise return an error.
    conn.execute_batch("BEGIN DEFERRED")?;
    let durable = inspect_locked(conn, prepared);
    clean_connection(conn)?;
    match (operation, durable) {
        (Ok(receipt), Ok(Some(saved))) if receipt.batch_id == saved.batch_id => Ok(receipt),
        (Err(_), Ok(Some(saved))) => Ok(saved),
        (Err(error), _) => Err(error),
        _ => Err(ImportError::Database),
    }
}

#[cfg(test)]
#[path = "swift_import/tests.rs"]
mod tests;

//! The 12 MCP tool handlers (ADR 0001 §6 plus `expand_neighbors`). Each takes
//! JSON args and returns the stable `{content:[{type:"text",text}], isError}`
//! envelope used by existing clients.
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

use crate::Memory;
use crate::brief::{
    BriefScope, BriefScopeInput, DEFAULT_TOKEN_BUDGET, load_brief_candidates,
    render_token_bound_brief, select_brief_items,
};
use crate::edges::{
    EdgeInput, expire_edge, fetch_graph_related, link_thoughts, traverse_graph, unlink_thoughts,
};
use crate::error::Error;
use crate::fts::{SearchOptions, search_thoughts};
use crate::limits::*;
use crate::reconcile::{Decision, can_reconcile, find_candidates, reconcile, tokenize};
use crate::resolve::{
    ProjectReference, ScopeResolution, resolve_project_reference, upsert_provisional_project,
};
use crate::telemetry::record_search_telemetry;
use crate::thoughts::{
    ListOptions, ThoughtInput, ThoughtUpdate, TrustLevel, count_thoughts, delete_thought,
    get_thought, increment_reinforcement, insert_thought, list_thoughts, update_thought,
};
use crate::topics::{canonicalize_topic, canonicalize_topics};
use crate::trust::{fence_record_text, fence_summaries, fence_text};
use crate::vectors::search_by_embedding;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }
}

pub fn success(data: Value) -> ToolResult {
    ToolResult {
        text: serde_json::to_string_pretty(&data).unwrap_or_default(),
        is_error: false,
    }
}

pub fn error(category: &str, message: impl Into<String>) -> ToolResult {
    ToolResult {
        text: json!({ "error": category, "message": message.into() }).to_string(),
        is_error: true,
    }
}

fn from_engine(e: Error) -> ToolResult {
    match e {
        Error::NotFound(m) => error("not_found", m),
        Error::Duplicate(m) => error("duplicate", m),
        Error::InvalidInput(m) => error("invalid_input", m),
        other => error("temporary_failure", other.to_string()),
    }
}

fn parse_args<T: for<'de> Deserialize<'de>>(args: &Value) -> Result<T, ToolResult> {
    serde_json::from_value(args.clone())
        .map_err(|e| error("invalid_input", format!("invalid arguments: {e}")))
}

macro_rules! try_tool {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => return from_engine(e),
        }
    };
}

fn validate_lengths(
    content: &str,
    summary: Option<&str>,
    topics: Option<&[String]>,
    people: Option<&[String]>,
) -> Option<String> {
    if js_len(content) > MAX_CONTENT_LENGTH {
        return Some(format!(
            "content exceeds maximum length of {MAX_CONTENT_LENGTH} characters (got {})",
            js_len(content)
        ));
    }
    if let Some(s) = summary
        && js_len(s) > MAX_SUMMARY_LENGTH
    {
        return Some(format!(
            "summary exceeds maximum length of {MAX_SUMMARY_LENGTH} characters (got {})",
            js_len(s)
        ));
    }
    if let Some(t) = topics {
        if t.len() > MAX_TOPICS_COUNT {
            return Some(format!(
                "topics array exceeds maximum of {MAX_TOPICS_COUNT} entries"
            ));
        }
        if let Some(bad) = t.iter().find(|x| js_len(x) > MAX_TOPIC_LENGTH) {
            return Some(format!(
                "topic \"{}...\" exceeds maximum length of {MAX_TOPIC_LENGTH} characters",
                bad.chars().take(30).collect::<String>()
            ));
        }
    }
    if let Some(p) = people {
        if p.len() > MAX_PEOPLE_COUNT {
            return Some(format!(
                "people array exceeds maximum of {MAX_PEOPLE_COUNT} entries"
            ));
        }
        if let Some(bad) = p.iter().find(|x| js_len(x) > MAX_PERSON_LENGTH) {
            return Some(format!(
                "person \"{}...\" exceeds maximum length of {MAX_PERSON_LENGTH} characters",
                bad.chars().take(30).collect::<String>()
            ));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// capture_thought
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CaptureItem {
    pub content: Option<String>,
    pub summary: Option<String>,
    pub r#type: Option<String>,
    pub source: Option<String>,
    pub source_agent: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub project: Option<String>,
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub visibility: Option<String>,
    pub topics: Option<Vec<String>>,
    pub people: Option<Vec<String>>,
    pub metadata: Option<Map<String, Value>>,
    pub related_to: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CaptureArgs {
    #[serde(flatten)]
    pub single: CaptureItem,
    pub thoughts: Option<Vec<CaptureItem>>,
}

pub fn validate_capture_thought_input(args: &Value) -> Result<CaptureArgs, ToolResult> {
    let bulk = args.get("thoughts").and_then(Value::as_array);
    if let Some(thoughts) = bulk {
        if thoughts.is_empty() {
            return Err(error("invalid_input", "thoughts array is empty"));
        }
        if thoughts.len() > MAX_BULK_THOUGHTS {
            return Err(error(
                "invalid_input",
                format!(
                    "bulk capture exceeds maximum of {MAX_BULK_THOUGHTS} thoughts per call (got {})",
                    thoughts.len()
                ),
            ));
        }
        for (index, thought) in thoughts.iter().enumerate() {
            if thought
                .get("content")
                .and_then(Value::as_str)
                .filter(|content| !content.is_empty())
                .is_none()
            {
                return Err(error(
                    "invalid_input",
                    format!("thoughts[{index}].content is required and must be a string"),
                ));
            }
            if thought
                .get("summary")
                .and_then(Value::as_str)
                .filter(|summary| !summary.trim().is_empty())
                .is_none()
            {
                return Err(error(
                    "invalid_input",
                    format!("thoughts[{index}].summary is required and must be a non-empty string"),
                ));
            }
        }
    } else {
        if args
            .get("content")
            .and_then(Value::as_str)
            .filter(|content| !content.is_empty())
            .is_none()
        {
            return Err(error(
                "invalid_input",
                "content is required and must be a string. For bulk capture, provide a thoughts array.",
            ));
        }
        if args
            .get("summary")
            .and_then(Value::as_str)
            .filter(|summary| !summary.trim().is_empty())
            .is_none()
        {
            return Err(error(
                "invalid_input",
                "summary is required and must be a non-empty string",
            ));
        }
    }

    let mut parsed: CaptureArgs = parse_args(args)?;
    if let Some(thoughts) = parsed.thoughts.as_mut() {
        for (index, thought) in thoughts.iter_mut().enumerate() {
            let summary = thought
                .summary
                .as_mut()
                .expect("preflight requires summary");
            *summary = summary.trim().to_string();
            if let Some(message) = validate_lengths(
                thought
                    .content
                    .as_deref()
                    .expect("preflight requires content"),
                Some(summary),
                thought.topics.as_deref(),
                thought.people.as_deref(),
            ) {
                return Err(error(
                    "invalid_input",
                    format!("thoughts[{index}].{message}"),
                ));
            }
        }
    } else {
        let summary = parsed
            .single
            .summary
            .as_mut()
            .expect("preflight requires summary");
        *summary = summary.trim().to_string();
        if let Some(message) = validate_lengths(
            parsed
                .single
                .content
                .as_deref()
                .expect("preflight requires content"),
            Some(summary),
            parsed.single.topics.as_deref(),
            parsed.single.people.as_deref(),
        ) {
            return Err(error("invalid_input", message));
        }
    }
    Ok(parsed)
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedRef {
    project_id: String,
    current_slug: String,
}

struct CaptureScope {
    reference: Option<ResolvedRef>,
    derived: Option<ScopeResolution>,
}

fn effective_visibility(item: &CaptureItem) -> String {
    item.visibility.clone().unwrap_or_else(|| {
        if item.r#type.as_deref() == Some("preference") {
            "shared".into()
        } else {
            "personal".into()
        }
    })
}

fn scope_error(kind: &str) -> ToolResult {
    error(
        "project_scope_invalid",
        format!(
            "Project scope could not be resolved ({kind}). Pass a registered project_id or project_identifier."
        ),
    )
}

fn capture_scope(
    m: &Memory,
    item: &CaptureItem,
    detected: &ScopeResolution,
) -> Result<CaptureScope, ToolResult> {
    if item.project_id.is_some() || item.project_identifier.is_some() {
        return match resolve_project_reference(
            &m.conn,
            item.project_id.as_deref(),
            item.project_identifier.as_deref(),
        )
        .map_err(from_engine)?
        {
            ProjectReference::Resolved {
                project_id,
                current_slug,
            } => Ok(CaptureScope {
                reference: Some(ResolvedRef {
                    project_id,
                    current_slug,
                }),
                derived: None,
            }),
            other => Err(scope_error(other.kind())),
        };
    }
    if effective_visibility(item) == "shared" {
        return Ok(CaptureScope {
            reference: None,
            derived: None,
        });
    }
    match detected {
        ScopeResolution::Unresolved => Err(error(
            "project_scope_unresolved",
            "Personal captures require a project. Pass a registered project_id or project_identifier, or configure MCP client roots.",
        )),
        ScopeResolution::Ambiguous { slugs } => Err(error(
            "project_scope_ambiguous",
            format!(
                "Client roots resolve to multiple projects ({}). Pass a registered project_id or project_identifier.",
                slugs.join(", ")
            ),
        )),
        ScopeResolution::InvalidExplicit { slug } => Err(error(
            "project_scope_invalid",
            format!("project_identifier \"{slug}\" is unknown or noncanonical."),
        )),
        ScopeResolution::Resolved { slug, source, .. } => {
            match resolve_project_reference(&m.conn, None, Some(slug)).map_err(from_engine)? {
                ProjectReference::Resolved {
                    project_id,
                    current_slug,
                } => Ok(CaptureScope {
                    reference: Some(ResolvedRef {
                        project_id,
                        current_slug,
                    }),
                    derived: Some(detected.clone()),
                }),
                other if *source != crate::resolve::ScopeSource::Derived => {
                    Err(scope_error(other.kind()))
                }
                _ => Ok(CaptureScope {
                    reference: None,
                    derived: Some(detected.clone()),
                }),
            }
        }
    }
}

fn resolve_capture_scope(m: &Memory, scope: &CaptureScope) -> crate::Result<Option<ResolvedRef>> {
    if scope.reference.is_some() || scope.derived.is_none() {
        return Ok(scope.reference.clone());
    }
    let derived = scope.derived.as_ref().unwrap();
    upsert_provisional_project(&m.conn, derived)?;
    let ScopeResolution::Resolved { slug, .. } = derived else {
        return Ok(None);
    };
    match resolve_project_reference(&m.conn, None, Some(slug))? {
        ProjectReference::Resolved {
            project_id,
            current_slug,
        } => Ok(Some(ResolvedRef {
            project_id,
            current_slug,
        })),
        _ => Err(Error::InvalidInput(
            "project_scope_resolution_failed".into(),
        )),
    }
}

fn sensitivity_rank(v: Option<&Value>) -> u8 {
    match v.and_then(Value::as_str) {
        Some("private") => 1,
        Some("secret") => 2,
        _ => 0,
    }
}

fn extra_obj(m: Option<&Map<String, Value>>) -> Option<&Map<String, Value>> {
    m.and_then(|m| m.get("extra")).and_then(Value::as_object)
}

/// §6c: a stricter incoming sensitivity is written into the survivor's metadata.
fn stricter_sensitivity(
    existing: Option<&Map<String, Value>>,
    incoming: Option<&Map<String, Value>>,
) -> Option<Map<String, Value>> {
    let incoming_s = extra_obj(incoming).and_then(|e| e.get("sensitivity"))?;
    if !incoming_s.is_string()
        || sensitivity_rank(Some(incoming_s))
            <= sensitivity_rank(extra_obj(existing).and_then(|e| e.get("sensitivity")))
    {
        return None;
    }
    let mut merged = existing.cloned().unwrap_or_default();
    let mut extra = extra_obj(existing).cloned().unwrap_or_default();
    extra.insert("sensitivity".into(), incoming_s.clone());
    merged.insert("extra".into(), Value::Object(extra));
    Some(merged)
}

fn attach_related(
    m: &Memory,
    source_id: &str,
    related: &[String],
    known: &HashSet<String>,
) -> crate::Result<(Vec<String>, Vec<String>)> {
    let mut linked = Vec::new();
    let mut skipped = Vec::new();
    let mut unique: Vec<String> = Vec::new();
    for id in related {
        if !unique.contains(id) {
            unique.push(id.clone());
        }
    }
    let mut existing: HashSet<String> = known.clone();
    for id in &unique {
        if id != source_id
            && !existing.contains(id)
            && crate::thoughts::thought_exists(&m.conn, id)?
        {
            existing.insert(id.clone());
        }
    }
    for id in unique {
        if id == source_id || !existing.contains(&id) {
            skipped.push(id);
            continue;
        }
        match link_thoughts(
            &m.conn,
            &EdgeInput {
                source_id: source_id.to_string(),
                target_id: id.clone(),
                edge_type: "related".into(),
                ..Default::default()
            },
        ) {
            Ok(_) => linked.push(id),
            Err(_) => skipped.push(id),
        }
    }
    Ok((linked, skipped))
}

fn capture_single(
    m: &Memory,
    item: &CaptureItem,
    resolved: Option<&ResolvedRef>,
) -> crate::Result<Value> {
    let content = item.content.clone().unwrap_or_default();
    let visibility = effective_visibility(item);
    let ty = match item.r#type.as_deref() {
        Some("preference") => "decision".to_string(),
        Some(t) => t.to_string(),
        None => "note".to_string(),
    };
    let topics = canonicalize_topics(item.topics.as_deref().unwrap_or(&[]));
    let tokens = tokenize(&content);
    let incoming_trust = item.trust_level.unwrap_or(TrustLevel::Trusted);
    let all = find_candidates(
        &m.conn,
        &content,
        resolved.map(|r| r.project_id.as_str()),
        item.project.as_deref(),
        &tokens,
    )?;
    let (candidates, blocked): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|c| can_reconcile(incoming_trust, c.trust_level));
    let decision = reconcile(&content, &ty, &candidates, &tokens);
    let blocked_decision = reconcile(&content, &ty, &blocked, &tokens);
    let trust_gate_prevented = matches!(blocked_decision, Decision::Noop { .. })
        || matches!(&blocked_decision, Decision::Add { suggested_edges } if suggested_edges.iter().any(|e| e.edge_type == Some("refuted_by")));

    if let Decision::Noop { existing_id } = &decision {
        let existing = get_thought(&m.conn, existing_id)?
            .ok_or_else(|| Error::NotFound(existing_id.clone()))?;
        let mut update = ThoughtUpdate::default();
        let mut merged_topics = existing.topics.clone();
        for t in &topics {
            if !merged_topics.contains(t) {
                merged_topics.push(t.clone());
            }
        }
        if merged_topics.len() != existing.topics.len() {
            update.topics = Some(merged_topics);
        }
        let mut merged_people = existing.people.clone();
        for p in item.people.as_deref().unwrap_or(&[]) {
            if !merged_people.contains(p) {
                merged_people.push(p.clone());
            }
        }
        if merged_people.len() != existing.people.len() {
            update.people = Some(merged_people);
        }
        if let Some(s) = &item.summary
            && !s.trim().is_empty()
            && Some(s) != existing.summary.as_ref()
        {
            update.summary = Some(s.clone());
        }
        if existing.visibility == "shared" && visibility == "personal" {
            update.visibility = Some("personal".into());
        }
        if incoming_trust.rank() > existing.trust_level.rank() {
            update.trust_level = Some(incoming_trust);
        }
        if let Some(meta) = stricter_sensitivity(existing.metadata.as_ref(), item.metadata.as_ref())
        {
            update.metadata = Some(meta);
        }
        let has_updates = update.topics.is_some()
            || update.people.is_some()
            || update.summary.is_some()
            || update.visibility.is_some()
            || update.trust_level.is_some()
            || update.metadata.is_some();
        increment_reinforcement(&m.conn, &existing.id, 1, true)?;
        if has_updates {
            update_thought(&m.conn, &existing.id, &update)?;
        }
        let (linked, skipped) = attach_related(
            m,
            &existing.id,
            item.related_to.as_deref().unwrap_or(&[]),
            &HashSet::new(),
        )?;
        return Ok(
            json!({ "id": existing.id, "action": if has_updates { "merged" } else { "reinforced" }, "linked": linked, "skipped": skipped, "suggested_connections": [] }),
        );
    }

    let id = insert_thought(
        &m.conn,
        &ThoughtInput {
            content: content.clone(),
            summary: item.summary.clone(),
            r#type: Some(ty),
            source: item.source.clone(),
            source_agent: item.source_agent.clone(),
            trust_level: item.trust_level,
            project: item.project.clone(),
            project_id: None,
            project_identifier: resolved.map(|r| r.current_slug.clone()),
            topics: Some(topics),
            people: item.people.clone(),
            visibility: Some(visibility),
            metadata: item.metadata.clone(),
        },
    )?;
    if let Some(r) = resolved {
        m.conn.execute(
            "UPDATE thoughts SET project_id = ?1 WHERE id = ?2",
            rusqlite::params![r.project_id, id],
        )?;
    }
    let Decision::Add { suggested_edges } = decision else {
        unreachable!()
    };
    let reversals: Vec<&str> = suggested_edges
        .iter()
        .filter(|e| e.edge_type == Some("refuted_by"))
        .map(|e| e.id.as_str())
        .collect();
    for superseded in &reversals {
        link_thoughts(
            &m.conn,
            &EdgeInput {
                source_id: superseded.to_string(),
                target_id: id.clone(),
                edge_type: "refuted_by".into(),
                ..Default::default()
            },
        )?;
    }
    let known: HashSet<String> = candidates.iter().map(|c| c.id.clone()).collect();
    let mut related: Vec<String> = item.related_to.clone().unwrap_or_default();
    related.extend(
        suggested_edges
            .iter()
            .filter(|e| e.auto_apply)
            .map(|e| e.id.clone()),
    );
    let (linked, skipped) = attach_related(m, &id, &related, &known)?;
    let mut suggestions: Vec<Value> = suggested_edges
        .iter()
        .filter(|e| !e.auto_apply)
        .take(5)
        .map(|e| {
            let summary = candidates.iter().find(|c| c.id == e.id).and_then(|c| c.summary.clone());
            let mut v = json!({ "id": e.id, "summary": summary, "similarity_reason": format!("Jaccard token similarity: {:.2}", e.similarity) });
            if let Some(et) = e.edge_type {
                v["edge_type"] = json!(et);
                v["source_id"] = json!(e.id);
                v["target_id"] = json!(id);
            }
            v
        })
        .collect();
    fence_value_summaries(m, &mut suggestions)?;
    let action = if !reversals.is_empty() {
        "superseded"
    } else if trust_gate_prevented {
        "stored_unverified"
    } else {
        "created"
    };
    Ok(
        json!({ "id": id, "action": action, "linked": linked, "skipped": skipped, "suggested_connections": suggestions }),
    )
}

/// Fence `summary` on a list of JSON objects carrying `id`.
fn fence_value_summaries(m: &Memory, items: &mut [Value]) -> crate::Result<()> {
    let mut pairs: Vec<(String, Option<String>)> = items
        .iter()
        .map(|v| {
            (
                v["id"].as_str().unwrap_or_default().to_string(),
                v["summary"].as_str().map(str::to_owned),
            )
        })
        .collect();
    fence_summaries(&m.conn, &mut pairs, |p| &p.0, |p| &mut p.1)?;
    for (item, (_, summary)) in items.iter_mut().zip(pairs) {
        item["summary"] = summary.map(Value::String).unwrap_or(Value::Null);
    }
    Ok(())
}

fn in_transaction<T>(m: &Memory, f: impl FnOnce() -> crate::Result<T>) -> crate::Result<T> {
    m.conn.execute_batch("BEGIN")?;
    match f() {
        Ok(v) => {
            m.conn.execute_batch("COMMIT")?;
            Ok(v)
        }
        Err(e) => {
            let _ = m.conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

pub fn capture_thought(m: &Memory, args: &Value, detected: &ScopeResolution) -> ToolResult {
    let a = match validate_capture_thought_input(args) {
        Ok(a) => a,
        Err(e) => return e,
    };
    if let Some(thoughts) = &a.thoughts {
        let mut scopes = Vec::with_capacity(thoughts.len());
        for t in thoughts {
            match capture_scope(m, t, detected) {
                Ok(s) => scopes.push(s),
                Err(e) => return e,
            }
        }
        let results = try_tool!(in_transaction(m, || {
            thoughts
                .iter()
                .zip(&scopes)
                .map(|(t, s)| capture_single(m, t, resolve_capture_scope(m, s)?.as_ref()))
                .collect::<crate::Result<Vec<_>>>()
        }));
        return success(json!({ "captured": results.len(), "thoughts": results }));
    }
    let item = &a.single;
    let scope = match capture_scope(m, item, detected) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let result = try_tool!(in_transaction(m, || capture_single(
        m,
        item,
        resolve_capture_scope(m, &scope)?.as_ref()
    )));
    success(result)
}

// ---------------------------------------------------------------------------
// Read scope shared by search/list/brief/context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReadScopeArgs {
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub include_shared: Option<bool>,
    pub shared_only: Option<bool>,
    pub all_projects: Option<bool>,
}

struct ReadScope {
    project_id: Option<String>,
    current_slug: Option<String>,
}

fn read_scope(m: &Memory, a: &ReadScopeArgs) -> Result<ReadScope, ToolResult> {
    if a.project_id.is_none() && a.project_identifier.is_none() {
        return Ok(ReadScope {
            project_id: None,
            current_slug: None,
        });
    }
    match resolve_project_reference(
        &m.conn,
        a.project_id.as_deref(),
        a.project_identifier.as_deref(),
    )
    .map_err(from_engine)?
    {
        ProjectReference::Resolved {
            project_id,
            current_slug,
        } => Ok(ReadScope {
            project_id: Some(project_id),
            current_slug: Some(current_slug),
        }),
        other => Err(scope_error(other.kind())),
    }
}

// ---------------------------------------------------------------------------
// search_thoughts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchArgs {
    #[serde(flatten)]
    pub scope: ReadScopeArgs,
    pub query: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub r#type: Option<String>,
    pub project: Option<String>,
    pub graph_depth: Option<i64>,
    pub topic: Option<String>,
}

fn summary_json(
    id: &str,
    summary: Option<&str>,
    ty: &str,
    topics: &[String],
    created_at: &str,
    extra: Value,
) -> Value {
    let mut v = json!({ "id": id, "summary": summary, "type": ty, "topics": topics, "created_at": created_at });
    if let Value::Object(extra) = extra {
        for (k, val) in extra {
            v[k] = val;
        }
    }
    v
}

fn graph_related_json(m: &Memory, ids: &[String], depth: i64) -> crate::Result<Vec<Value>> {
    let mut related: Vec<Value> = fetch_graph_related(&m.conn, ids, depth)?
        .into_iter()
        .map(|r| serde_json::to_value(r).unwrap())
        .collect();
    fence_value_summaries(m, &mut related)?;
    Ok(related)
}

pub fn search_thoughts_tool(m: &Memory, args: &Value) -> ToolResult {
    let a: SearchArgs = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let has_query = a.query.as_deref().is_some_and(|q| !q.is_empty());
    if !has_query && a.embedding.is_none() {
        return error(
            "invalid_input",
            "Either query (for full-text search) or embedding (for vector search) is required.",
        );
    }
    if let Some(e) = &a.embedding
        && e.is_empty()
    {
        return error(
            "invalid_input",
            "embedding must be a non-empty number array",
        );
    }
    let scope = match read_scope(m, &a.scope) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let all_projects = a.scope.all_projects == Some(true);
    let project_id = if all_projects {
        None
    } else {
        scope.project_id.clone()
    };
    let include_shared = a.scope.include_shared == Some(true);
    let shared_only = a.scope.shared_only == Some(true);
    let limit = clamp_limit(a.limit, 20, 100);
    let offset = a.offset.unwrap_or(0);
    let graph_depth = a.graph_depth.unwrap_or(0).clamp(0, 5);
    let canonical_topic = a.topic.as_deref().map(canonicalize_topic);
    let telemetry_slug = if all_projects {
        None
    } else {
        scope.current_slug.clone()
    };
    let record = |mode: &str, results: &[Value], count: i64| {
        let ids: Vec<String> = results
            .iter()
            .filter_map(|r| r["id"].as_str().map(str::to_owned))
            .collect();
        let _ = record_search_telemetry(
            m,
            a.query.as_deref().unwrap_or(""),
            mode,
            count,
            &ids,
            telemetry_slug.as_deref(),
        );
    };
    let load_meta = |ids: &[String]| -> crate::Result<std::collections::HashMap<String, Value>> {
        if ids.is_empty() {
            return Ok(Default::default());
        }
        let sql = format!(
            "SELECT t.id, t.project, t.project_id, COALESCE(p.current_slug, t.project_identifier) AS project_identifier, t.visibility
             FROM thoughts t LEFT JOIN projects p ON p.project_id = t.project_id WHERE t.id IN ({})",
            vec!["?"; ids.len()].join(", ")
        );
        let mut stmt = m.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            Ok((r.get::<_, String>(0)?, json!({ "project": r.get::<_, Option<String>>(1)?, "project_id": r.get::<_, Option<String>>(2)?, "project_identifier": r.get::<_, Option<String>>(3)?, "visibility": r.get::<_, String>(4)? })))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    };
    let matches_filters = |ty: &str, topics: &[String], meta: &Value| -> bool {
        if let Some(t) = &a.r#type
            && t != ty
        {
            return false;
        }
        if let Some(ct) = &canonical_topic
            && !topics.iter().any(|x| x == ct)
        {
            return false;
        }
        if let Some(p) = &a.project
            && meta["project"].as_str() != Some(p)
        {
            return false;
        }
        if shared_only && meta["visibility"] != "shared" {
            return false;
        }
        if let Some(pid) = &project_id
            && meta["project_id"].as_str() != Some(pid)
            && !(include_shared && meta["visibility"] == "shared")
        {
            return false;
        }
        true
    };
    let eligible_ids = |embedding: bool| -> crate::Result<Option<HashSet<String>>> {
        if !embedding {
            return Ok(None);
        }
        let mut wheres = vec!["embedding IS NOT NULL".to_string()];
        let mut vals: Vec<String> = Vec::new();
        if let Some(t) = &a.r#type {
            wheres.push("type = ?".into());
            vals.push(t.clone());
        }
        if let Some(p) = &a.project {
            wheres.push("project = ?".into());
            vals.push(p.clone());
        }
        if let Some(t) = &a.topic {
            wheres.push("topics LIKE ?".into());
            vals.push(crate::topics::topic_like_pattern(t));
        }
        if shared_only {
            wheres.push("visibility = 'shared'".into());
        }
        if let Some(pid) = &project_id {
            wheres.push(
                if include_shared {
                    "(project_id = ? OR visibility = 'shared')"
                } else {
                    "project_id = ?"
                }
                .into(),
            );
            vals.push(pid.clone());
        }
        if wheres.len() == 1 {
            return Ok(None);
        }
        let mut stmt = m.conn.prepare(&format!(
            "SELECT id FROM thoughts WHERE {}",
            wheres.join(" AND ")
        ))?;
        let ids = stmt
            .query_map(rusqlite::params_from_iter(vals.iter()), |r| {
                r.get::<_, String>(0)
            })?
            .collect::<std::result::Result<HashSet<_>, _>>()?;
        Ok(Some(ids))
    };

    if let Some(embedding) = &a.embedding
        && !has_query
    {
        let eligible = try_tool!(eligible_ids(true));
        let pool =
            if project_id.is_some() || a.r#type.is_some() || a.project.is_some() || shared_only {
                100
            } else {
                limit
            };
        let candidates = try_tool!(search_by_embedding(
            &m.conn,
            embedding,
            Some(pool),
            None,
            eligible.as_ref()
        ));
        let meta = try_tool!(load_meta(
            &candidates.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
        ));
        let filtered: Vec<Value> = candidates
            .iter()
            .filter(|c| meta.get(&c.id).is_some_and(|mt| matches_filters(&c.r#type, &c.topics, mt)))
            .map(|c| summary_json(&c.id, c.summary.as_deref(), &c.r#type, &c.topics, &c.created_at, json!({ "similarity": c.similarity, "project": meta[&c.id]["project"], "project_id": meta[&c.id]["project_id"], "project_identifier": meta[&c.id]["project_identifier"], "visibility": meta[&c.id]["visibility"] })))
            .collect();
        let total = filtered.len() as i64;
        let mut results: Vec<Value> = filtered.into_iter().take(limit as usize).collect();
        let ids: Vec<String> = results
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect();
        let related = try_tool!(graph_related_json(m, &ids, graph_depth));
        try_tool!(fence_value_summaries(m, &mut results));
        record("vector", &results, total);
        let mut out = json!({ "mode": "vector", "results": results, "total_count": total, "has_more": total > (ids.len() as i64), "offset": 0 });
        if graph_depth > 0 {
            out["graph_related"] = json!(related);
        }
        return success(out);
    }

    let fts = |lim: i64, off: i64| {
        search_thoughts(
            &m.conn,
            &SearchOptions {
                query: a.query.clone().unwrap_or_default(),
                limit: Some(lim),
                offset: Some(off),
                r#type: a.r#type.clone(),
                project: a.project.clone(),
                project_id: project_id.clone(),
                include_shared,
                shared_only,
                topic: a.topic.clone(),
            },
        )
    };

    if a.embedding.is_none() {
        let r = try_tool!(fts(limit, offset));
        let mut results: Vec<Value> = r
            .results
            .iter()
            .map(|x| serde_json::to_value(x).unwrap())
            .collect();
        let ids: Vec<String> = r.results.iter().map(|x| x.id.clone()).collect();
        let related = try_tool!(graph_related_json(m, &ids, graph_depth));
        try_tool!(fence_value_summaries(m, &mut results));
        record("fts", &results, r.total_count);
        let mut out = json!({ "mode": "fts", "results": results, "total_count": r.total_count, "has_more": r.has_more, "offset": r.offset });
        if graph_depth > 0 {
            out["graph_related"] = json!(related);
        }
        return success(out);
    }

    // hybrid: RRF with K = 60 over pools of min(limit*3, 100)
    let embedding = a.embedding.as_ref().unwrap();
    let pool = (limit * 3).min(100);
    let fts_r = try_tool!(fts(pool, 0));
    let eligible = try_tool!(eligible_ids(true));
    let vec_r = try_tool!(search_by_embedding(
        &m.conn,
        embedding,
        Some(pool),
        None,
        eligible.as_ref()
    ));
    let mut order: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<
        String,
        (Option<String>, String, Vec<String>, String),
    > = Default::default();
    for x in &fts_r.results {
        if !by_id.contains_key(&x.id) {
            order.push(x.id.clone());
            by_id.insert(
                x.id.clone(),
                (
                    x.summary.clone(),
                    x.r#type.clone(),
                    x.topics.clone(),
                    x.created_at.clone(),
                ),
            );
        }
    }
    for x in &vec_r {
        if !by_id.contains_key(&x.id) {
            order.push(x.id.clone());
            by_id.insert(
                x.id.clone(),
                (
                    x.summary.clone(),
                    x.r#type.clone(),
                    x.topics.clone(),
                    x.created_at.clone(),
                ),
            );
        }
    }
    let meta = try_tool!(load_meta(&order));
    const K: f64 = 60.0;
    let rank_of = |ids: Vec<&String>, id: &str| {
        ids.iter()
            .position(|x| x.as_str() == id)
            .map(|p| p as f64 + 1.0)
    };
    let mut scored: Vec<(f64, Value)> = order
        .iter()
        .filter_map(|id| {
            let (summary, ty, topics, created_at) = by_id.get(id)?;
            let mt = meta.get(id)?;
            if !matches_filters(ty, topics, mt) {
                return None;
            }
            let fr = rank_of(fts_r.results.iter().map(|x| &x.id).collect(), id).map_or(0.0, |r| 1.0 / (K + r));
            let vr = rank_of(vec_r.iter().map(|x| &x.id).collect(), id).map_or(0.0, |r| 1.0 / (K + r));
            let score = fr + vr;
            Some((score, summary_json(id, summary.as_deref(), ty, topics, created_at, json!({ "project_id": mt["project_id"], "project_identifier": mt["project_identifier"], "visibility": mt["visibility"], "rrf_score": score }))))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let total = scored.len() as i64;
    let mut results: Vec<Value> = scored
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|(_, v)| v)
        .collect();
    let ids: Vec<String> = results
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    let related = try_tool!(graph_related_json(m, &ids, graph_depth));
    try_tool!(fence_value_summaries(m, &mut results));
    record("hybrid", &results, total);
    let mut out = json!({ "mode": "hybrid", "results": results, "total_count": total, "has_more": offset + (ids.len() as i64) < total, "offset": offset });
    if graph_depth > 0 {
        out["graph_related"] = json!(related);
    }
    success(out)
}

// ---------------------------------------------------------------------------
// list_thoughts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListArgs {
    #[serde(flatten)]
    pub scope: ReadScopeArgs,
    pub r#type: Option<String>,
    pub project: Option<String>,
    pub topic: Option<String>,
    pub person: Option<String>,
    pub source: Option<String>,
    pub source_agent: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub has_summary: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub fn list_thoughts_tool(m: &Memory, args: &Value) -> ToolResult {
    let a: ListArgs = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let scope = match read_scope(m, &a.scope) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let r = try_tool!(list_thoughts(
        &m.conn,
        &ListOptions {
            r#type: a.r#type,
            project: a.project,
            project_id: if a.scope.all_projects == Some(true) {
                None
            } else {
                scope.project_id
            },
            include_shared: a.scope.include_shared == Some(true),
            shared_only: a.scope.shared_only == Some(true),
            topic: a.topic,
            person: a.person,
            source: a.source,
            source_agent: a.source_agent,
            trust_level: a.trust_level,
            since: a.since,
            until: a.until,
            has_summary: a.has_summary,
            limit: Some(clamp_limit(a.limit, 20, 100)),
            offset: a.offset,
        }
    ));
    let mut results: Vec<Value> = r
        .results
        .iter()
        .map(|x| serde_json::to_value(x).unwrap())
        .collect();
    try_tool!(fence_value_summaries(m, &mut results));
    success(
        json!({ "results": results, "total_count": r.total_count, "has_more": r.has_more, "offset": r.offset }),
    )
}

// ---------------------------------------------------------------------------
// get / update / delete
// ---------------------------------------------------------------------------

pub fn get_thought_tool(m: &Memory, args: &Value) -> ToolResult {
    let Some(id) = args["id"].as_str().filter(|s| !s.is_empty()) else {
        return error("invalid_input", "id is required and must be a string");
    };
    let Some(mut thought) = try_tool!(get_thought(&m.conn, id)) else {
        return error("not_found", format!("Thought \"{id}\" not found"));
    };
    if increment_reinforcement(&m.conn, id, 1, false).unwrap_or(false)
        && let Ok(Some(t)) = get_thought(&m.conn, id)
    {
        thought = t;
    }
    let has_embedding = thought.embedding.is_some();
    let trust = Some(thought.trust_level);
    let mut v = serde_json::to_value(&thought).unwrap();
    v["content"] = json!(fence_record_text(&thought.content, trust));
    v["summary"] = thought
        .summary
        .as_deref()
        .map(|s| json!(fence_record_text(s, trust)))
        .unwrap_or(Value::Null);
    v["has_embedding"] = json!(has_embedding);
    success(v)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateArgs {
    pub id: Option<String>,
    pub ids: Option<Vec<String>>,
    pub content: Option<String>,
    pub summary: Option<String>,
    pub r#type: Option<String>,
    pub source: Option<String>,
    pub project: Option<String>,
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub topics: Option<Vec<String>>,
    pub people: Option<Vec<String>>,
    pub metadata: Option<Map<String, Value>>,
    pub visibility: Option<String>,
}

pub fn update_thought_tool(m: &Memory, args: &Value) -> ToolResult {
    let a: UpdateArgs = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let targets: Vec<String> = match (&a.ids, &a.id) {
        (Some(ids), _) => ids.clone(),
        (None, Some(id)) => vec![id.clone()],
        _ => {
            return error(
                "invalid_input",
                "Either id (string) or ids (string[]) is required",
            );
        }
    };
    if targets.is_empty() {
        return error("invalid_input", "No target IDs provided");
    }
    if let Some(msg) = validate_lengths(
        a.content.as_deref().unwrap_or(""),
        a.summary.as_deref(),
        a.topics.as_deref(),
        a.people.as_deref(),
    ) {
        return error("invalid_input", msg);
    }
    let reference = if a.project_id.is_some() || a.project_identifier.is_some() {
        match try_tool!(resolve_project_reference(
            &m.conn,
            a.project_id.as_deref(),
            a.project_identifier.as_deref()
        )) {
            ProjectReference::Resolved {
                project_id,
                current_slug,
            } => Some((project_id, current_slug)),
            other => return error("project_scope_invalid", other.kind()),
        }
    } else {
        None
    };
    let update = ThoughtUpdate {
        content: a.content,
        summary: a.summary,
        r#type: a.r#type.map(|t| {
            if t == "preference" {
                "decision".into()
            } else {
                t
            }
        }),
        source: a.source,
        project: a.project,
        project_id: reference.as_ref().map(|r| r.0.clone()),
        project_identifier: reference.as_ref().map(|r| r.1.clone()),
        topics: a.topics,
        people: a.people,
        metadata: a.metadata,
        visibility: a.visibility,
        ..Default::default()
    };
    let has_fields = update.content.is_some()
        || update.summary.is_some()
        || update.r#type.is_some()
        || update.source.is_some()
        || update.project.is_some()
        || update.project_id.is_some()
        || update.topics.is_some()
        || update.people.is_some()
        || update.metadata.is_some()
        || update.visibility.is_some();
    if !has_fields {
        return error("invalid_input", "No fields to update were provided");
    }
    let mut updated = 0;
    let mut not_found = Vec::new();
    for id in targets {
        if !try_tool!(crate::thoughts::thought_exists(&m.conn, &id)) {
            not_found.push(id);
            continue;
        }
        if try_tool!(update_thought(&m.conn, &id, &update)) {
            updated += 1;
        }
    }
    success(json!({ "updated": updated, "not_found": not_found }))
}

pub fn delete_thought_tool(m: &Memory, args: &Value) -> ToolResult {
    let Some(id) = args["id"].as_str().filter(|s| !s.is_empty()) else {
        return error("invalid_input", "id is required and must be a string");
    };
    if !try_tool!(crate::thoughts::thought_exists(&m.conn, id)) {
        return error(
            "not_found",
            format!("Thought \"{id}\" not found. Try search_thoughts to find it by content."),
        );
    }
    let deleted = try_tool!(delete_thought(&m.conn, id));
    success(json!({ "deleted": deleted, "id": id }))
}

// ---------------------------------------------------------------------------
// manage_edges / explore_graph / expand_neighbors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ManageEdgesArgs {
    pub action: Option<String>,
    pub source_id: Option<String>,
    pub target_id: Option<String>,
    pub edge_type: Option<String>,
    pub metadata: Option<Map<String, Value>>,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub edge_id: Option<String>,
}

pub fn manage_edges_tool(m: &Memory, args: &Value) -> ToolResult {
    let a: ManageEdgesArgs = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return e,
    };
    match a.action.as_deref() {
        Some("link") => match link_thoughts(
            &m.conn,
            &EdgeInput {
                source_id: a.source_id.unwrap_or_default(),
                target_id: a.target_id.unwrap_or_default(),
                edge_type: a.edge_type.unwrap_or_default(),
                metadata: a.metadata,
                valid_from: a.valid_from,
                valid_until: a.valid_until,
            },
        ) {
            Ok(edge_id) => success(json!({ "edge_id": edge_id, "action": "linked" })),
            Err(e) => from_engine(e),
        },
        Some("unlink") => {
            let (s, t, et) = (
                a.source_id.unwrap_or_default(),
                a.target_id.unwrap_or_default(),
                a.edge_type.unwrap_or_default(),
            );
            if try_tool!(unlink_thoughts(&m.conn, &s, &t, &et)) {
                success(json!({ "action": "unlinked" }))
            } else {
                error("not_found", format!("No edge found: {s} -[{et}]-> {t}"))
            }
        }
        Some("expire") => {
            let Some(edge_id) = a.edge_id else {
                return error("invalid_input", "edge_id is required for expire action");
            };
            try_tool!(expire_edge(&m.conn, &edge_id, a.valid_until.as_deref()));
            success(json!({ "action": "expired", "edge_id": edge_id }))
        }
        _ => error(
            "invalid_input",
            "action must be \"link\", \"unlink\", or \"expire\"",
        ),
    }
}

pub fn explore_graph_tool(m: &Memory, args: &Value) -> ToolResult {
    let Some(thought_id) = args["thought_id"].as_str().filter(|s| !s.is_empty()) else {
        return error(
            "invalid_input",
            "thought_id is required and must be a string",
        );
    };
    let depth = args["max_depth"].as_i64().unwrap_or(1);
    let edge_types: Option<Vec<String>> = args["edge_types"].as_array().map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    });
    let nodes = try_tool!(traverse_graph(
        &m.conn,
        thought_id,
        depth,
        edge_types.as_deref(),
        args["include_expired"].as_bool().unwrap_or(false)
    ));
    if nodes.is_empty() {
        return error(
            "not_found",
            format!(
                "Thought \"{thought_id}\" not found. Try search_thoughts to find it by content."
            ),
        );
    }
    let mut nodes_json: Vec<Value> = nodes
        .iter()
        .map(|n| serde_json::to_value(n).unwrap())
        .collect();
    try_tool!(fence_value_summaries(m, &mut nodes_json));
    success(
        json!({ "root": thought_id, "max_depth": depth, "node_count": nodes.len(), "nodes": nodes_json }),
    )
}

pub fn expand_neighbors_tool(m: &Memory, args: &Value) -> ToolResult {
    let Some(thought_id) = args["thought_id"].as_str().filter(|s| !s.is_empty()) else {
        return error(
            "invalid_input",
            "thought_id is required and must be a string",
        );
    };
    let nodes = try_tool!(traverse_graph(&m.conn, thought_id, 1, None, false));
    if nodes.is_empty() {
        return error(
            "not_found",
            format!(
                "Thought \"{thought_id}\" not found. Try search_thoughts to find it by content."
            ),
        );
    }
    let Some(thought) = try_tool!(get_thought(&m.conn, thought_id)) else {
        return error("not_found", "thought vanished");
    };
    let limit = clamp_limit(args["limit"].as_i64(), 10, 100) as usize;
    let neighbors: Vec<_> = nodes.iter().filter(|n| n.depth != 0).collect();
    let mut out: Vec<Value> = Vec::new();
    for n in neighbors.iter().take(limit) {
        let topics = try_tool!(get_thought(&m.conn, &n.id))
            .map(|t| t.topics)
            .unwrap_or_default();
        out.push(json!({ "id": n.id, "summary": n.summary, "type": n.r#type, "topics": topics }));
    }
    try_tool!(fence_value_summaries(m, &mut out));
    let trust = Some(thought.trust_level);
    success(json!({
        "id": thought.id,
        "content": fence_text(&thought.content, trust),
        "summary": thought.summary.as_deref().map(|s| fence_text(s, trust)),
        "neighbor_count": neighbors.len(),
        "neighbors": out,
    }))
}

// ---------------------------------------------------------------------------
// thought_stats
// ---------------------------------------------------------------------------

pub fn thought_stats_tool(m: &Memory) -> ToolResult {
    let counts = |sql: &str| -> crate::Result<Map<String, Value>> {
        let mut stmt = m.conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Map::new();
        for row in rows {
            let (k, v) = row?;
            out.insert(k, json!(v));
        }
        Ok(out)
    };
    let thought_count = try_tool!(count_thoughts(&m.conn));
    let edge_count: i64 = try_tool!(
        m.conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .map_err(Error::from)
    );
    let embedding_count: i64 = try_tool!(
        m.conn
            .query_row(
                "SELECT COUNT(*) FROM thoughts WHERE embedding IS NOT NULL",
                [],
                |r| r.get(0)
            )
            .map_err(Error::from)
    );
    let by_type = try_tool!(counts(
        "SELECT type, COUNT(*) AS cnt FROM thoughts GROUP BY type ORDER BY cnt DESC"
    ));
    let by_edge_type = try_tool!(counts(
        "SELECT edge_type, COUNT(*) AS cnt FROM edges GROUP BY edge_type ORDER BY cnt DESC"
    ));
    success(
        json!({ "thought_count": thought_count, "edge_count": edge_count, "embedding_count": embedding_count, "by_type": by_type, "by_edge_type": by_edge_type }),
    )
}

// ---------------------------------------------------------------------------
// get_brief / select_context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BriefArgs {
    #[serde(flatten)]
    pub scope: ReadScopeArgs,
    #[serde(rename = "scope")]
    pub brief_scope: Option<String>,
    pub token_budget: Option<i64>,
    pub now: Option<String>,
}

pub fn get_brief_tool(m: &Memory, args: &Value) -> ToolResult {
    let a: BriefArgs = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let scope = match a.brief_scope.as_deref() {
        None => BriefScope::Full,
        Some(s) => match BriefScope::parse(s) {
            Some(s) => s,
            None => {
                return error(
                    "invalid_input",
                    "scope must be one of: \"essentials\", \"recent\", \"full\"",
                );
            }
        },
    };
    let project_scope = match read_scope(m, &a.scope) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let all_projects = a.scope.all_projects == Some(true);
    let now = a.now.clone().unwrap_or_else(crate::now_iso);
    let input = BriefScopeInput {
        project_id: if all_projects {
            None
        } else {
            project_scope.project_id.clone()
        },
        include_shared: Some(a.scope.include_shared.unwrap_or(true)),
        shared_only: a.scope.shared_only == Some(true),
        all_projects,
    };
    let candidates = try_tool!(load_brief_candidates(&m.conn, &now, &input));
    let mut policy = select_brief_items(&candidates, scope, &input, &now);
    let budget = a
        .token_budget
        .unwrap_or(DEFAULT_TOKEN_BUDGET)
        .clamp(600, 900);
    let rendered = render_token_bound_brief(&policy.items, &mut policy.omitted_counts, budget);
    let last_activity = rendered.items.iter().map(|i| i.updated_at.as_str()).max();
    success(json!({
        "project_id": if all_projects { None } else { project_scope.project_id },
        "project_identifier": if all_projects { None } else { project_scope.current_slug },
        "scope": scope,
        "thought_count": rendered.items.len(),
        "last_activity": last_activity,
        "policy_version": policy.policy_version,
        "estimated_tokens": rendered.estimated_tokens,
        "omitted_counts": policy.omitted_counts,
        "items": rendered.items,
        "brief": rendered.brief,
    }))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContextArgs {
    #[serde(flatten)]
    pub scope: ReadScopeArgs,
    pub types: Option<Vec<String>>,
    pub topics: Option<Vec<String>>,
    pub people: Option<Vec<String>>,
    pub since: Option<String>,
    pub include_brief: Option<bool>,
    pub include_stats: Option<bool>,
    pub limit: Option<i64>,
}

pub fn select_context_tool(m: &Memory, args: &Value) -> ToolResult {
    for key in ["types", "topics", "people"] {
        if let Some(v) = args.get(key)
            && !v.is_null()
            && !(v.is_array() && v.as_array().unwrap().iter().all(Value::is_string))
        {
            return error(
                "invalid_input",
                format!("{key} must be an array of strings"),
            );
        }
    }
    if let Some(v) = args.get("since")
        && !v.is_null()
        && !v.is_string()
    {
        return error("invalid_input", "since must be an ISO 8601 string");
    }
    let a: ContextArgs = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let project_scope = match read_scope(m, &a.scope) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let all_projects = a.scope.all_projects == Some(true);
    let limit = clamp_limit(a.limit, 20, 100);
    let scoped_project_id = if all_projects {
        None
    } else {
        project_scope.project_id.clone()
    };
    let shared_only = a.scope.shared_only == Some(true) && scoped_project_id.is_none();
    let mut sections: Vec<String> = Vec::new();
    if a.include_brief == Some(true) {
        let brief = get_brief_tool(
            m,
            &json!({
                "scope": "essentials", "project_id": scoped_project_id, "project_identifier": project_scope.current_slug,
                "include_shared": a.scope.include_shared, "shared_only": shared_only, "all_projects": a.scope.all_projects,
            }),
        );
        if !brief.is_error
            && let Some(text) = brief.json()["brief"].as_str()
            && !text.trim().is_empty()
        {
            sections.push(text.to_string());
        }
    }
    let list = |ty: Option<String>| {
        list_thoughts(
            &m.conn,
            &ListOptions {
                r#type: ty,
                topic: a.topics.as_ref().and_then(|t| t.first().cloned()),
                person: a.people.as_ref().and_then(|p| p.first().cloned()),
                since: a.since.clone(),
                project_id: scoped_project_id.clone(),
                include_shared: a.scope.include_shared.unwrap_or(true),
                shared_only,
                limit: Some(limit),
                ..Default::default()
            },
        )
    };
    let mut pool = Vec::new();
    match &a.types {
        Some(types) if !types.is_empty() => {
            for t in types {
                pool.extend(try_tool!(list(Some(t.clone()))).results);
            }
        }
        _ => pool.extend(try_tool!(list(None)).results),
    }
    let mut seen = HashSet::new();
    let mut thoughts = Vec::new();
    for t in pool {
        if seen.insert(t.id.clone()) {
            thoughts.push(t);
            if thoughts.len() as i64 >= limit {
                break;
            }
        }
    }
    if thoughts.is_empty() {
        sections.push("## Selected Context\nNo thoughts matched the given filters.".into());
    } else {
        let mut formatted = Vec::new();
        for s in &thoughts {
            let full = try_tool!(get_thought(&m.conn, &s.id));
            let trust = full.as_ref().map(|f| f.trust_level);
            let topics_line =
                (!s.topics.is_empty()).then(|| format!("Topics: {}", s.topics.join(", ")));
            let people_line = full
                .as_ref()
                .filter(|f| !f.people.is_empty())
                .map(|f| format!("People: {}", f.people.join(", ")));
            let raw = full
                .as_ref()
                .map(|f| f.content.clone())
                .or_else(|| s.summary.clone())
                .unwrap_or_else(|| "(no content)".into());
            let body = if trust == Some(TrustLevel::Trusted) {
                raw
            } else {
                [Some(raw), topics_line.clone(), people_line.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let mut lines = vec![format!("Content: {}", fence_text(&body, trust))];
            if let Some(summary) = full
                .as_ref()
                .and_then(|f| f.summary.clone())
                .filter(|x| !x.is_empty())
            {
                lines.push(format!("Summary: {}", fence_text(&summary, trust)));
            }
            lines.push(format!("Created: {}", s.created_at));
            lines.push(format!("Type: {}", s.r#type));
            if trust == Some(TrustLevel::Trusted) {
                if let Some(t) = topics_line {
                    lines.push(t);
                }
                if let Some(p) = people_line {
                    lines.push(p);
                }
            }
            formatted.push(lines.join("\n"));
        }
        sections.push(format!(
            "## Selected Context ({} thought{})\n{}",
            thoughts.len(),
            if thoughts.len() == 1 { "" } else { "s" },
            formatted.join("\n---\n")
        ));
    }
    if a.include_stats == Some(true) {
        let total = try_tool!(count_thoughts(&m.conn));
        sections.push(format!(
            "## Memory Stats\nTotal: {total} thought{}",
            if total == 1 { "" } else { "s" }
        ));
    }
    success(json!({
        "matched_count": thoughts.len(),
        "project_id": if all_projects { None } else { project_scope.project_id },
        "project_identifier": if all_projects { None } else { project_scope.current_slug },
        "document": sections.join("\n\n"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{ProjectSeed, upsert_project};
    use crate::resolve::ScopeSource;

    fn with_project() -> (Memory, ScopeResolution, String) {
        let m = Memory::open_in_memory().unwrap();
        upsert_project(
            &m.conn,
            &ProjectSeed {
                slug: "shelby".into(),
                display_name: "Shelby".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let detected = ScopeResolution::Resolved {
            slug: "shelby".into(),
            source: ScopeSource::MemberPath,
            member_paths: vec![],
            member_repos: vec![],
        };
        (m, detected, "4abbc729-70e8-5120-9cfc-bd34739c024e".into())
    }

    #[test]
    fn capture_actions_created_reinforced_merged_superseded() {
        let (m, detected, pid) = with_project();
        let content = "we always prefer tabs over spaces in this repo";
        let r = capture_thought(
            &m,
            &json!({ "content": content, "summary": "tabs" }),
            &detected,
        );
        assert!(!r.is_error, "{}", r.text);
        let created = r.json();
        assert_eq!(created["action"], "created");
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(
            get_thought(&m.conn, &id)
                .unwrap()
                .unwrap()
                .project_id
                .as_deref(),
            Some(pid.as_str())
        );

        let r = capture_thought(
            &m,
            &json!({ "content": content, "summary": "tabs" }),
            &detected,
        )
        .json();
        assert_eq!(r["action"], "reinforced");
        assert_eq!(r["id"], id);
        assert_eq!(
            get_thought(&m.conn, &id)
                .unwrap()
                .unwrap()
                .reinforcement_count,
            1
        );

        let r = capture_thought(&m, &json!({ "content": content, "summary": "tabs", "topics": ["Style"], "metadata": { "extra": { "sensitivity": "private" } } }), &detected).json();
        assert_eq!(r["action"], "merged");
        let t = get_thought(&m.conn, &id).unwrap().unwrap();
        assert_eq!(t.topics, vec!["style"]);
        assert_eq!(t.metadata.unwrap()["extra"]["sensitivity"], "private");

        let r = capture_thought(
            &m,
            &json!({ "content": "in this repo we always prefer spaces over tabs", "summary": "spaces" }),
            &detected,
        )
        .json();
        assert_eq!(r["action"], "superseded", "{r}");
        let new_id = r["id"].as_str().unwrap();
        let edges = crate::edges::get_edges_between(&m.conn, &id, new_id).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_type, "refuted_by");
    }

    #[test]
    fn capture_requires_and_normalizes_summaries_before_writing() {
        let m = Memory::open_in_memory().unwrap();

        for args in [
            json!({ "content": "Missing summary", "visibility": "shared" }),
            json!({ "content": "Blank summary", "summary": "   ", "visibility": "shared" }),
            json!({ "content": "Non-string summary", "summary": 42, "visibility": "shared" }),
        ] {
            let result = capture_thought(&m, &args, &ScopeResolution::Unresolved);
            assert!(result.is_error, "{args}");
            assert_eq!(result.json()["error"], "invalid_input");
        }

        for thought in [
            json!({ "content": "Missing summary", "visibility": "shared" }),
            json!({ "content": "Blank summary", "summary": "   ", "visibility": "shared" }),
            json!({ "content": "Non-string summary", "summary": 42, "visibility": "shared" }),
        ] {
            let result = capture_thought(
                &m,
                &json!({ "thoughts": [thought] }),
                &ScopeResolution::Unresolved,
            );
            assert!(result.is_error);
            assert_eq!(result.json()["error"], "invalid_input");
        }
        assert_eq!(count_thoughts(&m.conn).unwrap(), 0);

        let result = capture_thought(
            &m,
            &json!({ "content": "Normalized", "summary": "  Searchable summary  ", "visibility": "shared" }),
            &ScopeResolution::Unresolved,
        );
        assert!(!result.is_error, "{}", result.text);
        let id = result.json()["id"].as_str().unwrap().to_string();
        assert_eq!(
            get_thought(&m.conn, &id)
                .unwrap()
                .unwrap()
                .summary
                .as_deref(),
            Some("Searchable summary")
        );
    }

    #[test]
    fn bulk_capture_validates_every_summary_before_writing() {
        let m = Memory::open_in_memory().unwrap();
        let result = capture_thought(
            &m,
            &json!({
                "thoughts": [
                    { "content": "Valid first item", "summary": "First", "visibility": "shared" },
                    { "content": "Invalid second item", "summary": "   ", "visibility": "shared" }
                ]
            }),
            &ScopeResolution::Unresolved,
        );

        assert!(result.is_error);
        assert_eq!(result.json()["error"], "invalid_input");
        assert_eq!(count_thoughts(&m.conn).unwrap(), 0);
    }

    #[test]
    fn trust_gate_stores_unverified_separately() {
        let (m, detected, _) = with_project();
        let content = "the deploy pipeline requires a signed manifest before release";
        capture_thought(
            &m,
            &json!({ "content": content, "summary": "signed manifest" }),
            &detected,
        );
        let r = capture_thought(
            &m,
            &json!({ "content": content, "summary": "signed manifest", "trust_level": "external" }),
            &detected,
        )
        .json();
        assert_eq!(r["action"], "stored_unverified");
        assert_eq!(count_thoughts(&m.conn).unwrap(), 2);
    }

    #[test]
    fn capture_scope_errors_and_bulk() {
        let m = Memory::open_in_memory().unwrap();
        let r = capture_thought(
            &m,
            &json!({ "content": "personal needs a project", "summary": "Personal scope" }),
            &ScopeResolution::Unresolved,
        );
        assert!(r.is_error);
        assert_eq!(r.json()["error"], "project_scope_unresolved");
        let r = capture_thought(
            &m,
            &json!({ "content": "shared is fine", "summary": "Shared capture", "visibility": "shared" }),
            &ScopeResolution::Unresolved,
        );
        assert!(!r.is_error, "{}", r.text);
        let r = capture_thought(
            &m,
            &json!({ "content": "x", "summary": "Unknown project", "project_identifier": "ghost" }),
            &ScopeResolution::Unresolved,
        );
        assert_eq!(r.json()["error"], "project_scope_invalid");
        let r = capture_thought(&m, &json!({ "thoughts": [] }), &ScopeResolution::Unresolved);
        assert_eq!(r.json()["error"], "invalid_input");
        let derived = ScopeResolution::Resolved {
            slug: "newproj".into(),
            source: ScopeSource::Derived,
            member_paths: vec!["/w/newproj".into()],
            member_repos: vec![],
        };
        let r = capture_thought(&m, &json!({ "thoughts": [{ "content": "one", "summary": "One" }, { "content": "two", "summary": "Two", "related_to": ["nope"] }] }), &derived).json();
        assert_eq!(r["captured"], 2);
        assert_eq!(r["thoughts"][1]["skipped"], json!(["nope"]));
        assert!(
            crate::projects::get_project_by_alias(&m.conn, "newproj")
                .unwrap()
                .unwrap()
                .provisional
        );
        let r = capture_thought(
            &m,
            &json!({ "content": "x".repeat(50_001), "summary": "Too long", "visibility": "shared" }),
            &ScopeResolution::Unresolved,
        );
        assert_eq!(r.json()["error"], "invalid_input");
    }

    #[test]
    fn search_modes_and_hybrid_rrf() {
        let (m, detected, _) = with_project();
        let a = capture_thought(
            &m,
            &json!({ "content": "rust memory engine with fts", "summary": "a" }),
            &detected,
        )
        .json()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let b = capture_thought(
            &m,
            &json!({ "content": "svelte frontend shell", "summary": "b" }),
            &detected,
        )
        .json()["id"]
            .as_str()
            .unwrap()
            .to_string();
        crate::vectors::store_embedding(&m.conn, &a, &[0.0, 1.0]).unwrap();
        crate::vectors::store_embedding(&m.conn, &b, &[1.0, 0.0]).unwrap();
        let r = search_thoughts_tool(
            &m,
            &json!({ "query": "rust", "project_identifier": "shelby" }),
        )
        .json();
        assert_eq!(r["mode"], "fts");
        assert_eq!(r["total_count"], 1);
        let r = search_thoughts_tool(
            &m,
            &json!({ "embedding": [1.0, 0.0], "all_projects": true }),
        )
        .json();
        assert_eq!(r["mode"], "vector");
        assert_eq!(r["results"][0]["id"], b);
        let r = search_thoughts_tool(&m, &json!({ "query": "rust", "embedding": [1.0, 0.0], "all_projects": true, "graph_depth": 1 })).json();
        assert_eq!(r["mode"], "hybrid");
        assert_eq!(r["total_count"], 2);
        assert!(r["results"][0]["rrf_score"].as_f64().unwrap() > 0.0);
        assert!(r["graph_related"].is_array());
        assert!(search_thoughts_tool(&m, &json!({})).is_error);
        let n: i64 = m
            .conn
            .query_row("SELECT COUNT(*) FROM search_telemetry", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn crud_edge_graph_and_stats_tools() {
        let (m, detected, _) = with_project();
        let a = capture_thought(&m, &json!({ "content": "first thought here", "summary": "a", "trust_level": "unverified" }), &detected).json()["id"].as_str().unwrap().to_string();
        let b = capture_thought(
            &m,
            &json!({ "content": "second thought there", "summary": "b" }),
            &detected,
        )
        .json()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let got = get_thought_tool(&m, &json!({ "id": a })).json();
        assert!(
            got["content"]
                .as_str()
                .unwrap()
                .contains("untrusted_memory")
        );
        assert_eq!(got["has_embedding"], false);
        assert_eq!(
            get_thought(&m.conn, &a)
                .unwrap()
                .unwrap()
                .reinforcement_count,
            1,
            "read reinforces"
        );
        assert_eq!(
            get_thought_tool(&m, &json!({ "id": "nope" })).json()["error"],
            "not_found"
        );
        let r = update_thought_tool(
            &m,
            &json!({ "ids": [a, "ghost"], "summary": "renamed", "type": "preference" }),
        )
        .json();
        assert_eq!(r["updated"], 1);
        assert_eq!(r["not_found"], json!(["ghost"]));
        assert_eq!(
            get_thought(&m.conn, &a).unwrap().unwrap().r#type,
            "decision"
        );
        assert_eq!(
            update_thought_tool(&m, &json!({ "id": a })).json()["error"],
            "invalid_input"
        );
        let e = manage_edges_tool(
            &m,
            &json!({ "action": "link", "source_id": a, "target_id": b, "edge_type": "follows" }),
        )
        .json();
        assert_eq!(e["action"], "linked");
        assert_eq!(
            manage_edges_tool(
                &m,
                &json!({ "action": "link", "source_id": a, "target_id": b, "edge_type": "follows" })
            )
            .json()["error"],
            "duplicate"
        );
        assert_eq!(
            manage_edges_tool(
                &m,
                &json!({ "action": "link", "source_id": a, "target_id": b, "edge_type": "loves" })
            )
            .json()["error"],
            "invalid_input"
        );
        let g = explore_graph_tool(&m, &json!({ "thought_id": a })).json();
        assert_eq!(g["node_count"], 2);
        let n = expand_neighbors_tool(&m, &json!({ "thought_id": b })).json();
        assert_eq!(n["neighbor_count"], 1);
        assert!(
            n["neighbors"][0]["summary"]
                .as_str()
                .unwrap()
                .contains("renamed")
        );
        // Default expiration must take effect immediately through the canonical active predicate.
        assert_eq!(
            manage_edges_tool(&m, &json!({ "action": "expire", "edge_id": e["edge_id"] })).json()["action"],
            "expired"
        );
        assert_eq!(
            explore_graph_tool(&m, &json!({ "thought_id": a })).json()["node_count"],
            1
        );
        assert_eq!(manage_edges_tool(&m, &json!({ "action": "unlink", "source_id": a, "target_id": b, "edge_type": "follows" })).json()["action"], "unlinked");
        let s = thought_stats_tool(&m).json();
        assert_eq!(s["thought_count"], 2);
        assert_eq!(s["by_type"]["decision"], 1);
        assert_eq!(
            delete_thought_tool(&m, &json!({ "id": b })).json()["deleted"],
            true
        );
        assert_eq!(
            delete_thought_tool(&m, &json!({ "id": b })).json()["error"],
            "not_found"
        );
    }

    #[test]
    fn brief_and_context_tools() {
        let (m, detected, pid) = with_project();
        capture_thought(
            &m,
            &json!({ "content": "Ship the Rust memory crate before the app port.", "summary": "Ship the Rust memory crate first.", "type": "decision" }),
            &detected,
        );
        let b = get_brief_tool(&m, &json!({ "project_identifier": "shelby" })).json();
        let keys: Vec<&str> = b.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "project_id",
                "project_identifier",
                "scope",
                "thought_count",
                "last_activity",
                "policy_version",
                "estimated_tokens",
                "omitted_counts",
                "items",
                "brief"
            ]
        );
        assert_eq!(b["project_id"], pid);
        assert_eq!(b["thought_count"], 1);
        assert!(
            b["brief"]
                .as_str()
                .unwrap()
                .contains("Ship the Rust memory crate first.")
        );
        assert_eq!(
            get_brief_tool(&m, &json!({ "scope": "weird" })).json()["error"],
            "invalid_input"
        );
        let c = select_context_tool(&m, &json!({ "project_identifier": "shelby", "types": ["decision"], "include_brief": true, "include_stats": true })).json();
        assert_eq!(c["matched_count"], 1);
        let doc = c["document"].as_str().unwrap();
        assert!(
            doc.contains("## Shelby memory context")
                && doc.contains("## Selected Context (1 thought)")
                && doc.contains("Total: 1 thought")
        );
        assert_eq!(
            select_context_tool(&m, &json!({ "types": "decision" })).json()["error"],
            "invalid_input"
        );
    }
}

//! Tool definitions: names, descriptions, JSON input schemas, and annotations
//! (ADR 0001 §6 / §6g). Kept compatible with the committed pre-0.4 contract fixtures.
use assistant_memory::limits::*;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map, Value, json};

const PROJECT_ID_PATTERN: &str =
    "^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";
const TYPES: [&str; 7] = [
    "note",
    "decision",
    "task",
    "question",
    "reference",
    "insight",
    "preference",
];
const TRUST: [&str; 3] = ["trusted", "unverified", "external"];

fn annotations(read_only: bool, destructive: bool, idempotent: bool) -> ToolAnnotations {
    let mut a = ToolAnnotations::new();
    a.read_only_hint = Some(read_only);
    a.destructive_hint = Some(destructive);
    a.idempotent_hint = Some(idempotent);
    a.open_world_hint = Some(false);
    a
}

fn object(properties: Value, required: &[&str]) -> Map<String, Value> {
    let mut schema = Map::new();
    schema.insert("type".into(), json!("object"));
    schema.insert("properties".into(), properties);
    if !required.is_empty() {
        schema.insert("required".into(), json!(required));
    }
    schema
}

fn s(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}
fn n(desc: &str) -> Value {
    json!({ "type": "number", "description": desc })
}
fn b(desc: &str) -> Value {
    json!({ "type": "boolean", "description": desc })
}
fn project_id() -> Value {
    json!({ "type": "string", "pattern": PROJECT_ID_PATTERN, "description": "Immutable canonical project UUID" })
}
fn string_list(desc: &str, max_len: usize, max_items: usize) -> Value {
    json!({ "type": "array", "items": { "type": "string", "maxLength": max_len }, "maxItems": max_items, "description": desc })
}

fn thought_fields(with_related: bool) -> Value {
    let mut p = json!({
        "content": { "type": "string", "maxLength": MAX_CONTENT_LENGTH, "description": "The thought content" },
        "summary": { "type": "string", "minLength": 1, "maxLength": MAX_SUMMARY_LENGTH, "description": "One-line summary for search results" },
        "type": { "type": "string", "enum": TYPES, "description": "Thought type" },
        "source": s("Source tool or context"),
        "source_agent": s("Originating AI agent identifier (e.g. claude-code, cursor, windsurf)"),
        "trust_level": { "type": "string", "enum": TRUST, "description": "Trust level for memory poisoning defense: trusted (default), unverified, or external" },
        "project": s("Project association"),
        "project_id": s("Immutable project UUID"),
        "project_identifier": s("Project registry slug (auto-resolved from cwd if omitted)"),
        "visibility": { "type": "string", "enum": ["personal", "shared"], "description": "Visibility: personal (default) or shared across projects" },
        "topics": string_list("Topic tags", MAX_TOPIC_LENGTH, MAX_TOPICS_COUNT),
        "people": string_list("People mentioned", MAX_PERSON_LENGTH, MAX_PEOPLE_COUNT),
        "metadata": { "type": "object", "additionalProperties": true, "description": "Arbitrary metadata" },
    });
    if with_related {
        p["related_to"] = json!({ "type": "array", "items": { "type": "string" }, "description": "IDs of related thoughts to link" });
    }
    p
}

fn scope_fields(verb: &str) -> Value {
    json!({
        "project_id": project_id(),
        "project_identifier": s("Scope to project slug (use with include_shared to also include shared thoughts; auto-defaulted from cwd when omitted)"),
        "include_shared": b("When project_identifier is set, also include shared thoughts (default true when auto-scoped from cwd)"),
        "shared_only": b("Return only thoughts with visibility=shared"),
        "all_projects": b(&format!("Set true to {verb} across all projects regardless of cwd (disables auto-scoping)")),
    })
}

fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(ao), Value::Object(bo)) = (a.as_object_mut(), b) {
        for (k, v) in bo {
            ao.insert(k, v);
        }
    }
    a
}

pub fn tools() -> Vec<Tool> {
    let mut capture_item = thought_fields(true);
    capture_item["content"] = json!({ "type": "string", "maxLength": MAX_CONTENT_LENGTH });
    capture_item["type"] = s("Thought type");
    let mut capture_schema = object(
        merge(
            thought_fields(true),
            json!({
                "thoughts": { "type": "array", "maxItems": MAX_BULK_THOUGHTS, "description": "Bulk capture: array of thoughts",
                              "items": { "type": "object", "properties": capture_item, "required": ["content", "summary"] } }
            }),
        ),
        &[],
    );
    capture_schema.insert(
        "anyOf".into(),
        json!([
            { "required": ["content", "summary"] },
            { "required": ["thoughts"] }
        ]),
    );
    vec![
        Tool::new(
            "capture_thought",
            "Persist a thought, decision, insight, task, question, or reference to long-term memory. Use whenever something is worth remembering across sessions: architecture choices, user preferences, project goals, bug root causes, or key facts. Supports optional metadata (topics, people, project, source) and bulk capture via the thoughts[] array. Always include a one-line summary so the thought is findable via search_thoughts.",
            capture_schema,
        )
        .with_annotations(annotations(false, false, false)),
        Tool::new(
            "search_thoughts",
            "Search long-term memory by keyword (FTS) or vector similarity, or both (hybrid). Call this before starting any task, making any decision, or when the user asks what you remember about a topic. Returns matching thought summaries with IDs — call get_thought to read full content. Supports graph_depth for GraphRAG-style retrieval: after FTS/vector results are found, traverse N hops of graph edges and include related thoughts in the response.",
            object(merge(json!({
                "query": s("Full-text search query"),
                "embedding": { "type": "array", "items": { "type": "number" }, "description": "Embedding vector for similarity search" },
                "limit": n("Max results (default 20, max 100)"),
                "offset": n("Pagination offset"),
                "type": s("Filter by thought type"),
                "topic": s("Filter by topic"),
                "project": s("Filter by project"),
                "graph_depth": n("Graph traversal depth after retrieval (0 = none, max 5). When >= 1, related thoughts reachable via graph edges are included in graph_related."),
            }), scope_fields("search")), &[]),
        )
        .with_annotations(annotations(false, false, true)),
        Tool::new(
            "list_thoughts",
            "Browse and filter memories using structured fields: type, topic, person, project, source, or date range. Use when you want to enumerate all thoughts of a category (e.g., all decisions for a project, all thoughts mentioning a person, thoughts captured this week) rather than searching by keyword. Complementary to search_thoughts — use list_thoughts to browse by metadata, search_thoughts to find by content.",
            object(merge(json!({
                "type": s("Filter by thought type"),
                "project": s("Filter by project"),
                "topic": s("Filter by topic"),
                "person": s("Filter by person mentioned"),
                "source": s("Filter by source"),
                "source_agent": s("Filter by originating AI agent"),
                "trust_level": { "type": "string", "enum": TRUST, "description": "Filter by trust level" },
                "since": s("ISO 8601 start date"),
                "until": s("ISO 8601 end date"),
                "has_summary": b("Filter by summary presence: true = has summary, false = missing summary"),
                "limit": n("Max results (default 20, max 100)"),
                "offset": n("Pagination offset"),
            }), scope_fields("list")), &[]),
        )
        .with_annotations(annotations(true, false, true)),
        Tool::new(
            "get_thought",
            "Fetch the complete content of a single memory by UUID. Use after search_thoughts or list_thoughts returns a summary that you need to read in full — those tools only return summaries and IDs. Returns all fields: content, summary, type, topics, people, project, source, metadata, and timestamps.",
            object(json!({ "id": s("Thought UUID") }), &["id"]),
        )
        .with_annotations(annotations(false, false, true)),
        Tool::new(
            "update_thought",
            "Modify an existing memory — correct outdated content, add a missing summary, reclassify the type, or update topics and people. Always prefer update_thought over deleting and re-creating. Supports bulk updates by passing multiple IDs in the ids[] array. Use this when the user says something has changed or was saved incorrectly.",
            object(json!({
                "id": s("Single thought ID to update"),
                "ids": { "type": "array", "items": { "type": "string" }, "description": "Multiple thought IDs for bulk update" },
                "content": { "type": "string", "maxLength": MAX_CONTENT_LENGTH, "description": "New content" },
                "summary": { "type": "string", "maxLength": MAX_SUMMARY_LENGTH, "description": "New summary" },
                "type": s("New type"),
                "source": s("New source"),
                "project": s("New project"),
                "project_id": project_id(),
                "project_identifier": s("New project registry slug"),
                "topics": string_list("New topics", MAX_TOPIC_LENGTH, MAX_TOPICS_COUNT),
                "people": string_list("New people", MAX_PERSON_LENGTH, MAX_PEOPLE_COUNT),
                "metadata": { "type": "object", "additionalProperties": true, "description": "New metadata" },
                "visibility": s("New visibility"),
            }), &[]),
        )
        .with_annotations(annotations(false, false, true)),
        Tool::new(
            "delete_thought",
            "Permanently remove a memory and all its relationship edges from the database. Use only for truly obsolete or duplicate entries — prefer update_thought for corrections. Destructive and cannot be undone. Requires the thought's UUID.",
            object(json!({ "id": s("Thought UUID to delete") }), &["id"]),
        )
        .with_annotations(annotations(false, true, true)),
        Tool::new(
            "manage_edges",
            "Create, remove, or expire a typed relationship edge between two memories. Use to build a knowledge graph: connect a decision to the tasks it affects, a reference to the insight it supports, or chain a sequence of thoughts with follows. Use 'expire' to mark an edge as no longer current without deleting it (non-destructive fact resolution). Edge types: refines, cites, refuted_by, tags, related, follows.",
            object(json!({
                "action": { "type": "string", "enum": ["link", "unlink", "expire"], "description": "Action to perform" },
                "source_id": s("Source thought ID (required for link/unlink)"),
                "target_id": s("Target thought ID (required for link/unlink)"),
                "edge_type": { "type": "string", "enum": assistant_memory::edges::VALID_EDGE_TYPES, "description": "Relationship type (required for link/unlink)" },
                "metadata": { "type": "object", "additionalProperties": true, "description": "Edge metadata" },
                "valid_from": s("ISO 8601 start of validity period (optional, for link)"),
                "valid_until": s("ISO 8601 end of validity period (optional, for link/expire)"),
                "edge_id": s("Edge ID (required for expire action)"),
            }), &["action"]),
        )
        .with_annotations(annotations(false, false, true)),
        Tool::new(
            "explore_graph",
            "Traverse the knowledge graph outward from a starting memory, returning all connected thoughts up to a configurable depth (max 5). Use to discover related context for a specific thought — e.g., find all decisions and references linked to a task, trace how an insight connects to supporting references, or understand the full neighborhood around a memory. For combined retrieval + traversal in one call, use search_thoughts with graph_depth instead.",
            object(json!({
                "thought_id": s("Starting thought ID"),
                "max_depth": n("Traversal depth (default 1, max 5)"),
                "edge_types": { "type": "array", "items": { "type": "string" }, "description": "Filter by edge types" },
                "include_expired": b("Include expired edges in traversal (default false)"),
            }), &["thought_id"]),
        )
        .with_annotations(annotations(true, false, true)),
        Tool::new(
            "expand_neighbors",
            "Fetch a memory's full content together with summaries, types, and topics for its immediate graph neighbors. Use when an agent already has a thought ID and needs both that thought and its directly connected context in one call, instead of calling get_thought and explore_graph separately.",
            object(json!({ "thought_id": s("Thought ID to fetch and expand"), "limit": n("Max neighbors to return (default 10, max 100)") }), &["thought_id"]),
        )
        .with_annotations(annotations(true, false, true)),
        Tool::new(
            "thought_stats",
            "Get aggregate statistics about the memory database: total thought count, breakdown by type, top topics and projects, edge count, and recent activity. Use to audit the state of memory, verify that capture_thought calls are persisting, or understand what categories of knowledge have been accumulated.",
            object(json!({}), &[]),
        )
        .with_annotations(annotations(true, false, true)),
        Tool::new(
            "get_brief",
            "Generate a trusted, privacy-filtered, token-bounded project brief for session orientation. Scope 'essentials' returns durable decisions, milestones, blockers, constraints, and preferences; 'recent' returns eligible activity updated in the last 7 days; 'full' (default) returns their deduplicated union.",
            object(json!({
                "scope": { "type": "string", "enum": ["essentials", "recent", "full"], "description": "What to include. Default: full" },
                "project_id": project_id(),
                "project_identifier": s("Project slug to scope the brief to (auto-defaulted from cwd when omitted)"),
                "include_shared": b("Include explicitly brief-eligible shared records with the project brief (default: true)"),
                "all_projects": b("Set true to generate a brief across all projects regardless of cwd (disables auto-scoping)"),
            }), &[]),
        )
        .with_annotations(annotations(true, false, true)),
        Tool::new(
            "select_context",
            "Compose a targeted context payload by filtering thoughts by type, topic, person, and date. Returns a formatted markdown document ready for agent consumption. Use this instead of get_brief when you need specific context rather than a full overview — for example, 'all decisions about auth from the last 30 days' or 'everything mentioning Sarah'. Can optionally prepend an essentials brief header and/or append memory stats.",
            object(json!({
                "types": { "type": "array", "items": { "type": "string" }, "description": "Thought types to include" },
                "topics": { "type": "array", "items": { "type": "string" }, "description": "Topic filters (first topic is applied to the list query)" },
                "people": { "type": "array", "items": { "type": "string" }, "description": "People filters (first person is applied to the list query)" },
                "since": s("ISO 8601 date — only include thoughts created after this date"),
                "project_id": project_id(),
                "project_identifier": s("Scope to project slug (use with include_shared to also include shared thoughts; auto-defaulted from cwd when omitted)"),
                "include_shared": b("When project_identifier is set, also include shared thoughts (default true when auto-scoped from cwd)"),
                "all_projects": b("Set true to compose context across all projects regardless of cwd (disables auto-scoping)"),
                "include_brief": b("Prepend an essentials brief header (default: false)"),
                "include_stats": b("Append a memory stats summary (default: false)"),
                "limit": n("Max thoughts to return (default: 20, max: 100)"),
            }), &[]),
        )
        .with_annotations(annotations(true, false, true)),
    ]
}

pub fn validate_tool_input(name: &str, args: &Value) -> Result<(), String> {
    let Some(tool) = tools().into_iter().find(|tool| tool.name == name) else {
        return Ok(());
    };
    let schema = Value::Object((*tool.input_schema).clone());
    jsonschema::validate(&schema, args)
        .map_err(|validation| format!("arguments do not match the tool schema: {validation}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_tools_with_contract_names_and_annotations() {
        let t = tools();
        let mut names: Vec<&str> = t.iter().map(|x| x.name.as_ref()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "capture_thought",
                "delete_thought",
                "expand_neighbors",
                "explore_graph",
                "get_brief",
                "get_thought",
                "list_thoughts",
                "manage_edges",
                "search_thoughts",
                "select_context",
                "thought_stats",
                "update_thought"
            ]
        );
        for tool in &t {
            let a = tool.annotations.as_ref().unwrap();
            assert!(
                a.read_only_hint.is_some()
                    && a.destructive_hint.is_some()
                    && a.idempotent_hint.is_some()
                    && a.open_world_hint == Some(false),
                "{}",
                tool.name
            );
            assert_eq!(tool.input_schema["type"], "object", "{}", tool.name);
        }
        // §6g: reads that mutate (reinforcement, telemetry) are not read-only
        let by = |n: &str| {
            t.iter()
                .find(|x| x.name == n)
                .unwrap()
                .annotations
                .clone()
                .unwrap()
        };
        assert_eq!(by("get_thought").read_only_hint, Some(false));
        assert_eq!(by("search_thoughts").read_only_hint, Some(false));
        assert_eq!(by("list_thoughts").read_only_hint, Some(true));
        assert_eq!(by("delete_thought").destructive_hint, Some(true));
    }

    #[test]
    fn capture_schema_requires_nonempty_summaries_for_single_and_bulk_items() {
        let tools = tools();
        let capture = tools
            .iter()
            .find(|tool| tool.name == "capture_thought")
            .unwrap();
        let schema = Value::Object((*capture.input_schema).clone());

        assert!(jsonschema::is_valid(
            &schema,
            &json!({ "content": "Single", "summary": "Summary" })
        ));
        assert!(!jsonschema::is_valid(
            &schema,
            &json!({ "content": "Single" })
        ));
        assert!(!jsonschema::is_valid(
            &schema,
            &json!({ "content": "Single", "summary": "" })
        ));
        assert!(jsonschema::is_valid(
            &schema,
            &json!({ "thoughts": [{ "content": "Bulk", "summary": "Summary" }] })
        ));
        assert!(jsonschema::is_valid(
            &schema,
            &json!({ "thoughts": [{ "content": "Bulk", "summary": "Summary", "type": "custom" }] })
        ));
        assert!(!jsonschema::is_valid(
            &schema,
            &json!({ "thoughts": [{ "content": "Bulk" }] })
        ));
    }
}

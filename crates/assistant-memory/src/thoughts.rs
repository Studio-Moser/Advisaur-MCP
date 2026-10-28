//! Thought rows (ADR 0001 §1).
use rusqlite::{Connection, OptionalExtension, Row, params, types::Value};
use serde::{Deserialize, Serialize};
use serde_json::Map;

use crate::error::Result;
use crate::now_iso;
use crate::topics::{canonicalize_topics, topic_like_pattern};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    Trusted,
    Unverified,
    External,
}

impl TrustLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            TrustLevel::Trusted => "trusted",
            TrustLevel::Unverified => "unverified",
            TrustLevel::External => "external",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "trusted" => Some(Self::Trusted),
            "unverified" => Some(Self::Unverified),
            "external" => Some(Self::External),
            _ => None,
        }
    }
    /// Ranking used by the trust gate (§6d): trusted > unverified > external.
    pub fn rank(self) -> u8 {
        match self {
            TrustLevel::Trusted => 2,
            TrustLevel::Unverified => 1,
            TrustLevel::External => 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThoughtInput {
    pub content: String,
    pub summary: Option<String>,
    pub r#type: Option<String>,
    pub source: Option<String>,
    pub source_agent: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub project: Option<String>,
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub topics: Option<Vec<String>>,
    pub people: Option<Vec<String>>,
    pub visibility: Option<String>,
    pub metadata: Option<Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThoughtRecord {
    pub id: String,
    pub content: String,
    pub summary: Option<String>,
    pub r#type: String,
    pub source: String,
    pub source_agent: Option<String>,
    pub trust_level: TrustLevel,
    pub project: Option<String>,
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub topics: Vec<String>,
    pub people: Vec<String>,
    pub visibility: String,
    pub metadata: Option<Map<String, serde_json::Value>>,
    #[serde(skip)]
    pub embedding: Option<Vec<u8>>,
    pub created_at: String,
    pub updated_at: String,
    pub consolidated_into: Option<String>,
    pub reinforcement_count: i64,
    pub last_confirmed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThoughtSummary {
    pub id: String,
    pub summary: Option<String>,
    pub r#type: String,
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub topics: Vec<String>,
    pub created_at: String,
}

/// Partial update; `Some(_)` fields are written, `None` fields untouched.
#[derive(Debug, Clone, Default)]
pub struct ThoughtUpdate {
    pub content: Option<String>,
    pub summary: Option<String>,
    pub r#type: Option<String>,
    pub source: Option<String>,
    pub source_agent: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub project: Option<String>,
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub topics: Option<Vec<String>>,
    pub people: Option<Vec<String>>,
    pub visibility: Option<String>,
    pub metadata: Option<Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    pub r#type: Option<String>,
    pub project: Option<String>,
    pub project_id: Option<String>,
    pub include_shared: bool,
    pub shared_only: bool,
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

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ListResult {
    pub results: Vec<ThoughtSummary>,
    pub total_count: i64,
    pub has_more: bool,
    pub offset: i64,
}

pub fn parse_json_array(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else { return vec![] };
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => vec![],
    }
}

pub fn parse_json_object(raw: Option<&str>) -> Option<Map<String, serde_json::Value>> {
    match serde_json::from_str::<serde_json::Value>(raw?) {
        Ok(serde_json::Value::Object(m)) => Some(m),
        _ => None,
    }
}

const PROJECT_IDENTIFIER_EXPR: &str = "COALESCE((SELECT current_slug FROM projects WHERE projects.project_id = thoughts.project_id), thoughts.project_identifier)";

fn row_to_record(r: &Row) -> rusqlite::Result<ThoughtRecord> {
    let topics: Option<String> = r.get("topics")?;
    let people: Option<String> = r.get("people")?;
    let metadata: Option<String> = r.get("metadata")?;
    let trust: Option<String> = r.get("trust_level")?;
    Ok(ThoughtRecord {
        id: r.get("id")?,
        content: r.get("content")?,
        summary: r.get("summary")?,
        r#type: r.get("type")?,
        source: r.get("source")?,
        source_agent: r.get("source_agent")?,
        trust_level: trust
            .as_deref()
            .and_then(TrustLevel::parse)
            .unwrap_or(TrustLevel::Unverified),
        project: r.get("project")?,
        project_id: r.get("project_id")?,
        project_identifier: r.get("project_identifier")?,
        topics: parse_json_array(topics.as_deref()),
        people: parse_json_array(people.as_deref()),
        visibility: r.get("visibility")?,
        metadata: parse_json_object(metadata.as_deref()),
        embedding: r.get("embedding")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        consolidated_into: r.get("consolidated_into")?,
        reinforcement_count: r.get("reinforcement_count")?,
        last_confirmed_at: r.get("last_confirmed_at")?,
    })
}

pub(crate) fn row_to_summary(r: &Row) -> rusqlite::Result<ThoughtSummary> {
    let topics: Option<String> = r.get("topics")?;
    Ok(ThoughtSummary {
        id: r.get("id")?,
        summary: r.get("summary")?,
        r#type: r.get("type")?,
        project_id: r.get("project_id")?,
        project_identifier: r.get("project_identifier")?,
        topics: parse_json_array(topics.as_deref()),
        created_at: r.get("created_at")?,
    })
}

pub fn insert_thought(conn: &Connection, input: &ThoughtInput) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_iso();
    // A slug alias resolves to (project_id, current_slug) when no explicit project_id was given.
    let alias: Option<(String, String)> = match (&input.project_id, &input.project_identifier) {
        (None, Some(slug)) => conn
            .query_row(
                "SELECT projects.project_id, projects.current_slug FROM project_slug_aliases
                 JOIN projects USING (project_id) WHERE project_slug_aliases.slug = ?1",
                params![slug],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
        _ => None,
    };
    let topics = input
        .topics
        .as_ref()
        .map(|t| serde_json::to_string(&canonicalize_topics(t)))
        .transpose()?;
    let people = input
        .people
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let metadata = input
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    conn.execute(
        "INSERT INTO thoughts (id, content, summary, type, source, source_agent, trust_level, project, project_id,
           project_identifier, topics, people, visibility, metadata, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            id,
            input.content,
            input.summary,
            input.r#type.as_deref().unwrap_or("note"),
            input.source.as_deref().unwrap_or("unknown"),
            input.source_agent,
            input.trust_level.unwrap_or(TrustLevel::Trusted).as_str(),
            input.project,
            input.project_id.clone().or_else(|| alias.as_ref().map(|a| a.0.clone())),
            alias.as_ref().map(|a| a.1.clone()).or_else(|| input.project_identifier.clone()),
            topics,
            people,
            input.visibility.as_deref().unwrap_or("personal"),
            metadata,
            now,
            now,
        ],
    )?;
    Ok(id)
}

pub fn get_thought(conn: &Connection, id: &str) -> Result<Option<ThoughtRecord>> {
    let sql = format!(
        "SELECT thoughts.*, {PROJECT_IDENTIFIER_EXPR} AS project_identifier FROM thoughts WHERE id = ?1"
    );
    Ok(conn
        .query_row(&sql, params![id], row_to_record)
        .optional()?)
}

pub fn thought_exists(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM thoughts WHERE id = ?1", params![id], |_| {
            Ok(())
        })
        .optional()?
        .is_some())
}

/// Bump `reinforcement_count`; with `confirm`, also stamp `last_confirmed_at` (§6f).
pub fn increment_reinforcement(
    conn: &Connection,
    id: &str,
    amount: i64,
    confirm: bool,
) -> Result<bool> {
    let now = now_iso();
    let changed = conn.execute(
        "UPDATE thoughts SET reinforcement_count = reinforcement_count + ?1, updated_at = ?2,
           last_confirmed_at = CASE WHEN ?3 = 1 THEN ?2 ELSE last_confirmed_at END
         WHERE id = ?4",
        params![amount, now, confirm as i64, id],
    )?;
    Ok(changed > 0)
}

pub fn update_thought(conn: &Connection, id: &str, u: &ThoughtUpdate) -> Result<bool> {
    let mut sets: Vec<&str> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    macro_rules! set {
        ($col:literal, $val:expr) => {
            if let Some(v) = $val {
                sets.push(concat!($col, " = ?"));
                vals.push(v);
            }
        };
    }
    set!("content", u.content.clone().map(Value::Text));
    set!("summary", u.summary.clone().map(Value::Text));
    set!("type", u.r#type.clone().map(Value::Text));
    set!("source", u.source.clone().map(Value::Text));
    set!("project", u.project.clone().map(Value::Text));
    set!("project_id", u.project_id.clone().map(Value::Text));
    set!(
        "project_identifier",
        u.project_identifier.clone().map(Value::Text)
    );
    set!(
        "topics",
        u.topics
            .as_ref()
            .map(|t| serde_json::to_string(&canonicalize_topics(t)))
            .transpose()?
            .map(Value::Text)
    );
    set!(
        "people",
        u.people
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?
            .map(Value::Text)
    );
    set!("visibility", u.visibility.clone().map(Value::Text));
    set!(
        "metadata",
        u.metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?
            .map(Value::Text)
    );
    set!("source_agent", u.source_agent.clone().map(Value::Text));
    set!(
        "trust_level",
        u.trust_level.map(|t| Value::Text(t.as_str().to_string()))
    );
    if sets.is_empty() {
        return Ok(false);
    }
    sets.push("updated_at = ?");
    vals.push(Value::Text(now_iso()));
    vals.push(Value::Text(id.to_string()));
    let sql = format!("UPDATE thoughts SET {} WHERE id = ?", sets.join(", "));
    Ok(conn.execute(&sql, rusqlite::params_from_iter(vals))? > 0)
}

pub fn delete_thought(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM thoughts WHERE id = ?1", params![id])? > 0)
}

pub fn list_thoughts(conn: &Connection, o: &ListOptions) -> Result<ListResult> {
    let mut wheres: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    let mut filters: Vec<(&str, Value)> = Vec::new();
    if let Some(t) = &o.r#type {
        filters.push(("type = ?", Value::Text(t.clone())));
    }
    if let Some(p) = &o.project {
        filters.push(("project = ?", Value::Text(p.clone())));
    }
    if o.shared_only {
        filters.push(("visibility = 'shared'", Value::Null));
    }
    if let Some(pid) = &o.project_id {
        let clause = if o.include_shared {
            "(project_id = ? OR visibility = 'shared')"
        } else {
            "project_id = ?"
        };
        filters.push((clause, Value::Text(pid.clone())));
    }
    if let Some(t) = &o.topic {
        filters.push(("topics LIKE ?", Value::Text(topic_like_pattern(t))));
    }
    if let Some(p) = &o.person {
        filters.push(("people LIKE ?", Value::Text(format!("%\"{p}\"%"))));
    }
    if let Some(s) = &o.source {
        filters.push(("source = ?", Value::Text(s.clone())));
    }
    if let Some(s) = &o.source_agent {
        filters.push(("source_agent = ?", Value::Text(s.clone())));
    }
    if let Some(t) = o.trust_level {
        filters.push(("trust_level = ?", Value::Text(t.as_str().into())));
    }
    if let Some(s) = &o.since {
        filters.push(("created_at >= ?", Value::Text(s.clone())));
    }
    if let Some(u) = &o.until {
        filters.push(("created_at <= ?", Value::Text(u.clone())));
    }
    match o.has_summary {
        Some(true) => filters.push(("summary IS NOT NULL AND summary != ''", Value::Null)),
        Some(false) => filters.push(("(summary IS NULL OR summary = '')", Value::Null)),
        None => {}
    }
    for (clause, value) in filters {
        wheres.push(clause.to_string());
        if clause.contains('?') {
            vals.push(value);
        }
    }
    let where_sql = if wheres.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", wheres.join(" AND "))
    };
    let limit = o.limit.unwrap_or(20).clamp(1, 100);
    let offset = o.offset.unwrap_or(0);

    let total_count: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM thoughts {where_sql}"),
        rusqlite::params_from_iter(vals.iter()),
        |r| r.get(0),
    )?;
    let sql = format!(
        "SELECT id, summary, type, project_id, {PROJECT_IDENTIFIER_EXPR} AS project_identifier, topics, created_at
         FROM thoughts {where_sql} ORDER BY created_at DESC LIMIT ? OFFSET ?"
    );
    let mut all = vals.clone();
    all.push(Value::Integer(limit));
    all.push(Value::Integer(offset));
    let mut stmt = conn.prepare(&sql)?;
    let results: Vec<ThoughtSummary> = stmt
        .query_map(rusqlite::params_from_iter(all), row_to_summary)?
        .collect::<std::result::Result<_, _>>()?;
    let has_more = offset + (results.len() as i64) < total_count;
    Ok(ListResult {
        results,
        total_count,
        has_more,
        offset,
    })
}

pub fn count_thoughts(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM thoughts", [], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;

    fn input(content: &str) -> ThoughtInput {
        ThoughtInput {
            content: content.into(),
            ..Default::default()
        }
    }

    #[test]
    fn insert_defaults_and_roundtrip() {
        let m = Memory::open_in_memory().unwrap();
        let id = insert_thought(
            &m.conn,
            &ThoughtInput {
                topics: Some(vec!["Knowledge Graph".into()]),
                people: Some(vec!["Tim".into()]),
                metadata: Some(serde_json::from_str(r#"{"z":1,"a":{"extra":true}}"#).unwrap()),
                ..input("hello world")
            },
        )
        .unwrap();
        let t = get_thought(&m.conn, &id).unwrap().unwrap();
        assert_eq!(t.r#type, "note");
        assert_eq!(t.source, "unknown");
        assert_eq!(t.trust_level, TrustLevel::Trusted);
        assert_eq!(t.visibility, "personal");
        assert_eq!(t.topics, vec!["knowledge-graph"]);
        assert_eq!(t.people, vec!["Tim"]);
        assert_eq!(t.reinforcement_count, 0);
        assert_eq!(t.created_at, t.updated_at);
        // metadata key order preserved as written (JS parity)
        let raw: String = m
            .conn
            .query_row(
                "SELECT metadata FROM thoughts WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, r#"{"z":1,"a":{"extra":true}}"#);
        assert!(get_thought(&m.conn, "missing").unwrap().is_none());
    }

    #[test]
    fn alias_resolves_project_id_on_insert() {
        let m = Memory::open_in_memory().unwrap();
        m.conn.execute_batch(
            "INSERT INTO projects (slug, display_name, created_at, updated_at, project_id, current_slug, identity_state)
             VALUES ('old','Shelby','x','x','4abbc729-70e8-5120-9cfc-bd34739c024e','shelby','local_only');
             INSERT INTO project_slug_aliases (slug, project_id, status, claimed_at) VALUES ('old','4abbc729-70e8-5120-9cfc-bd34739c024e','retired','x');
             INSERT INTO project_slug_aliases (slug, project_id, status, claimed_at) VALUES ('shelby','4abbc729-70e8-5120-9cfc-bd34739c024e','current','x');",
        ).unwrap();
        let id = insert_thought(
            &m.conn,
            &ThoughtInput {
                project_identifier: Some("old".into()),
                ..input("x")
            },
        )
        .unwrap();
        let t = get_thought(&m.conn, &id).unwrap().unwrap();
        assert_eq!(
            t.project_id.as_deref(),
            Some("4abbc729-70e8-5120-9cfc-bd34739c024e")
        );
        assert_eq!(t.project_identifier.as_deref(), Some("shelby"));
    }

    #[test]
    fn update_reinforce_delete() {
        let m = Memory::open_in_memory().unwrap();
        let id = insert_thought(&m.conn, &input("a b c")).unwrap();
        assert!(!update_thought(&m.conn, &id, &ThoughtUpdate::default()).unwrap());
        assert!(
            update_thought(
                &m.conn,
                &id,
                &ThoughtUpdate {
                    summary: Some("s".into()),
                    trust_level: Some(TrustLevel::External),
                    ..Default::default()
                }
            )
            .unwrap()
        );
        assert!(increment_reinforcement(&m.conn, &id, 1, true).unwrap());
        let t = get_thought(&m.conn, &id).unwrap().unwrap();
        assert_eq!(t.summary.as_deref(), Some("s"));
        assert_eq!(t.trust_level, TrustLevel::External);
        assert_eq!(t.reinforcement_count, 1);
        assert!(t.last_confirmed_at.is_some());
        assert!(delete_thought(&m.conn, &id).unwrap());
        assert!(!delete_thought(&m.conn, &id).unwrap());
    }

    #[test]
    fn list_filters_and_pagination() {
        let m = Memory::open_in_memory().unwrap();
        for i in 0..5 {
            insert_thought(
                &m.conn,
                &ThoughtInput {
                    r#type: Some(if i % 2 == 0 {
                        "decision".into()
                    } else {
                        "note".into()
                    }),
                    topics: Some(vec!["Auth Flow".into()]),
                    summary: if i == 0 { None } else { Some(format!("s{i}")) },
                    visibility: Some(if i == 4 {
                        "shared".into()
                    } else {
                        "personal".into()
                    }),
                    project_id: Some(if i == 4 { "other".into() } else { "p1".into() }),
                    ..input(&format!("thought {i}"))
                },
            )
            .unwrap();
        }
        let r = list_thoughts(
            &m.conn,
            &ListOptions {
                r#type: Some("decision".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.total_count, 3);
        let r = list_thoughts(
            &m.conn,
            &ListOptions {
                topic: Some("auth-flow".into()),
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((r.total_count, r.results.len(), r.has_more), (5, 2, true));
        let r = list_thoughts(
            &m.conn,
            &ListOptions {
                has_summary: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.total_count, 1);
        let r = list_thoughts(
            &m.conn,
            &ListOptions {
                project_id: Some("p1".into()),
                include_shared: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.total_count, 5);
        let r = list_thoughts(
            &m.conn,
            &ListOptions {
                project_id: Some("p1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.total_count, 4);
    }
}

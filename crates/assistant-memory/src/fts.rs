//! FTS5 keyword search (ADR 0001 §7): porter unicode61, BM25 rank.
use regex::Regex;
use rusqlite::{Connection, types::Value};
use serde::Serialize;
use std::sync::LazyLock;

use crate::error::Result;
use crate::thoughts::parse_json_array;
use crate::topics::topic_like_pattern;

static SPECIAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"["*()+\-^:{}\[\]]"#).unwrap());
static KEYWORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(AND|OR|NOT|NEAR)\b").unwrap());

/// Turn free text into a safe prefix-match FTS5 query; empty when nothing searchable remains.
pub fn sanitize_fts_query(query: &str) -> String {
    let s = SPECIAL.replace_all(query, " ");
    let s = KEYWORDS.replace_all(&s, " ");
    s.split_whitespace()
        .map(|t| format!("\"{t}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchResult {
    pub id: String,
    pub summary: Option<String>,
    pub r#type: String,
    pub project_id: Option<String>,
    pub project_identifier: Option<String>,
    pub topics: Vec<String>,
    pub created_at: String,
    pub rank: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub query: String,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub r#type: Option<String>,
    pub project: Option<String>,
    pub project_id: Option<String>,
    pub include_shared: bool,
    pub shared_only: bool,
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchListResult {
    pub results: Vec<SearchResult>,
    pub total_count: i64,
    pub has_more: bool,
    pub offset: i64,
}

pub fn search_thoughts(conn: &Connection, o: &SearchOptions) -> Result<SearchListResult> {
    let sanitized = sanitize_fts_query(&o.query);
    if sanitized.is_empty() {
        return Ok(SearchListResult {
            results: vec![],
            total_count: 0,
            has_more: false,
            offset: 0,
        });
    }
    let limit = o.limit.unwrap_or(20).clamp(1, 100);
    let offset = o.offset.unwrap_or(0);
    let mut wheres = vec!["thoughts_fts MATCH ?".to_string()];
    let mut vals = vec![Value::Text(sanitized)];
    if let Some(t) = &o.r#type {
        wheres.push("t.type = ?".into());
        vals.push(Value::Text(t.clone()));
    }
    if let Some(p) = &o.project {
        wheres.push("t.project = ?".into());
        vals.push(Value::Text(p.clone()));
    }
    if let Some(t) = &o.topic {
        wheres.push("t.topics LIKE ?".into());
        vals.push(Value::Text(topic_like_pattern(t)));
    }
    if o.shared_only {
        wheres.push("t.visibility = 'shared'".into());
    }
    if let Some(pid) = &o.project_id {
        wheres.push(
            if o.include_shared {
                "(t.project_id = ? OR t.visibility = 'shared')"
            } else {
                "t.project_id = ?"
            }
            .into(),
        );
        vals.push(Value::Text(pid.clone()));
    }
    let where_sql = wheres.join(" AND ");
    let total_count: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM thoughts_fts JOIN thoughts t ON thoughts_fts.rowid = t.rowid WHERE {where_sql}"),
        rusqlite::params_from_iter(vals.iter()),
        |r| r.get(0),
    )?;
    let sql = format!(
        "SELECT t.id, t.summary, t.type, t.project_id,
           COALESCE((SELECT current_slug FROM projects WHERE projects.project_id = t.project_id), t.project_identifier) AS project_identifier,
           t.topics, t.created_at, rank
         FROM thoughts_fts JOIN thoughts t ON thoughts_fts.rowid = t.rowid
         WHERE {where_sql} ORDER BY rank LIMIT ? OFFSET ?"
    );
    vals.push(Value::Integer(limit));
    vals.push(Value::Integer(offset));
    let mut stmt = conn.prepare(&sql)?;
    let results: Vec<SearchResult> = stmt
        .query_map(rusqlite::params_from_iter(vals), |r| {
            let topics: Option<String> = r.get(5)?;
            let rank: f64 = r.get(7)?;
            Ok(SearchResult {
                id: r.get(0)?,
                summary: r.get(1)?,
                r#type: r.get(2)?,
                project_id: r.get(3)?,
                project_identifier: r.get(4)?,
                topics: parse_json_array(topics.as_deref()),
                created_at: r.get(6)?,
                rank: -rank, // higher = more relevant
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    let has_more = offset + (results.len() as i64) < total_count;
    Ok(SearchListResult {
        results,
        total_count,
        has_more,
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;
    use crate::thoughts::{ThoughtInput, insert_thought};

    #[test]
    fn sanitize_matches_ts_behavior() {
        assert_eq!(
            sanitize_fts_query(r#"hello "world" (x)"#),
            r#""hello"* "world"* "x"*"#
        );
        assert_eq!(
            sanitize_fts_query("cats AND dogs NOT android"),
            r#""cats"* "dogs"* "android"*"#
        );
        assert_eq!(sanitize_fts_query("  *()  "), "");
        assert_eq!(
            sanitize_fts_query("AND."),
            r#"".""#.to_owned() + "*",
            "keyword removed at a word boundary, punctuation remains"
        );
    }

    #[test]
    fn search_ranks_filters_and_paginates() {
        let m = Memory::open_in_memory().unwrap();
        let mk = |content: &str, ty: &str, pid: &str, vis: &str| {
            insert_thought(
                &m.conn,
                &ThoughtInput {
                    content: content.into(),
                    r#type: Some(ty.into()),
                    project_id: Some(pid.into()),
                    visibility: Some(vis.into()),
                    summary: Some("sum".into()),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let a = mk("rust rust rust memory engine", "decision", "p1", "personal");
        mk("a rust note", "note", "p1", "personal");
        mk("rust in another project", "note", "p2", "shared");
        mk("rust private elsewhere", "note", "p2", "personal");
        let r = search_thoughts(
            &m.conn,
            &SearchOptions {
                query: "rust".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.total_count, 4);
        assert_eq!(r.results[0].id, a, "most repeated term ranks first");
        assert!(r.results[0].rank > r.results[1].rank);
        let r = search_thoughts(
            &m.conn,
            &SearchOptions {
                query: "rust".into(),
                project_id: Some("p1".into()),
                include_shared: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.total_count, 3);
        let r = search_thoughts(
            &m.conn,
            &SearchOptions {
                query: "rust".into(),
                project_id: Some("p1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.total_count, 2);
        let r = search_thoughts(
            &m.conn,
            &SearchOptions {
                query: "rust".into(),
                limit: Some(1),
                offset: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((r.results.len(), r.has_more), (1, false));
        let r = search_thoughts(
            &m.conn,
            &SearchOptions {
                query: "nothing-here".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.total_count, 0);
    }
}

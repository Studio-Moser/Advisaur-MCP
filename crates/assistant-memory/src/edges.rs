//! Typed edges and graph traversal (ADR 0001 §§4, 5, 7).
use rusqlite::{Connection, OptionalExtension, params, types::Value};
use serde::Serialize;
use serde_json::Map;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::{Error, Result};
use crate::now_iso;

pub const VALID_EDGE_TYPES: [&str; 6] = [
    "refines",
    "cites",
    "refuted_by",
    "tags",
    "related",
    "follows",
];

/// UTF-8 byte bound shared by accepted import identities and graph cursors.
/// Historical full-record readers retain their existing unconstrained ID API.
pub const MAX_EDGE_ID_BYTES: usize = 512;

pub fn is_valid_edge_type(t: &str) -> bool {
    VALID_EDGE_TYPES.contains(&t)
}

#[derive(Debug, Clone, Default)]
pub struct EdgeInput {
    pub source_id: String,
    pub target_id: String,
    pub edge_type: String,
    pub metadata: Option<Map<String, serde_json::Value>>,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EdgeRecord {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub edge_type: String,
    pub metadata: Option<Map<String, serde_json::Value>>,
    pub created_at: String,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConnectedThought {
    pub thought_id: String,
    pub summary: Option<String>,
    pub r#type: String,
    pub edge_id: String,
    pub edge_type: String,
    pub direction: Direction,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphEdge {
    pub edge_id: String,
    pub edge_type: String,
    pub connected_to: String,
    pub direction: Direction,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub summary: Option<String>,
    pub r#type: String,
    pub depth: i64,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphRelatedThought {
    pub id: String,
    pub summary: Option<String>,
    pub r#type: String,
    pub depth: i64,
    pub via_edge_type: String,
    pub direction: Direction,
}

/// (neighbor_id, edge_id, edge_type, neighbor_summary, neighbor_type, direction)
type Hop = (String, String, String, Option<String>, String, Direction);

use crate::temporal::{self, ACTIVE_SQL};

pub fn link_thoughts(conn: &Connection, input: &EdgeInput) -> Result<String> {
    for (field, value) in [
        ("valid_from", input.valid_from.as_deref()),
        ("valid_until", input.valid_until.as_deref()),
    ] {
        if let Some(value) = value {
            temporal::parse_bound(value)
                .map_err(|error| Error::InvalidInput(format!("{field}: {error}")))?;
        }
    }
    if !is_valid_edge_type(&input.edge_type) {
        return Err(Error::InvalidInput(format!(
            "Invalid edge type \"{}\". Must be one of: {}",
            input.edge_type,
            VALID_EDGE_TYPES.join(", ")
        )));
    }
    for (label, id) in [("Source", &input.source_id), ("Target", &input.target_id)] {
        if !crate::thoughts::thought_exists(conn, id)? {
            return Err(Error::NotFound(format!(
                "{label} thought \"{id}\" does not exist"
            )));
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let metadata = input
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let res = conn.execute(
        "INSERT INTO edges (id, source_id, target_id, edge_type, metadata, created_at, valid_from, valid_until)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, input.source_id, input.target_id, input.edge_type, metadata, now_iso(), input.valid_from, input.valid_until],
    );
    match res {
        Ok(_) => Ok(id),
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(Error::Duplicate(format!(
                "Edge already exists: {} -[{}]-> {}",
                input.source_id, input.edge_type, input.target_id
            )))
        }
        Err(e) => Err(e.into()),
    }
}

pub fn unlink_thoughts(
    conn: &Connection,
    source_id: &str,
    target_id: &str,
    edge_type: &str,
) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM edges WHERE source_id = ?1 AND target_id = ?2 AND edge_type = ?3",
        params![source_id, target_id, edge_type],
    )? > 0)
}

pub fn expire_edge(conn: &Connection, edge_id: &str, valid_until: Option<&str>) -> Result<()> {
    let ts = valid_until.map(str::to_owned).unwrap_or_else(now_iso);
    temporal::parse_bound(&ts)
        .map_err(|error| Error::InvalidInput(format!("valid_until: {error}")))?;
    if conn.execute(
        "UPDATE edges SET valid_until = ?1 WHERE id = ?2",
        params![ts, edge_id],
    )? == 0
    {
        return Err(Error::NotFound(format!(
            "Edge \"{edge_id}\" does not exist"
        )));
    }
    Ok(())
}

fn row_to_edge(r: &rusqlite::Row) -> rusqlite::Result<EdgeRecord> {
    let metadata: Option<String> = r.get("metadata")?;
    Ok(EdgeRecord {
        id: r.get("id")?,
        source_id: r.get("source_id")?,
        target_id: r.get("target_id")?,
        edge_type: r.get("edge_type")?,
        metadata: crate::thoughts::parse_json_object(metadata.as_deref()),
        created_at: r.get("created_at")?,
        valid_from: r.get("valid_from")?,
        valid_until: r.get("valid_until")?,
    })
}

pub fn get_edge(conn: &Connection, id: &str) -> Result<Option<EdgeRecord>> {
    Ok(conn
        .query_row(
            "SELECT * FROM edges WHERE id = ?1",
            params![id],
            row_to_edge,
        )
        .optional()?)
}

pub fn get_edges_between(
    conn: &Connection,
    source_id: &str,
    target_id: &str,
) -> Result<Vec<EdgeRecord>> {
    let mut stmt = conn.prepare("SELECT * FROM edges WHERE source_id = ?1 AND target_id = ?2")?;
    let rows = stmt.query_map(params![source_id, target_id], row_to_edge)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn type_filter(edge_types: Option<&[String]>) -> (String, Vec<Value>) {
    match edge_types {
        Some(types) if !types.is_empty() => (
            format!("AND e.edge_type IN ({})", vec!["?"; types.len()].join(", ")),
            types.iter().map(|t| Value::Text(t.clone())).collect(),
        ),
        _ => (String::new(), vec![]),
    }
}

/// One hop: (neighbor_id, edge_id, edge_type, summary, type, direction) for both directions.
fn neighbors(
    conn: &Connection,
    id: &str,
    type_sql: &str,
    type_vals: &[Value],
    temporal_sql: &str,
    now: Option<&str>,
) -> Result<Vec<Hop>> {
    let mut out = Vec::new();
    for (direction, join_col, where_col) in [
        (Direction::Outgoing, "target_id", "source_id"),
        (Direction::Incoming, "source_id", "target_id"),
    ] {
        let sql = format!(
            "SELECT e.id, e.edge_type, e.{join_col}, t.summary, t.type FROM edges e
             JOIN thoughts t ON t.id = e.{join_col} WHERE e.{where_col} = ? {temporal_sql} {type_sql}"
        );
        let mut vals = vec![Value::Text(id.to_string())];
        if let Some(now) = now {
            vals.push(Value::Text(now.into()));
        }
        vals.extend(type_vals.iter().cloned());
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(vals), |r| {
            Ok((
                r.get::<_, String>(2)?,
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (nid, eid, et, summary, ty) = row?;
            out.push((nid, eid, et, summary, ty, direction));
        }
    }
    Ok(out)
}

pub fn get_connections(
    conn: &Connection,
    thought_id: &str,
    edge_types: Option<&[String]>,
) -> Result<Vec<ConnectedThought>> {
    get_connections_at(conn, thought_id, edge_types, &now_iso())
}

/// Active connections evaluated at one explicit RFC3339 instant.
pub fn get_connections_at(
    conn: &Connection,
    thought_id: &str,
    edge_types: Option<&[String]>,
    now: &str,
) -> Result<Vec<ConnectedThought>> {
    temporal::parse_now(now)?;
    temporal::ensure_sql_function(conn)?;
    let (type_sql, type_vals) = type_filter(edge_types);
    let temporal = format!("AND {ACTIVE_SQL}");
    Ok(neighbors(
        conn,
        thought_id,
        &type_sql,
        &type_vals,
        &temporal,
        Some(now),
    )?
    .into_iter()
    .map(|(nid, eid, et, summary, ty, direction)| ConnectedThought {
        thought_id: nid,
        summary,
        r#type: ty,
        edge_id: eid,
        edge_type: et,
        direction,
    })
    .collect())
}

/// BFS from every result id up to `graph_depth` hops (clamped 1..=5), excluding the seeds.
pub fn fetch_graph_related(
    conn: &Connection,
    result_ids: &[String],
    graph_depth: i64,
) -> Result<Vec<GraphRelatedThought>> {
    fetch_graph_related_at(conn, result_ids, graph_depth, &now_iso())
}

/// Graph expansion uses the same explicit instant for every hop.
pub fn fetch_graph_related_at(
    conn: &Connection,
    result_ids: &[String],
    graph_depth: i64,
    now: &str,
) -> Result<Vec<GraphRelatedThought>> {
    temporal::parse_now(now)?;
    if graph_depth <= 0 || result_ids.is_empty() {
        return Ok(vec![]);
    }
    temporal::ensure_sql_function(conn)?;
    let depth_cap = graph_depth.clamp(1, 5);
    let mut seen: HashSet<String> = result_ids.iter().cloned().collect();
    let mut related = Vec::new();
    let mut queue: VecDeque<(String, i64)> = result_ids.iter().map(|id| (id.clone(), 0)).collect();
    let temporal = format!("AND {ACTIVE_SQL}");
    while let Some((id, depth)) = queue.pop_front() {
        if depth >= depth_cap {
            continue;
        }
        for (nid, _eid, et, summary, ty, direction) in
            neighbors(conn, &id, "", &[], &temporal, Some(now))?
        {
            if seen.insert(nid.clone()) {
                related.push(GraphRelatedThought {
                    id: nid.clone(),
                    summary,
                    r#type: ty,
                    depth: depth + 1,
                    via_edge_type: et,
                    direction,
                });
                queue.push_back((nid, depth + 1));
            }
        }
    }
    Ok(related)
}

/// BFS from one thought (depth clamped 0..=5). Empty when the root doesn't exist.
pub fn traverse_graph(
    conn: &Connection,
    thought_id: &str,
    max_depth: i64,
    edge_types: Option<&[String]>,
    include_expired: bool,
) -> Result<Vec<GraphNode>> {
    traverse_graph_at(
        conn,
        thought_id,
        max_depth,
        edge_types,
        include_expired,
        &now_iso(),
    )
}

/// Explicit-time traversal; include_expired preserves all recorded relationships.
pub fn traverse_graph_at(
    conn: &Connection,
    thought_id: &str,
    max_depth: i64,
    edge_types: Option<&[String]>,
    include_expired: bool,
    now: &str,
) -> Result<Vec<GraphNode>> {
    temporal::parse_now(now)?;
    if !include_expired {
        temporal::ensure_sql_function(conn)?;
    }
    let depth_cap = max_depth.clamp(0, 5);
    let Some((summary, ty)) = conn
        .query_row(
            "SELECT summary, type FROM thoughts WHERE id = ?1",
            params![thought_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    else {
        return Ok(vec![]);
    };
    let (type_sql, type_vals) = type_filter(edge_types);
    let temporal = if include_expired {
        String::new()
    } else {
        format!("AND {ACTIVE_SQL}")
    };
    let mut order: Vec<String> = vec![thought_id.to_string()];
    let mut visited: HashMap<String, GraphNode> = HashMap::new();
    visited.insert(
        thought_id.to_string(),
        GraphNode {
            id: thought_id.to_string(),
            summary,
            r#type: ty,
            depth: 0,
            edges: vec![],
        },
    );
    let mut queue: VecDeque<(String, i64)> = VecDeque::from([(thought_id.to_string(), 0)]);
    while let Some((id, depth)) = queue.pop_front() {
        if depth >= depth_cap {
            continue;
        }
        for (nid, eid, et, summary, ty, direction) in neighbors(
            conn,
            &id,
            &type_sql,
            &type_vals,
            &temporal,
            (!include_expired).then_some(now),
        )? {
            visited
                .get_mut(&id)
                .expect("current node visited")
                .edges
                .push(GraphEdge {
                    edge_id: eid,
                    edge_type: et,
                    connected_to: nid.clone(),
                    direction,
                });
            if !visited.contains_key(&nid) {
                visited.insert(
                    nid.clone(),
                    GraphNode {
                        id: nid.clone(),
                        summary,
                        r#type: ty,
                        depth: depth + 1,
                        edges: vec![],
                    },
                );
                order.push(nid.clone());
                queue.push_back((nid, depth + 1));
            }
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|id| visited.remove(&id))
        .collect())
}

/// Canonical validity check for a previously loaded record; does not authorize its endpoints.
pub fn is_active_at(edge: &EdgeRecord, now: &str) -> Result<bool> {
    temporal::active_at(edge.valid_from.as_deref(), edge.valid_until.as_deref(), now)
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveEdgePage {
    pub edges: Vec<EdgeRecord>,
    pub next_after_id: Option<String>,
}

/// Scan 1..=200 candidates in binary ID order and return their active records.
/// Empty pages may have continuation. Keep type/instant/scope fixed while paging.
/// This bounds rows, not historic metadata bytes, and is not snapshot isolation:
/// concurrent inserts before the cursor appear after a refresh. Callers authorize
/// both endpoints before exposing records; no global count is implied.
pub fn active_edges_page(
    conn: &Connection,
    edge_type: &str,
    now: &str,
    after_id: Option<&str>,
    page_size: usize,
) -> Result<ActiveEdgePage> {
    temporal::parse_now(now)?;
    if !(1..=200).contains(&page_size)
        || !is_valid_edge_type(edge_type)
        || after_id.is_some_and(|id| id.len() > MAX_EDGE_ID_BYTES)
    {
        return Err(Error::InvalidInput(format!(
            "Use a valid edge type, page size 1–200 and cursor up to {MAX_EDGE_ID_BYTES} bytes"
        )));
    }
    let mut stmt = conn.prepare("SELECT * FROM edges WHERE edge_type=?1 AND (?2 IS NULL OR id > ?2 COLLATE BINARY) ORDER BY id COLLATE BINARY LIMIT ?3")?;
    let rows = stmt.query_map(params![edge_type, after_id, page_size as i64], row_to_edge)?;
    let mut edges = Vec::new();
    let mut last = None;
    for row in rows {
        let edge = row?;
        last = Some(edge.id.clone());
        if is_active_at(&edge, now)? {
            edges.push(edge);
        }
    }
    let more = match &last {
        Some(last) => conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM edges WHERE edge_type=?1 AND id>?2 COLLATE BINARY)",
            params![edge_type, last],
            |r| r.get::<_, bool>(0),
        )?,
        None => false,
    };
    Ok(ActiveEdgePage {
        edges,
        next_after_id: last.filter(|_| more),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;
    use crate::thoughts::{ThoughtInput, insert_thought};

    fn chain(m: &Memory, n: usize) -> Vec<String> {
        let ids: Vec<String> = (0..n)
            .map(|i| {
                insert_thought(
                    &m.conn,
                    &ThoughtInput {
                        content: format!("n{i}"),
                        ..Default::default()
                    },
                )
                .unwrap()
            })
            .collect();
        for w in ids.windows(2) {
            link_thoughts(
                &m.conn,
                &EdgeInput {
                    source_id: w[0].clone(),
                    target_id: w[1].clone(),
                    edge_type: "follows".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        ids
    }

    #[test]
    fn link_validation_duplicates_and_cascade() {
        let m = Memory::open_in_memory().unwrap();
        let ids = chain(&m, 2);
        let dup = link_thoughts(
            &m.conn,
            &EdgeInput {
                source_id: ids[0].clone(),
                target_id: ids[1].clone(),
                edge_type: "follows".into(),
                ..Default::default()
            },
        );
        assert!(matches!(dup, Err(Error::Duplicate(_))));
        let bad = link_thoughts(
            &m.conn,
            &EdgeInput {
                source_id: ids[0].clone(),
                target_id: ids[1].clone(),
                edge_type: "loves".into(),
                ..Default::default()
            },
        );
        assert!(matches!(bad, Err(Error::InvalidInput(_))));
        let missing = link_thoughts(
            &m.conn,
            &EdgeInput {
                source_id: ids[0].clone(),
                target_id: "nope".into(),
                edge_type: "cites".into(),
                ..Default::default()
            },
        );
        assert!(matches!(missing, Err(Error::NotFound(_))));
        let meta: Map<String, serde_json::Value> =
            serde_json::from_str(r#"{"claim":"x"}"#).unwrap();
        let eid = link_thoughts(
            &m.conn,
            &EdgeInput {
                source_id: ids[1].clone(),
                target_id: ids[0].clone(),
                edge_type: "refuted_by".into(),
                metadata: Some(meta.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            get_edge(&m.conn, &eid).unwrap().unwrap().metadata,
            Some(meta)
        );
        assert_eq!(
            get_edges_between(&m.conn, &ids[1], &ids[0]).unwrap().len(),
            1
        );
        crate::thoughts::delete_thought(&m.conn, &ids[0]).unwrap();
        let n: i64 = m
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "FK cascade removed both edges");
    }

    #[test]
    fn traversal_depth_types_and_expiry() {
        let m = Memory::open_in_memory().unwrap();
        let ids = chain(&m, 4); // 0->1->2->3
        let nodes = traverse_graph(&m.conn, &ids[0], 2, None, false).unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].depth, 0);
        assert_eq!(nodes[2].depth, 2);
        assert_eq!(
            traverse_graph(&m.conn, &ids[0], 10, None, false)
                .unwrap()
                .len(),
            4,
            "clamped to 5, reaches all"
        );
        assert!(
            traverse_graph(&m.conn, "missing", 1, None, false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            traverse_graph(&m.conn, &ids[0], 3, Some(&["cites".to_string()]), false)
                .unwrap()
                .len(),
            1
        );
        // expire the 1->2 edge: traversal stops unless include_expired
        let e = get_edges_between(&m.conn, &ids[1], &ids[2])
            .unwrap()
            .remove(0);
        expire_edge(&m.conn, &e.id, Some("2000-01-01T00:00:00.000Z")).unwrap();
        assert_eq!(
            traverse_graph(&m.conn, &ids[0], 5, None, false)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            traverse_graph(&m.conn, &ids[0], 5, None, true)
                .unwrap()
                .len(),
            4
        );
        assert!(matches!(
            expire_edge(&m.conn, "nope", None),
            Err(Error::NotFound(_))
        ));
        // incoming direction from the tail
        let conns = get_connections(&m.conn, &ids[3], None).unwrap();
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].direction, Direction::Incoming);
        let related = fetch_graph_related(&m.conn, &[ids[3].clone()], 1).unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].id, ids[2]);
        assert!(
            fetch_graph_related(&m.conn, &[ids[3].clone()], 0)
                .unwrap()
                .is_empty()
        );
        assert!(unlink_thoughts(&m.conn, &ids[2], &ids[3], "follows").unwrap());
        assert!(!unlink_thoughts(&m.conn, &ids[2], &ids[3], "follows").unwrap());
    }

    const AT: &str = "2026-09-06T12:00:00.500Z";
    fn fixture() -> (Memory, Vec<String>) {
        let m = Memory::open_in_memory().unwrap();
        let ids = chain(&m, 5);
        m.conn.execute("DELETE FROM edges", []).unwrap();
        (m, ids)
    }
    fn refutation(
        m: &Memory,
        source: &str,
        target: &str,
        start: Option<&str>,
        end: Option<&str>,
        claim: Option<&str>,
    ) -> EdgeRecord {
        let id = link_thoughts(
            &m.conn,
            &EdgeInput {
                source_id: source.into(),
                target_id: target.into(),
                edge_type: "refuted_by".into(),
                valid_from: start.map(str::to_owned),
                valid_until: end.map(str::to_owned),
                metadata: claim.map(|claim| {
                    serde_json::from_value(serde_json::json!({"claim":claim})).unwrap()
                }),
            },
        )
        .unwrap();
        get_edge(&m.conn, &id).unwrap().unwrap()
    }
    fn assert_readers(m: &Memory, ids: &[String], at: &str, expected: usize) {
        assert_eq!(
            get_connections_at(&m.conn, &ids[0], None, at)
                .unwrap()
                .len(),
            expected
        );
        assert_eq!(
            fetch_graph_related_at(&m.conn, &ids[..1], 2, at)
                .unwrap()
                .len(),
            expected
        );
        assert_eq!(
            traverse_graph_at(&m.conn, &ids[0], 2, None, false, at)
                .unwrap()
                .len(),
            expected + 1
        );
        let candidates = crate::brief::load_brief_candidates(
            &m.conn,
            at,
            &crate::brief::BriefScopeInput {
                all_projects: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            candidates
                .iter()
                .find(|c| c.id == ids[0])
                .unwrap()
                .actively_refuted,
            expected > 0
        );
    }
    #[test]
    fn temporal_readers_share_boundaries_and_preserve_specific_refutation_history() {
        let (m, ids) = fixture();
        let whole = refutation(
            &m,
            &ids[0],
            &ids[1],
            Some("2026-09-06 12:00:00.500"),
            Some("2026-09-06T05:00:00.501-07:00"),
            None,
        );
        assert_readers(&m, &ids, "2026-09-06T12:00:00.499999999Z", 0);
        assert_readers(&m, &ids, AT, 1);
        assert_readers(&m, &ids, "2026-09-06T12:00:00.501Z", 0);
        assert!(is_active_at(&whole, AT).unwrap());
        let scoped = refutation(&m, &ids[0], &ids[2], None, None, Some("Only this claim"));
        expire_edge(&m.conn, &whole.id, Some(AT)).unwrap();
        let candidates = crate::brief::load_brief_candidates(
            &m.conn,
            AT,
            &crate::brief::BriefScopeInput {
                all_projects: true,
                ..Default::default()
            },
        )
        .unwrap();
        let source = candidates.iter().find(|c| c.id == ids[0]).unwrap();
        assert!(!source.actively_refuted);
        assert_eq!(source.refuted_claims, vec!["Only this claim"]);
        expire_edge(&m.conn, &scoped.id, Some(AT)).unwrap();
        let original = get_edge(&m.conn, &scoped.id).unwrap().unwrap();
        assert_eq!(original.metadata, scoped.metadata);
        assert_eq!(original.source_id, scoped.source_id);
        assert_eq!(
            get_edges_between(&m.conn, &ids[0], &ids[2]).unwrap().len(),
            1
        );
        assert_eq!(
            traverse_graph_at(&m.conn, &ids[0], 2, None, true, AT)
                .unwrap()
                .len(),
            3
        );
        assert_readers(&m, &ids, AT, 0);
        refutation(&m, &ids[0], &ids[3], None, None, None);
        refutation(&m, &ids[3], &ids[4], None, None, None);
        let remaining = crate::brief::load_brief_candidates(
            &m.conn,
            AT,
            &crate::brief::BriefScopeInput {
                all_projects: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            remaining
                .iter()
                .find(|c| c.id == ids[0])
                .unwrap()
                .actively_refuted
        );
        assert!(
            remaining
                .iter()
                .find(|c| c.id == ids[3])
                .unwrap()
                .actively_refuted
        );
        assert!(get_connections_at(&m.conn, "absent", None, "invalid").is_err());
        assert!(fetch_graph_related_at(&m.conn, &[], 0, "invalid").is_err());
        assert!(traverse_graph_at(&m.conn, "absent", 0, None, true, "invalid").is_err());
    }
    #[test]
    fn temporal_malformed_siblings_fail_exhaustively_in_both_orders_and_tool_path() {
        for future_start in [false, true] {
            for malformed_first in [false, true] {
                for claim in [None, Some("Partial claim")] {
                    for field in ["valid_from", "valid_until"] {
                        let (m, ids) = fixture();
                        let mut malformed_id = String::new();
                        for malformed in [malformed_first, !malformed_first] {
                            let edge = refutation(
                                &m,
                                &ids[0],
                                if malformed { &ids[1] } else { &ids[2] },
                                None,
                                None,
                                claim,
                            );
                            if malformed {
                                malformed_id = edge.id;
                            }
                        }
                        m.conn
                            .execute(
                                &format!(
                                    "UPDATE edges SET {field}='legacy secret malformed' WHERE id=?1"
                                ),
                                [&malformed_id],
                            )
                            .unwrap();
                        if future_start && field == "valid_until" {
                            m.conn.execute("UPDATE edges SET valid_from='2027-01-01T00:00:00Z' WHERE id=?1", [&malformed_id]).unwrap();
                        }
                        assert!(get_connections_at(&m.conn, &ids[0], None, AT).is_err());
                        assert!(fetch_graph_related_at(&m.conn, &ids[..1], 2, AT).is_err());
                        assert!(traverse_graph_at(&m.conn, &ids[0], 2, None, false, AT).is_err());
                        assert!(active_edges_page(&m.conn, "refuted_by", AT, None, 200).is_err());
                        assert!(
                            crate::brief::load_brief_candidates(
                                &m.conn,
                                AT,
                                &crate::brief::BriefScopeInput {
                                    all_projects: true,
                                    ..Default::default()
                                }
                            )
                            .is_err()
                        );
                        let tool = crate::tools::get_brief_tool(
                            &m,
                            &serde_json::json!({"all_projects":true,"now":AT}),
                        )
                        .json();
                        assert_eq!(tool["error"], "temporary_failure");
                        assert!(!tool.to_string().contains("legacy secret"));
                        assert!(get_edge(&m.conn, &malformed_id).unwrap().is_some());
                        assert_eq!(
                            traverse_graph_at(&m.conn, &ids[0], 2, None, true, AT)
                                .unwrap()
                                .len(),
                            3
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn temporal_write_validation_is_atomic_and_default_expiration_is_immediate() {
        let (m, ids) = fixture();
        for field in ["valid_from", "valid_until"] {
            let mut input = EdgeInput {
                source_id: ids[0].clone(),
                target_id: ids[1].clone(),
                edge_type: "refuted_by".into(),
                ..Default::default()
            };
            if field == "valid_from" {
                input.valid_from = Some("bad".into());
            } else {
                input.valid_until = Some("bad".into());
            }
            assert!(link_thoughts(&m.conn, &input).is_err());
            let result=crate::tools::manage_edges_tool(&m,&serde_json::json!({"action":"link","source_id":ids[0],"target_id":ids[1],"edge_type":"refuted_by",field:"bad"})).json();
            assert_eq!(result["error"], "invalid_input");
            assert!(!result.to_string().contains("Edge type must"));
        }
        assert!(
            get_edges_between(&m.conn, &ids[0], &ids[1])
                .unwrap()
                .is_empty()
        );
        let edge = refutation(&m, &ids[0], &ids[1], None, None, None);
        assert!(expire_edge(&m.conn, &edge.id, Some("bad")).is_err());
        assert_eq!(get_edge(&m.conn, &edge.id).unwrap().unwrap(), edge);
        assert_eq!(get_connections(&m.conn, &ids[0], None).unwrap().len(), 1);
        expire_edge(&m.conn, &edge.id, None).unwrap();
        assert!(get_connections(&m.conn, &ids[0], None).unwrap().is_empty());
        assert!(
            fetch_graph_related(&m.conn, &ids[..1], 1)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            traverse_graph(&m.conn, &ids[0], 1, None, false)
                .unwrap()
                .len(),
            1
        );
        let saved = get_edge(&m.conn, &edge.id).unwrap().unwrap();
        assert!(saved.valid_until.is_some());
        assert_eq!(saved.metadata, edge.metadata);
    }
    #[test]
    fn active_edges_page_scans_bounded_candidates_not_only_visible_rows() {
        let (m, ids) = fixture();
        for (i, end) in [(0, Some(AT)), (1, None), (2, None)] {
            let edge = refutation(&m, &ids[0], &ids[i + 1], None, end, Some("exact claim"));
            m.conn
                .execute(
                    "UPDATE edges SET id=?1 WHERE id=?2",
                    params![format!("edge-{i}"), edge.id],
                )
                .unwrap();
        }
        let first = active_edges_page(&m.conn, "refuted_by", AT, None, 1).unwrap();
        assert!(first.edges.is_empty());
        assert_eq!(first.next_after_id.as_deref(), Some("edge-0"));
        let second =
            active_edges_page(&m.conn, "refuted_by", AT, first.next_after_id.as_deref(), 1)
                .unwrap();
        assert_eq!(second.edges[0].id, "edge-1");
        assert_eq!(
            second.edges[0].metadata.as_ref().unwrap()["claim"],
            "exact claim"
        );
        let last = active_edges_page(
            &m.conn,
            "refuted_by",
            AT,
            second.next_after_id.as_deref(),
            200,
        )
        .unwrap();
        assert_eq!(last.edges.len(), 1);
        assert!(last.next_after_id.is_none());
        assert_eq!(
            active_edges_page(&m.conn, "refuted_by", AT, None, 200)
                .unwrap()
                .edges
                .len(),
            2
        );
        assert!(
            active_edges_page(&m.conn, "cites", AT, None, 1)
                .unwrap()
                .edges
                .is_empty()
        );
        for size in [0, 201, usize::MAX] {
            assert!(active_edges_page(&m.conn, "refuted_by", AT, None, size).is_err());
        }
        assert!(active_edges_page(&m.conn, "bad", AT, None, 1).is_err());
        assert!(active_edges_page(&m.conn, "refuted_by", "bad", None, 1).is_err());
        assert!(active_edges_page(&m.conn, "refuted_by", AT, Some(&"x".repeat(513)), 1).is_err());
        assert!(active_edges_page(&m.conn, "refuted_by", AT, Some("' OR 1=1 --"), 1).is_ok());
        m.conn
            .execute("UPDATE edges SET valid_until='bad' WHERE id='edge-2'", [])
            .unwrap();
        assert!(active_edges_page(&m.conn, "refuted_by", AT, None, 1).is_ok());
        assert!(active_edges_page(&m.conn, "refuted_by", AT, Some("edge-1"), 1).is_err());
    }
}

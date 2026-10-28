//! Local-only search telemetry (not part of any sync path).
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::Memory;
use crate::error::Result;
use crate::now_iso;
use crate::thoughts::parse_json_array;

pub fn is_rediscovery(current: &[String], previous: &[String]) -> bool {
    if previous.is_empty() {
        return false;
    }
    let prev: std::collections::HashSet<&String> = previous.iter().collect();
    let overlap: std::collections::HashSet<&String> =
        current.iter().filter(|id| prev.contains(id)).collect();
    overlap.len() as f64 / prev.len() as f64 > 0.5
}

pub fn record_search_telemetry(
    m: &Memory,
    query: &str,
    mode: &str,
    result_count: i64,
    top_ids: &[String],
    project_identifier: Option<&str>,
) -> Result<()> {
    let previous = {
        let mut cache = m.telemetry_prev.borrow_mut();
        if cache.is_none() {
            let raw: Option<Option<String>> = m
                .conn
                .query_row("SELECT top_ids FROM search_telemetry ORDER BY created_at DESC, rowid DESC LIMIT 1", [], |r| r.get(0))
                .optional()?;
            *cache = Some(parse_json_array(raw.flatten().as_deref()));
        }
        cache.clone().unwrap_or_default()
    };
    let rediscovery = is_rediscovery(top_ids, &previous);
    let hash = format!("{:x}", Sha256::digest(query.as_bytes()));
    m.conn.execute(
        "INSERT INTO search_telemetry (id, created_at, query_hash, mode, result_count, top_ids, rediscovery, project_identifier)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![uuid::Uuid::new_v4().to_string(), now_iso(), hash, mode, result_count, serde_json::to_string(top_ids)?, rediscovery as i64, project_identifier],
    )?;
    *m.telemetry_prev.borrow_mut() = Some(top_ids.to_vec());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rediscovery_needs_majority_overlap() {
        let a = vec!["1".to_string(), "2".into(), "3".into()];
        assert!(!is_rediscovery(&a, &[]));
        assert!(is_rediscovery(&a, &["1".into(), "2".into()]));
        assert!(!is_rediscovery(&a, &["1".into(), "9".into()]));
    }

    #[test]
    fn records_rows_and_tracks_previous() {
        let m = Memory::open_in_memory().unwrap();
        record_search_telemetry(&m, "q", "fts", 2, &["a".into(), "b".into()], Some("shelby"))
            .unwrap();
        record_search_telemetry(&m, "q", "fts", 2, &["a".into(), "b".into()], None).unwrap();
        let rows: Vec<i64> = m
            .conn
            .prepare("SELECT rediscovery FROM search_telemetry ORDER BY created_at, rowid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![0, 1]);
    }
}

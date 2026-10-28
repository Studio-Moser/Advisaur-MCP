//! Embeddings (ADR 0001 §3): float32 LE blobs, cosine similarity, default threshold 0.3.
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::HashSet;

use crate::error::Result;
use crate::thoughts::parse_json_array;

pub const DEFAULT_THRESHOLD: f32 = 0.3;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VectorSearchResult {
    pub id: String,
    pub summary: Option<String>,
    pub r#type: String,
    pub topics: Vec<String>,
    pub created_at: String,
    pub similarity: f32,
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (*x as f64, *y as f64);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        (dot / denom) as f32
    }
}

pub fn embedding_to_bytes(e: &[f32]) -> Vec<u8> {
    e.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn bytes_to_embedding(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

pub fn store_embedding(conn: &Connection, thought_id: &str, embedding: &[f32]) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE thoughts SET embedding = ?1 WHERE id = ?2",
        params![embedding_to_bytes(embedding), thought_id],
    )? > 0)
}

pub fn get_embedding(conn: &Connection, thought_id: &str) -> Result<Option<Vec<f32>>> {
    let blob: Option<Option<Vec<u8>>> = conn
        .query_row(
            "SELECT embedding FROM thoughts WHERE id = ?1",
            params![thought_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(blob.flatten().map(|b| bytes_to_embedding(&b)))
}

/// In-memory cosine scan over every embedded thought (optionally restricted to `eligible`).
pub fn search_by_embedding(
    conn: &Connection,
    query: &[f32],
    limit: Option<i64>,
    threshold: Option<f32>,
    eligible: Option<&HashSet<String>>,
) -> Result<Vec<VectorSearchResult>> {
    let limit = limit.unwrap_or(20).clamp(1, 100) as usize;
    let threshold = threshold.unwrap_or(DEFAULT_THRESHOLD);
    let mut stmt = conn.prepare("SELECT id, summary, type, topics, created_at, embedding FROM thoughts WHERE embedding IS NOT NULL")?;
    let rows = stmt.query_map([], |r| {
        let topics: Option<String> = r.get(3)?;
        let blob: Vec<u8> = r.get(5)?;
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            topics,
            r.get::<_, String>(4)?,
            blob,
        ))
    })?;
    let mut scored = Vec::new();
    for row in rows {
        let (id, summary, ty, topics, created_at, blob) = row?;
        if let Some(e) = eligible
            && !e.contains(&id)
        {
            continue;
        }
        let sim = cosine_similarity(query, &bytes_to_embedding(&blob));
        if sim >= threshold {
            scored.push(VectorSearchResult {
                id,
                summary,
                r#type: ty,
                topics: parse_json_array(topics.as_deref()),
                created_at,
                similarity: sim,
            });
        }
    }
    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);
    Ok(scored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;
    use crate::thoughts::{ThoughtInput, insert_thought};

    #[test]
    fn cosine_and_bytes() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 2.0]), 0.0);
        let v = vec![0.5f32, -1.25, 3.0];
        assert_eq!(bytes_to_embedding(&embedding_to_bytes(&v)), v);
    }

    #[test]
    fn vector_search_thresholds_sorts_and_filters() {
        let m = Memory::open_in_memory().unwrap();
        let ids: Vec<String> = (0..3)
            .map(|i| {
                insert_thought(
                    &m.conn,
                    &ThoughtInput {
                        content: format!("t{i}"),
                        ..Default::default()
                    },
                )
                .unwrap()
            })
            .collect();
        store_embedding(&m.conn, &ids[0], &[1.0, 0.0]).unwrap();
        store_embedding(&m.conn, &ids[1], &[0.8, 0.6]).unwrap();
        store_embedding(&m.conn, &ids[2], &[0.0, 1.0]).unwrap();
        assert!(!store_embedding(&m.conn, "missing", &[1.0]).unwrap());
        assert_eq!(
            get_embedding(&m.conn, &ids[1]).unwrap(),
            Some(vec![0.8, 0.6])
        );
        let r = search_by_embedding(&m.conn, &[1.0, 0.0], None, None, None).unwrap();
        assert_eq!(
            r.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            vec![ids[0].as_str(), ids[1].as_str()],
            "0.0 similarity is below 0.3"
        );
        let eligible: HashSet<String> = [ids[1].clone()].into();
        let r =
            search_by_embedding(&m.conn, &[1.0, 0.0], None, Some(0.0), Some(&eligible)).unwrap();
        assert_eq!(r.len(), 1);
    }
}

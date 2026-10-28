//! Write-path reconciliation (ADR 0001 §§6b–6d): token similarity decides
//! no-op / merge / supersede / add. Thresholds are shared contract constants.
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::HashSet;

use crate::error::Result;
use crate::fts::sanitize_fts_query;
use crate::thoughts::TrustLevel;

pub const MIN_TOKENS: usize = 4;
pub const NOOP_THRESHOLD: f64 = 0.9;
pub const ORDERED_NOOP_THRESHOLD: f64 = 0.8;
pub const AUTO_EDGE_THRESHOLD: f64 = 0.8;
pub const SUGGEST_THRESHOLD: f64 = 0.3;
pub const CANDIDATE_LIMIT: i64 = 20;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub content: String,
    pub r#type: String,
    pub summary: Option<String>,
    pub trust_level: Option<TrustLevel>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SuggestedEdge {
    pub id: String,
    pub similarity: f64,
    pub auto_apply: bool,
    /// `Some("refuted_by")` when this candidate is a reversal (§6b).
    pub edge_type: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Noop { existing_id: String },
    Add { suggested_edges: Vec<SuggestedEdge> },
}

/// Lowercased runs of letters/digits, in order.
pub fn token_sequence(content: &str) -> Vec<String> {
    // ponytail: `is_alphanumeric` ≈ \p{L}\p{N}; differs only on rare Other_Alphabetic marks.
    content
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn tokenize(content: &str) -> HashSet<String> {
    token_sequence(content).into_iter().collect()
}

pub fn jaccard(left: &HashSet<String>, right: &HashSet<String>) -> f64 {
    let intersection = left.iter().filter(|t| right.contains(*t)).count();
    let union = left.len() + right.len() - intersection;
    if union == 0 {
        f64::NAN
    } else {
        intersection as f64 / union as f64
    }
}

fn levenshtein(a: &[String], b: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let cost = usize::from(x != y);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// `1 − levenshtein(tokensA, tokensB) / max(lenA, lenB)`; 0 when either side is empty.
pub fn ordered_similarity(left: &str, right: &str) -> f64 {
    let l = token_sequence(left);
    let r = token_sequence(right);
    if l.is_empty() || r.is_empty() {
        return 0.0;
    }
    1.0 - levenshtein(&l, &r) as f64 / l.len().max(r.len()) as f64
}

pub fn reconcile(
    content: &str,
    r#type: &str,
    candidates: &[Candidate],
    input_tokens: &HashSet<String>,
) -> Decision {
    if input_tokens.len() < MIN_TOKENS {
        return Decision::Add {
            suggested_edges: vec![],
        };
    }
    struct Scored<'a> {
        id: &'a str,
        similarity: f64,
        ordered: Option<f64>,
        same_type: bool,
    }
    let mut scored: Vec<Scored> = candidates
        .iter()
        .map(|c| {
            let similarity = jaccard(input_tokens, &tokenize(&c.content));
            let same_type = c.r#type == r#type;
            let ordered = (same_type && similarity >= NOOP_THRESHOLD)
                .then(|| ordered_similarity(content, &c.content));
            Scored {
                id: &c.id,
                similarity,
                ordered,
                same_type,
            }
        })
        .collect();
    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(b.id))
    });

    let is_reversal = |s: &Scored| {
        s.same_type
            && s.similarity >= NOOP_THRESHOLD
            && s.ordered.is_some_and(|o| o < ORDERED_NOOP_THRESHOLD)
    };
    if let Some(dup) = scored.iter().find(|s| {
        s.same_type
            && s.similarity >= NOOP_THRESHOLD
            && s.ordered.is_some_and(|o| o >= ORDERED_NOOP_THRESHOLD)
    }) {
        return Decision::Noop {
            existing_id: dup.id.to_string(),
        };
    }
    Decision::Add {
        suggested_edges: scored
            .iter()
            .filter(|s| s.similarity >= SUGGEST_THRESHOLD)
            .map(|s| SuggestedEdge {
                id: s.id.to_string(),
                similarity: s.similarity,
                auto_apply: s.similarity >= AUTO_EDGE_THRESHOLD && !is_reversal(s),
                edge_type: is_reversal(s).then_some("refuted_by"),
            })
            .collect(),
    }
}

/// FTS-prefiltered candidates in the same scope (null project scope = unscoped personal rows).
pub fn find_candidates(
    conn: &Connection,
    content: &str,
    project_id: Option<&str>,
    project: Option<&str>,
    input_tokens: &HashSet<String>,
) -> Result<Vec<Candidate>> {
    if input_tokens.len() < MIN_TOKENS {
        return Ok(vec![]);
    }
    let head: String = content.chars().take(200).collect();
    let fts_query = sanitize_fts_query(&head);
    if fts_query.is_empty() || (project_id.is_none() && project.is_some()) {
        return Ok(vec![]);
    }
    let scope = if project_id.is_none() {
        "t.project_id IS NULL AND t.project_identifier IS NULL AND t.project IS NULL AND t.visibility = 'personal'"
    } else {
        "t.project_id = ?2 AND (t.project_identifier IS NULL OR EXISTS (
           SELECT 1 FROM project_slug_aliases alias WHERE alias.slug = t.project_identifier AND alias.project_id = t.project_id))"
    };
    let sql = format!(
        "SELECT t.id, t.content, t.type, t.summary, t.trust_level FROM thoughts_fts
         JOIN thoughts t ON thoughts_fts.rowid = t.rowid
         WHERE thoughts_fts MATCH ?1 AND {scope} ORDER BY rank LIMIT ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![fts_query, project_id, CANDIDATE_LIMIT], |r| {
        let trust: Option<String> = r.get(4)?;
        Ok(Candidate {
            id: r.get(0)?,
            content: r.get(1)?,
            r#type: r.get(2)?,
            summary: r.get(3)?,
            trust_level: trust.as_deref().and_then(TrustLevel::parse),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// §6d: lower-trust captures never touch trusted thoughts.
pub fn can_reconcile(capture: TrustLevel, existing: Option<TrustLevel>) -> bool {
    capture == TrustLevel::Trusted || existing != Some(TrustLevel::Trusted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, content: &str, ty: &str) -> Candidate {
        Candidate {
            id: id.into(),
            content: content.into(),
            r#type: ty.into(),
            summary: None,
            trust_level: Some(TrustLevel::Trusted),
        }
    }

    #[test]
    fn tokens_and_similarities() {
        assert_eq!(
            token_sequence("Tabs, over spaces! Ünïcode 42"),
            vec!["tabs", "over", "spaces", "ünïcode", "42"]
        );
        let a = tokenize("tabs over spaces always");
        let b = tokenize("spaces over tabs always");
        assert_eq!(jaccard(&a, &b), 1.0);
        assert!(
            (ordered_similarity("tabs over spaces always", "spaces over tabs always") - 0.5).abs()
                < 1e-9
        );
        assert_eq!(ordered_similarity("", "x"), 0.0);
        assert_eq!(ordered_similarity("a b c d", "a b c d"), 1.0);
    }

    #[test]
    fn duplicate_is_noop_reversal_supersedes() {
        let content = "we always prefer tabs over spaces here";
        let dup = cand("dup", "we always prefer tabs over spaces here", "note");
        let d = reconcile(content, "note", &[dup], &tokenize(content));
        assert_eq!(
            d,
            Decision::Noop {
                existing_id: "dup".into()
            }
        );

        let rev = cand("rev", "here we always prefer spaces over tabs", "note");
        let d = reconcile(content, "note", &[rev], &tokenize(content));
        let Decision::Add { suggested_edges } = d else {
            panic!("reversal must not be a noop")
        };
        assert_eq!(suggested_edges.len(), 1);
        assert_eq!(suggested_edges[0].edge_type, Some("refuted_by"));
        assert!(!suggested_edges[0].auto_apply);
    }

    #[test]
    fn type_mismatch_never_noops_and_short_inputs_skip() {
        let content = "we always prefer tabs over spaces here";
        let other = cand("o", content, "decision");
        let Decision::Add { suggested_edges } =
            reconcile(content, "note", &[other], &tokenize(content))
        else {
            panic!()
        };
        assert!(suggested_edges[0].auto_apply && suggested_edges[0].edge_type.is_none());
        assert_eq!(
            reconcile(
                "too short",
                "note",
                &[cand("x", "too short", "note")],
                &tokenize("too short")
            ),
            Decision::Add {
                suggested_edges: vec![]
            }
        );
    }

    #[test]
    fn trust_gate() {
        assert!(can_reconcile(
            TrustLevel::Trusted,
            Some(TrustLevel::External)
        ));
        assert!(!can_reconcile(
            TrustLevel::External,
            Some(TrustLevel::Trusted)
        ));
        assert!(can_reconcile(
            TrustLevel::Unverified,
            Some(TrustLevel::Unverified)
        ));
        assert!(can_reconcile(TrustLevel::External, None));
    }

    #[test]
    fn candidates_are_scoped() {
        use crate::thoughts::{ThoughtInput, insert_thought};
        let m = crate::Memory::open_in_memory().unwrap();
        let content = "reconciliation candidates must respect the project scope";
        insert_thought(
            &m.conn,
            &ThoughtInput {
                content: content.into(),
                ..Default::default()
            },
        )
        .unwrap();
        insert_thought(
            &m.conn,
            &ThoughtInput {
                content: content.into(),
                project_id: Some("p1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let toks = tokenize(content);
        assert_eq!(
            find_candidates(&m.conn, content, None, None, &toks)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            find_candidates(&m.conn, content, Some("p1"), None, &toks)
                .unwrap()
                .len(),
            1
        );
        assert!(
            find_candidates(&m.conn, content, None, Some("/path"), &toks)
                .unwrap()
                .is_empty()
        );
    }
}

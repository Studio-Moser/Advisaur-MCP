//! Topic canonicalization (ADR 0001 §6e): NFC → lowercase → runs of
//! whitespace/underscore/hyphen → "-" → trim hyphens.
use unicode_normalization::UnicodeNormalization;

pub fn canonicalize_topic(topic: &str) -> String {
    let lowered: String = topic.nfc().collect::<String>().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut pending_sep = false;
    for ch in lowered.chars() {
        if ch.is_whitespace() || ch == '_' || ch == '-' {
            pending_sep = true;
        } else {
            if pending_sep && !out.is_empty() {
                out.push('-');
            }
            pending_sep = false;
            out.push(ch);
        }
    }
    out
}

pub fn canonicalize_topics(topics: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for topic in topics {
        let value = canonicalize_topic(topic);
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        out.push(value);
    }
    out
}

/// Canonicalize a stored JSON array of topics; `None` when the JSON isn't a string array.
pub fn canonicalize_stored_topics(raw: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    let items = parsed.as_array()?;
    let strings: Option<Vec<String>> = items
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect();
    serde_json::to_string(&canonicalize_topics(&strings?)).ok()
}

/// SQL LIKE pattern matching a canonical topic inside the stored JSON array.
pub fn topic_like_pattern(topic: &str) -> String {
    format!("%\"{}\"%", canonicalize_topic(topic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adr_examples() {
        assert_eq!(canonicalize_topic("Knowledge Graph"), "knowledge-graph");
        assert_eq!(canonicalize_topic("_Knowledge_Graph_"), "knowledge-graph");
        assert_eq!(canonicalize_topic("  --a--  b  "), "a-b");
        assert_eq!(canonicalize_topic("---"), "");
    }

    #[test]
    fn nfc_makes_decomposed_and_precomposed_match() {
        assert_eq!(
            canonicalize_topic("Cafe\u{301}"),
            canonicalize_topic("Café")
        );
    }

    #[test]
    fn dedupes_and_drops_empty() {
        let out = canonicalize_topics(&["A".into(), "a".into(), "".into(), "B_c".into()]);
        assert_eq!(out, vec!["a", "b-c"]);
    }

    #[test]
    fn stored_topics() {
        assert_eq!(
            canonicalize_stored_topics(r#"["Foo Bar","foo-bar"]"#).as_deref(),
            Some(r#"["foo-bar"]"#)
        );
        assert_eq!(canonicalize_stored_topics(r#"[1]"#), None);
        assert_eq!(canonicalize_stored_topics("nope"), None);
    }
}

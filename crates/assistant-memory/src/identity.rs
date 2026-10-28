//! Project Identity v2 primitives (ADR 0001 §"Project Identity v2").
use uuid::Uuid;

pub const PROJECT_NAMESPACE: &str = "cbe30437-5f4b-55bd-af13-8a1029beeffe";
pub const PROJECT_NAME_PREFIX: &str = "shelby-project-v1:";

/// UUIDv5 for a legacy slug; identical to the TS/Swift derivation.
pub fn derive_existing_project_id(legacy_slug: &str) -> Option<String> {
    if !is_valid_project_slug(legacy_slug) {
        return None;
    }
    let ns = Uuid::parse_str(PROJECT_NAMESPACE).expect("constant namespace");
    Some(
        Uuid::new_v5(
            &ns,
            format!("{PROJECT_NAME_PREFIX}{legacy_slug}").as_bytes(),
        )
        .to_string(),
    )
}

/// Lowercase, hyphenated, RFC-4122 UUID string. Uppercase/compact/padded forms are rejected.
pub fn is_canonical_project_id(value: &str) -> bool {
    value == value.to_lowercase() && is_rfc4122_string(value)
}

fn is_rfc4122_string(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    // Version nibble 1-8 and RFC variant, mirroring the `uuid` npm validator.
    let version = b[14];
    let variant = b[19].to_ascii_lowercase();
    matches!(version, b'1'..=b'8') && matches!(variant, b'8' | b'9' | b'a' | b'b')
}

/// `^[a-z0-9]+(?:-[a-z0-9]+)*$`, 1..=128 UTF-8 bytes.
pub fn is_valid_project_slug(value: &str) -> bool {
    let len = value.len();
    if len == 0 || len > 128 {
        return false;
    }
    let mut prev_dash = true; // disallow leading dash
    for c in value.bytes() {
        match c {
            b'a'..=b'z' | b'0'..=b'9' => prev_dash = false,
            b'-' if !prev_dash => prev_dash = true,
            _ => return false,
        }
    }
    !prev_dash
}

/// Shared `slugify` (ADR 0001 §1b.3): trim → lowercase → [\s_]+→"-" → strip
/// [^a-z0-9-] → collapse "-" → trim "-" → fallback "project".
pub fn slugify(input: &str) -> String {
    let lowered = input.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut pending = false;
    for ch in lowered.chars() {
        if ch.is_whitespace() || ch == '_' || ch == '-' {
            pending = true;
        } else if ch.is_ascii_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('-');
            }
            pending = false;
            out.push(ch);
        }
        // any other char is stripped (does not act as a separator)
    }
    if out.is_empty() {
        "project".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/project-identity-v2.json");

    #[test]
    fn uuid_v5_vectors_from_shared_fixture() {
        let fx: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(fx["uuid_v5"]["namespace"], PROJECT_NAMESPACE);
        assert_eq!(fx["uuid_v5"]["name_prefix"], PROJECT_NAME_PREFIX);
        let vectors = fx["uuid_v5"]["vectors"].as_array().unwrap();
        assert!(!vectors.is_empty());
        for v in vectors {
            let slug = v["legacy_slug"].as_str().unwrap();
            assert_eq!(
                derive_existing_project_id(slug).as_deref(),
                v["project_id"].as_str(),
                "{slug}"
            );
        }
    }

    #[test]
    fn uuid_validation_cases_from_shared_fixture() {
        let fx: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        for case in fx["uuid_validation"]["cases"].as_array().unwrap() {
            let value = case["value"].as_str().unwrap();
            assert_eq!(
                is_canonical_project_id(value),
                case["expected"].as_bool().unwrap(),
                "{value:?}"
            );
        }
    }

    #[test]
    fn slug_rules_from_shared_fixture() {
        let fx: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let rules = &fx["slug_rules"];
        assert_eq!(rules["max_utf8_bytes"], 128);
        if let Some(cases) = rules["cases"].as_array() {
            for case in cases {
                let value = case["value"].as_str().unwrap();
                assert_eq!(
                    is_valid_project_slug(value),
                    case["expected"].as_bool().unwrap(),
                    "{value:?}"
                );
            }
        }
        assert!(is_valid_project_slug("advisaur-mcp"));
        assert!(!is_valid_project_slug("-shelby"));
        assert!(!is_valid_project_slug("shelby--mcp"));
        assert!(!is_valid_project_slug("Shelby"));
        assert!(!is_valid_project_slug(&"a".repeat(129)));
    }

    #[test]
    fn slugify_contract() {
        assert_eq!(slugify("  The Crooked Line "), "the-crooked-line");
        assert_eq!(slugify("KUOW_Games"), "kuow-games");
        assert_eq!(slugify("Ausra Photos!"), "ausra-photos");
        assert_eq!(slugify("--a---b--"), "a-b");
        assert_eq!(slugify("!!!"), "project");
        assert_eq!(slugify("é"), "project");
    }
}

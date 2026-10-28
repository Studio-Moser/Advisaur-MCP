//! Input caps (OWASP ASI06 — memory poisoning mitigation). Shared contract values.
pub const MAX_CONTENT_LENGTH: usize = 50_000;
pub const MAX_SUMMARY_LENGTH: usize = 200;
pub const MAX_TOPIC_LENGTH: usize = 100;
pub const MAX_TOPICS_COUNT: usize = 20;
pub const MAX_PEOPLE_COUNT: usize = 20;
pub const MAX_PERSON_LENGTH: usize = 100;
pub const MAX_BULK_THOUGHTS: usize = 50;

/// Clamp a caller-supplied limit into `[1, max]`, defaulting when absent.
pub fn clamp_limit(limit: Option<i64>, default: i64, max: i64) -> i64 {
    limit.unwrap_or(default).clamp(1, max)
}

/// Lengths are counted in UTF-16 code units to preserve the pre-0.4 contract's
/// `String#length`, so both engines accept and reject identical inputs.
pub fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clamp() {
        assert_eq!(clamp_limit(None, 20, 100), 20);
        assert_eq!(clamp_limit(Some(0), 20, 100), 1);
        assert_eq!(clamp_limit(Some(500), 20, 100), 100);
    }
    #[test]
    fn js_length_counts_utf16_units() {
        assert_eq!(js_len("a😀"), 3);
    }
}

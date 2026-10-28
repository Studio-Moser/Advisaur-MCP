//! Shelby memory engine: the shared contract from ADR 0001 implemented once in Rust.
//!
//! Layering: `db` opens SQLite and runs `migrations`; `thoughts`, `fts`, `vectors`,
//! `edges` are the storage operations; `reconcile`, `topics`, `trust`, `identity`
//! are pure contract functions. Tool-level behavior (scope resolution, capture
//! actions, briefs) sits above this crate's storage layer.

pub mod brief;
pub mod db;
pub mod detect;
pub mod edges;
pub mod error;
pub mod fts;
pub mod identity;
pub mod limits;
pub mod migrations;
pub mod projects;
pub mod reconcile;
pub mod repair;
pub mod resolve;
pub mod seed;
pub mod swift_import;
pub mod telemetry;
mod temporal;
pub mod thoughts;
pub mod tools;
pub mod topics;
pub mod trust;
pub mod vectors;

pub use db::Memory;
pub use error::{Error, Result};
pub use rusqlite;

/// ISO 8601 UTC with millisecond precision, byte-identical to JS `Date#toISOString()`.
pub fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn now_iso_matches_js_shape() {
        let s = super::now_iso();
        assert_eq!(s.len(), 24, "{s}");
        assert!(s.ends_with('Z') && s.as_bytes()[10] == b'T' && s.as_bytes()[19] == b'.');
    }
}

//! Connection lifecycle. WAL for file databases, foreign keys on, migrations at open.
use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;
use crate::migrations::{run_migrations, schema_version};
use crate::topics::canonicalize_topic;

pub struct Memory {
    pub conn: Connection,
    /// Top ids of the previous search on this connection (rediscovery telemetry).
    pub(crate) telemetry_prev: std::cell::RefCell<Option<Vec<String>>>,
}

impl Memory {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        run_migrations(&conn)?;
        Ok(Self {
            conn,
            telemetry_prev: std::cell::RefCell::new(None),
        })
    }

    pub fn schema_version(&self) -> Result<i64> {
        schema_version(&self.conn)
    }

    /// Distinct values inside a JSON-array column (`topics` | `people`) starting with `prefix`.
    pub fn distinct_array_values(
        &self,
        column: &str,
        prefix: &str,
        limit: i64,
    ) -> Result<Vec<String>> {
        let column = match column {
            "topics" | "people" => column,
            other => {
                return Err(crate::Error::InvalidInput(format!(
                    "not an array column: {other}"
                )));
            }
        };
        let prefix = if column == "topics" {
            canonicalize_topic(prefix)
        } else {
            prefix.to_string()
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT DISTINCT je.value AS val FROM thoughts, json_each(thoughts.{column}) AS je
             WHERE je.value LIKE ?1 ORDER BY je.value LIMIT ?2"
        ))?;
        let rows = stmt.query_map(rusqlite::params![format!("{prefix}%"), limit], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Distinct scalar values (`project` | `source` | `type`) starting with `prefix`.
    pub fn distinct_values(&self, column: &str, prefix: &str, limit: i64) -> Result<Vec<String>> {
        let column = match column {
            "project" | "source" | "type" => column,
            other => {
                return Err(crate::Error::InvalidInput(format!(
                    "not a completable column: {other}"
                )));
            }
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT DISTINCT {column} AS val FROM thoughts WHERE {column} IS NOT NULL AND {column} LIKE ?1 ORDER BY {column} LIMIT ?2"
        ))?;
        let rows = stmt.query_map(rusqlite::params![format!("{prefix}%"), limit], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_file_db_in_wal_and_migrates() {
        let dir = std::env::temp_dir().join(format!("shelby-memory-{}", uuid::Uuid::new_v4()));
        let m = Memory::open(dir.join("nested").join("memory.db")).unwrap();
        let mode: String = m
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(m.schema_version().unwrap(), 18);
        drop(m);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn completion_helpers_reject_unknown_columns() {
        let m = Memory::open_in_memory().unwrap();
        assert!(m.distinct_values("id", "", 5).is_err());
        assert!(m.distinct_array_values("content", "", 5).is_err());
    }
}

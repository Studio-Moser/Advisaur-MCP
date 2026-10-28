//! Parsed edge validity shared by graph, brief, and paginated reads.
use crate::{Error, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use regex::Regex;
use rusqlite::{Connection, functions::FunctionFlags};
use std::sync::OnceLock;

pub(crate) const ACTIVE_SQL: &str = "shelby_edge_active_v1(e.valid_from, e.valid_until, @edge_now)";
fn invalid(field: &str) -> Error {
    Error::InvalidInput(format!(
        "Invalid {field}; use a calendar-valid RFC3339 instant (legacy UTC bounds may use YYYY-MM-DD HH:MM:SS), with up to nine fractional digits"
    ))
}
fn parse(value: &str, legacy: bool, field: &str) -> Result<DateTime<Utc>> {
    static SHAPE: OnceLock<Regex> = OnceLock::new();
    if value.len() > 64 {
        return Err(invalid(field));
    }
    let shape = SHAPE.get_or_init(|| Regex::new(r"\A[0-9]{4}-[0-9]{2}-[0-9]{2}(?P<separator>T| )[0-9]{2}:[0-9]{2}:[0-5][0-9](?:\.[0-9]{1,9})?(?P<zone>Z|[+-][0-9]{2}:[0-9]{2})?\z").expect("constant timestamp grammar"));
    let captures = shape.captures(value).ok_or_else(|| invalid(field))?;
    if &captures["separator"] == " " {
        if !legacy || captures.name("zone").is_some() {
            return Err(invalid(field));
        }
        NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
            .map(|date| date.and_utc())
            .map_err(|_| invalid(field))
    } else {
        if captures.name("zone").is_none() {
            return Err(invalid(field));
        }
        DateTime::parse_from_rfc3339(value)
            .map(|date| date.with_timezone(&Utc))
            .map_err(|_| invalid(field))
    }
}
pub(crate) fn parse_bound(value: &str) -> Result<DateTime<Utc>> {
    parse(value, true, "validity bound")
}
pub(crate) fn parse_now(value: &str) -> Result<DateTime<Utc>> {
    parse(value, false, "current instant")
}
pub(crate) fn active_at(start: Option<&str>, end: Option<&str>, now: &str) -> Result<bool> {
    let now = parse_now(now)?;
    let start = start
        .map(|value| parse_bound(value).map_err(|_| invalid("valid_from")))
        .transpose()?;
    let end = end
        .map(|value| parse_bound(value).map_err(|_| invalid("valid_until")))
        .transpose()?;
    Ok(start.is_none_or(|start| start <= now) && end.is_none_or(|end| now < end))
}
// The name is reserved. SQLite metadata identifies compatible registration, not
// callback identity; a caller-owned connection is not authenticated by this check.
pub(crate) fn ensure_sql_function(conn: &Connection) -> Result<()> {
    let required = FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS;
    let existing = {
        let mut stmt = conn.prepare(
            "SELECT narg, flags FROM pragma_function_list WHERE name = 'shelby_edge_active_v1'",
        )?;
        stmt.query_map([], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    if !existing.is_empty() {
        if existing.len() == 1
            && existing[0].0 == 3
            && existing[0].1 & required.bits() == required.bits()
        {
            return Ok(());
        }
        return Err(Error::InvalidInput(
            "Reserved temporal SQL function conflicts with this connection".into(),
        ));
    }
    conn.create_scalar_function(
        "shelby_edge_active_v1",
        3,
        required | FunctionFlags::SQLITE_UTF8,
        |ctx| {
            // Borrow SQLite text and reject oversized values before copying or parsing them.
            let text = |index, field| -> rusqlite::Result<Option<&str>> {
                match ctx.get_raw(index) {
                    rusqlite::types::ValueRef::Null => Ok(None),
                    rusqlite::types::ValueRef::Text(bytes) if bytes.len() <= 64 => {
                        std::str::from_utf8(bytes).map(Some).map_err(|_| {
                            rusqlite::Error::UserFunctionError(Box::new(invalid(field)))
                        })
                    }
                    _ => Err(rusqlite::Error::UserFunctionError(Box::new(invalid(field)))),
                }
            };
            let start = text(0, "valid_from")?;
            let end = text(1, "valid_until")?;
            let now = text(2, "current instant")?.ok_or_else(|| {
                rusqlite::Error::UserFunctionError(Box::new(invalid("current instant")))
            })?;
            active_at(start, end, now)
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    const NOW: &str = "2026-09-06T12:00:00.500Z";

    #[test]
    fn intervals_preserve_offsets_and_fractional_boundaries() {
        for (start, end, expected) in [
            (None, None, true),
            (Some(NOW), None, true),
            (None, Some(NOW), false),
            (Some("2026-09-06T12:00:00.501Z"), None, false),
            (None, Some("2026-09-06T12:00:00.501Z"), true),
            (Some("2026-09-06T12:00:00.500000001Z"), None, false),
            (None, Some("2026-09-06T12:00:00.500000001Z"), true),
            (Some("2026-09-06T12:00:00.499999999Z"), None, true),
            (Some(NOW), Some(NOW), false),
            (Some("2027-01-01T00:00:00Z"), Some(NOW), false),
            (Some("2026-09-06 12:00:00.500"), None, true),
            (Some("2026-09-06 12:00:00"), None, true),
            (Some("2026-09-06T05:00:00.500-07:00"), None, true),
            (Some("2026-09-07T02:00:00.500+14:00"), None, true),
        ] {
            assert_eq!(
                active_at(start, end, NOW).unwrap(),
                expected,
                "{start:?} {end:?}"
            );
        }
    }

    #[test]
    fn invalid_dates_are_bounded_and_both_bounds_are_validated() {
        for bad in [
            "",
            "now",
            "2026-02-30T00:00:00Z",
            "2026-09-06",
            "2026-09-06T12:00:00",
            "2026-09-06T24:00:00Z",
            "2026-09-06T12:00:60Z",
            "2026-09-06T12:00:00.1234567890Z",
            " 2026-09-06T12:00:00Z",
            "2026-09-06 12:00:00Z",
            "2026-09-06T12:00:00Z ",
            &"x".repeat(65),
        ] {
            assert!(parse_bound(bad).is_err(), "{bad:?}");
            let error = active_at(Some("2027-01-01T00:00:00Z"), Some(bad), NOW)
                .unwrap_err()
                .to_string();
            assert!(error.contains("valid_until"));
            assert!(!error.contains("2027"));
            assert!(active_at(Some(bad), None, NOW).is_err());
        }
        assert!(active_at(None, None, "invalid").is_err());
        assert!(parse_now("2026-09-06 12:00:00").is_err());
    }

    #[test]
    fn sql_registration_is_connection_local_and_safe_with_live_cursors_and_transactions() {
        for _ in 0..2 {
            let mut conn = Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE sample(id); INSERT INTO sample VALUES(1),(2)")
                .unwrap();
            let mut stmt = conn.prepare("SELECT id FROM sample").unwrap();
            let mut rows = stmt.query([]).unwrap();
            assert!(rows.next().unwrap().is_some());
            ensure_sql_function(&conn).unwrap();
            ensure_sql_function(&conn).unwrap();
            let sql = "SELECT shelby_edge_active_v1(?1,?2,?3)";
            for (start, end) in [
                (None, None),
                (Some(NOW), Some(NOW)),
                (Some("2026-09-06 12:00:00.500"), None),
                (None, Some("2026-09-06T05:00:00.500000001-07:00")),
            ] {
                let value: bool = conn
                    .query_row(sql, rusqlite::params![start, end, NOW], |r| r.get(0))
                    .unwrap();
                assert_eq!(value, active_at(start, end, NOW).unwrap());
            }
            assert!(
                conn.query_row(
                    sql,
                    rusqlite::params!["2027-01-01T00:00:00Z", "bad", NOW],
                    |r| r.get::<_, bool>(0)
                )
                .is_err()
            );
            assert!(rows.next().unwrap().is_some());
            drop(rows);
            drop(stmt);
            let tx = conn.transaction().unwrap();
            ensure_sql_function(&tx).unwrap();
            tx.execute_batch("SAVEPOINT temporal_test").unwrap();
            ensure_sql_function(&tx).unwrap();
            tx.execute_batch("RELEASE temporal_test").unwrap();
            tx.rollback().unwrap();
        }
        let path = std::env::temp_dir().join(format!("temporal-{}.sqlite", uuid::Uuid::new_v4()));
        for _ in 0..2 {
            let conn = Connection::open(&path).unwrap();
            ensure_sql_function(&conn).unwrap();
            assert!(
                conn.query_row("SELECT shelby_edge_active_v1(NULL,NULL,?1)", [NOW], |r| {
                    r.get::<_, bool>(0)
                })
                .unwrap()
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn conflicting_reserved_function_is_not_overwritten() {
        let conn = Connection::open_in_memory().unwrap();
        conn.create_scalar_function(
            "shelby_edge_active_v1",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            |_| Ok(false),
        )
        .unwrap();
        assert!(ensure_sql_function(&conn).is_err());
    }
}

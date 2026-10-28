//! Additive SQLite migrations, including the complete pre-0.4 database history.
use rusqlite::{Connection, params};

use crate::error::Result;
use crate::identity::derive_existing_project_id;
use crate::topics::canonicalize_stored_topics;

pub const CURRENT_SCHEMA_VERSION: i64 = 18;

struct Migration {
    version: i64,
    description: &'static str,
    up: fn(&Connection) -> Result<()>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Initial schema — thoughts, FTS5, edges",
        up: v1,
    },
    Migration {
        version: 2,
        description: "OAuth 2.1 — oauth_clients table",
        up: v2,
    },
    Migration {
        version: 3,
        description: "Temporal edges — valid_from, valid_until",
        up: v3,
    },
    Migration {
        version: 4,
        description: "source_agent and trust_level columns on thoughts",
        up: v4,
    },
    Migration {
        version: 5,
        description: "version-stamp alignment with Shelby-MacOS (no schema change)",
        up: |_| Ok(()),
    },
    Migration {
        version: 6,
        description: "Project identity — project_identifier column + projects registry",
        up: v6,
    },
    Migration {
        version: 7,
        description: "Normalize legacy display-name project_identifiers to registry slugs",
        up: v7,
    },
    Migration {
        version: 8,
        description: "Canonical project identity and slug aliases",
        up: v8,
    },
    Migration {
        version: 9,
        description: "Local search telemetry and rediscovery tracking",
        up: v9,
    },
    Migration {
        version: 10,
        description: "Local KTO-shaped feedback log",
        up: v10,
    },
    Migration {
        version: 11,
        description: "Track thought re-confirmation timestamps",
        up: v11,
    },
    // v12-v17 are occupied by macOS-only tables and columns.
    Migration {
        version: 18,
        description: "Canonicalize legacy thought topics",
        up: v18,
    },
];

pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

pub fn set_schema_version(conn: &Connection, version: i64) -> Result<()> {
    conn.pragma_update(None, "user_version", version)?;
    Ok(())
}

pub fn run_migrations(conn: &Connection) -> Result<()> {
    let current = schema_version(conn)?;
    for m in MIGRATIONS.iter().filter(|m| m.version > current) {
        conn.execute_batch("BEGIN")?;
        let outcome = (m.up)(conn).and_then(|_| set_schema_version(conn, m.version));
        match outcome {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
        eprintln!(
            "[INFO] Migration v{}: {} — complete",
            m.version, m.description
        );
    }
    Ok(())
}

fn v1(c: &Connection) -> Result<()> {
    c.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS thoughts (
          id              TEXT PRIMARY KEY,
          content         TEXT NOT NULL,
          summary         TEXT,
          type            TEXT NOT NULL DEFAULT 'note',
          source          TEXT NOT NULL DEFAULT 'unknown',
          project         TEXT,
          topics          TEXT,
          people          TEXT,
          visibility      TEXT NOT NULL DEFAULT 'personal',
          metadata        TEXT,
          embedding       BLOB,
          created_at      TEXT NOT NULL,
          updated_at      TEXT NOT NULL,
          consolidated_into TEXT,
          reinforcement_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_thoughts_type ON thoughts(type);
        CREATE INDEX IF NOT EXISTS idx_thoughts_project ON thoughts(project);
        CREATE INDEX IF NOT EXISTS idx_thoughts_created ON thoughts(created_at);
        CREATE INDEX IF NOT EXISTS idx_thoughts_updated ON thoughts(updated_at);
        CREATE INDEX IF NOT EXISTS idx_thoughts_consolidated ON thoughts(consolidated_into);

        CREATE VIRTUAL TABLE IF NOT EXISTS thoughts_fts USING fts5(
          content,
          content=thoughts,
          content_rowid=rowid,
          tokenize='porter unicode61'
        );
        CREATE TRIGGER IF NOT EXISTS thoughts_ai AFTER INSERT ON thoughts BEGIN
          INSERT INTO thoughts_fts(rowid, content) VALUES (new.rowid, new.content);
        END;
        CREATE TRIGGER IF NOT EXISTS thoughts_ad AFTER DELETE ON thoughts BEGIN
          INSERT INTO thoughts_fts(thoughts_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
        END;
        CREATE TRIGGER IF NOT EXISTS thoughts_au AFTER UPDATE OF content ON thoughts BEGIN
          INSERT INTO thoughts_fts(thoughts_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
          INSERT INTO thoughts_fts(rowid, content) VALUES (new.rowid, new.content);
        END;

        CREATE TABLE IF NOT EXISTS edges (
          id          TEXT PRIMARY KEY,
          source_id   TEXT NOT NULL,
          target_id   TEXT NOT NULL,
          edge_type   TEXT NOT NULL DEFAULT 'related',
          metadata    TEXT,
          created_at  TEXT NOT NULL,
          FOREIGN KEY (source_id) REFERENCES thoughts(id) ON DELETE CASCADE,
          FOREIGN KEY (target_id) REFERENCES thoughts(id) ON DELETE CASCADE,
          UNIQUE(source_id, target_id, edge_type)
        );
        CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
        CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
        CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);
        "#,
    )?;
    Ok(())
}

fn v2(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS oauth_clients (
          client_id     TEXT PRIMARY KEY,
          client_name   TEXT,
          redirect_uris TEXT NOT NULL,
          registered_at INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

fn v3(c: &Connection) -> Result<()> {
    c.execute_batch(
        "ALTER TABLE edges ADD COLUMN valid_from TEXT;
         ALTER TABLE edges ADD COLUMN valid_until TEXT;
         CREATE INDEX IF NOT EXISTS idx_edges_valid_until ON edges(valid_until);",
    )?;
    Ok(())
}

fn v4(c: &Connection) -> Result<()> {
    c.execute_batch(
        "ALTER TABLE thoughts ADD COLUMN source_agent TEXT;
         ALTER TABLE thoughts ADD COLUMN trust_level TEXT CHECK(trust_level IN ('trusted', 'unverified', 'external')) NOT NULL DEFAULT 'trusted';
         CREATE INDEX IF NOT EXISTS idx_thoughts_trust_level ON thoughts(trust_level);
         CREATE INDEX IF NOT EXISTS idx_thoughts_source_agent ON thoughts(source_agent);",
    )?;
    Ok(())
}

fn v6(c: &Connection) -> Result<()> {
    c.execute_batch(
        "ALTER TABLE thoughts ADD COLUMN project_identifier TEXT;
         CREATE INDEX IF NOT EXISTS idx_thoughts_project_identifier ON thoughts(project_identifier);
         CREATE TABLE IF NOT EXISTS projects (
           slug          TEXT PRIMARY KEY,
           display_name  TEXT NOT NULL,
           member_repos  TEXT NOT NULL DEFAULT '[]',
           member_paths  TEXT NOT NULL DEFAULT '[]',
           provisional   INTEGER NOT NULL DEFAULT 0,
           created_at    TEXT NOT NULL,
           updated_at    TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_projects_provisional ON projects(provisional);",
    )?;
    Ok(())
}

fn v7(c: &Connection) -> Result<()> {
    for (from, to) in [
        ("Shelby", "shelby"),
        ("The Crooked Line", "the-crooked-line"),
        ("KUOW Games", "kuow-games"),
        ("Ausra Photos", "ausra-photos"),
    ] {
        c.execute(
            "UPDATE thoughts SET project_identifier = ?1 WHERE project_identifier = ?2",
            params![to, from],
        )?;
    }
    c.execute(
        "UPDATE thoughts SET project_identifier = NULL WHERE project_identifier = ''",
        [],
    )?;
    Ok(())
}

fn v8(c: &Connection) -> Result<()> {
    let slugs: Vec<String> = c
        .prepare("SELECT slug FROM projects ORDER BY slug")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let mut identities = Vec::with_capacity(slugs.len());
    for slug in slugs {
        let id = derive_existing_project_id(&slug)
            .ok_or_else(|| crate::Error::InvalidInput(format!("invalid_legacy_slug: {slug}")))?;
        identities.push((slug, id));
    }
    c.execute_batch(
        "ALTER TABLE projects ADD COLUMN project_id TEXT;
         ALTER TABLE projects ADD COLUMN current_slug TEXT;
         ALTER TABLE projects ADD COLUMN identity_state TEXT;
         ALTER TABLE thoughts ADD COLUMN project_id TEXT;
         CREATE TABLE project_slug_aliases (
           slug        TEXT PRIMARY KEY,
           project_id  TEXT NOT NULL,
           status      TEXT NOT NULL CHECK(status IN ('tentative', 'current', 'retired')),
           claimed_at  TEXT NOT NULL,
           retired_at  TEXT,
           UNIQUE(project_id, slug)
         );
         CREATE UNIQUE INDEX one_current_slug_per_project
           ON project_slug_aliases(project_id) WHERE status = 'current';
         CREATE INDEX idx_thoughts_project_id ON thoughts(project_id);",
    )?;
    for (slug, project_id) in &identities {
        c.execute(
            "UPDATE projects SET project_id = ?1, current_slug = slug, identity_state = 'local_only' WHERE slug = ?2",
            params![project_id, slug],
        )?;
        c.execute(
            "INSERT INTO project_slug_aliases (slug, project_id, status, claimed_at)
             SELECT slug, ?1, 'tentative', created_at FROM projects WHERE slug = ?2",
            params![project_id, slug],
        )?;
    }
    c.execute(
        "UPDATE thoughts SET project_id = (
           SELECT projects.project_id FROM projects WHERE projects.slug = thoughts.project_identifier
         ) WHERE EXISTS (SELECT 1 FROM projects WHERE projects.slug = thoughts.project_identifier)",
        [],
    )?;
    c.execute_batch(
        "ALTER TABLE projects RENAME TO projects_v7;
         CREATE TABLE projects (
           slug            TEXT PRIMARY KEY,
           display_name    TEXT NOT NULL,
           member_repos    TEXT NOT NULL DEFAULT '[]',
           member_paths    TEXT NOT NULL DEFAULT '[]',
           provisional     INTEGER NOT NULL DEFAULT 0,
           created_at      TEXT NOT NULL,
           updated_at      TEXT NOT NULL,
           project_id      TEXT NOT NULL UNIQUE,
           current_slug    TEXT NOT NULL UNIQUE,
           identity_state  TEXT NOT NULL CHECK(identity_state IN ('local_only', 'pending', 'active', 'collision'))
         );
         INSERT INTO projects (slug, display_name, member_repos, member_paths, provisional,
                               created_at, updated_at, project_id, current_slug, identity_state)
         SELECT slug, display_name, member_repos, member_paths, provisional,
                created_at, updated_at, project_id, current_slug, identity_state
         FROM projects_v7;
         DROP TABLE projects_v7;
         CREATE INDEX idx_projects_provisional ON projects(provisional);",
    )?;
    Ok(())
}

fn v9(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS search_telemetry (
           id TEXT PRIMARY KEY,
           created_at TEXT NOT NULL,
           query_hash TEXT,
           mode TEXT,
           result_count INTEGER NOT NULL DEFAULT 0,
           top_ids TEXT,
           rediscovery INTEGER NOT NULL DEFAULT 0,
           project_identifier TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_search_telemetry_created ON search_telemetry(created_at);",
    )?;
    Ok(())
}

fn v10(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS feedback (
           id TEXT PRIMARY KEY,
           created_at TEXT NOT NULL,
           feature TEXT NOT NULL,
           variant_id TEXT,
           label TEXT NOT NULL,
           prompt_hash TEXT,
           response_hash TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_feedback_variant ON feedback(variant_id);",
    )?;
    Ok(())
}

fn v11(c: &Connection) -> Result<()> {
    c.execute_batch("ALTER TABLE thoughts ADD COLUMN last_confirmed_at TEXT;")?;
    Ok(())
}

fn v18(c: &Connection) -> Result<()> {
    let rows: Vec<(String, String)> = c
        .prepare("SELECT id, topics FROM thoughts WHERE topics IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (id, topics) in rows {
        if let Some(canonical) = canonicalize_stored_topics(&topics)
            && canonical != topics
        {
            c.execute(
                "UPDATE thoughts SET topics = ?1 WHERE id = ?2",
                params![canonical, id],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        run_migrations(&c).unwrap();
        c
    }

    fn columns(c: &Connection, table: &str) -> Vec<String> {
        c.prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn fresh_db_reaches_v18_with_contract_columns() {
        let c = fresh();
        assert_eq!(schema_version(&c).unwrap(), CURRENT_SCHEMA_VERSION);
        let t = columns(&c, "thoughts");
        for col in [
            "source_agent",
            "trust_level",
            "project_identifier",
            "project_id",
            "last_confirmed_at",
            "reinforcement_count",
        ] {
            assert!(t.contains(&col.to_string()), "missing {col}");
        }
        let p = columns(&c, "projects");
        for col in ["project_id", "current_slug", "identity_state"] {
            assert!(p.contains(&col.to_string()), "missing {col}");
        }
        assert!(columns(&c, "edges").contains(&"valid_until".to_string()));
        run_migrations(&c).unwrap(); // idempotent
    }

    #[test]
    fn v7_and_v8_backfill_legacy_rows_like_the_ts_engine() {
        let c = Connection::open_in_memory().unwrap();
        // Run to v6, seed legacy data, then finish.
        let sub: Vec<&Migration> = MIGRATIONS.iter().filter(|m| m.version <= 6).collect();
        for m in sub {
            (m.up)(&c).unwrap();
            set_schema_version(&c, m.version).unwrap();
        }
        c.execute(
            "INSERT INTO projects (slug, display_name, created_at, updated_at) VALUES ('shelby','Shelby','2026-01-01T00:00:00.000Z','2026-01-01T00:00:00.000Z')",
            [],
        ).unwrap();
        c.execute(
            "INSERT INTO thoughts (id, content, project_identifier, created_at, updated_at, topics) VALUES ('t1','x','Shelby','2026-01-01T00:00:00.000Z','2026-01-01T00:00:00.000Z','[\"Knowledge Graph\"]')",
            [],
        ).unwrap();
        run_migrations(&c).unwrap();
        let (pid, slug, topics): (String, String, String) = c
            .query_row(
                "SELECT project_id, project_identifier, topics FROM thoughts WHERE id='t1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(pid, "4abbc729-70e8-5120-9cfc-bd34739c024e"); // fixture vector for "shelby"
        assert_eq!(slug, "shelby");
        assert_eq!(topics, r#"["knowledge-graph"]"#);
        let status: String = c
            .query_row(
                "SELECT status FROM project_slug_aliases WHERE slug='shelby'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "tentative");
    }
}

use super::*;
const PROJECTS: &str = "CREATE TABLE projects (slug TEXT PRIMARY KEY,display_name TEXT NOT NULL,member_repos TEXT NOT NULL DEFAULT '[]',member_paths TEXT NOT NULL DEFAULT '[]',provisional INTEGER NOT NULL DEFAULT 0,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,project_id TEXT NOT NULL UNIQUE,current_slug TEXT NOT NULL UNIQUE,identity_state TEXT NOT NULL CHECK(identity_state IN ('local_only', 'pending', 'active', 'collision')))";
const ALIASES: &str = "CREATE TABLE project_slug_aliases (slug TEXT PRIMARY KEY,project_id TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('tentative', 'current', 'retired')),claimed_at TEXT NOT NULL,retired_at TEXT,UNIQUE(project_id, slug))";
const PROVISIONAL: &str = "CREATE INDEX idx_projects_provisional ON projects(provisional)";
const CURRENT: &str = "CREATE UNIQUE INDEX one_current_slug_per_project ON project_slug_aliases(project_id) WHERE status = 'current'";
fn shape(sql: &str) -> String {
    let mut quoted = false;
    sql.chars()
        .filter(|c| {
            if *c == '\'' {
                quoted = !quoted;
            }
            quoted || !c.is_ascii_whitespace()
        })
        .collect()
}
pub(super) fn check(conn: &Connection) -> Result<()> {
    if crate::migrations::schema_version(conn).map_err(|_| ProjectMembershipError::Unavailable)?
        != 18
    {
        return Err(ProjectMembershipError::Unavailable);
    }
    let predicate = "tbl_name IN ('projects','project_slug_aliases') OR name IN ('projects','project_slug_aliases','idx_projects_provisional','one_current_slug_per_project','sqlite_autoindex_projects_1','sqlite_autoindex_projects_2','sqlite_autoindex_projects_3','sqlite_autoindex_project_slug_aliases_1','sqlite_autoindex_project_slug_aliases_2')";
    if conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM temp.sqlite_schema WHERE {predicate})"),
        [],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(ProjectMembershipError::Unavailable);
    }
    let mut q=conn.prepare(&format!("SELECT type,CASE WHEN length(CAST(name AS BLOB))<=128 THEN name ELSE NULL END,CASE WHEN length(CAST(tbl_name AS BLOB))<=128 THEN tbl_name ELSE NULL END,CASE WHEN length(CAST(sql AS BLOB))<=65536 THEN sql ELSE NULL END FROM main.sqlite_schema WHERE {predicate} ORDER BY name LIMIT 10"))?;
    let actual = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let expected = [
        (
            "index",
            "idx_projects_provisional",
            "projects",
            Some(PROVISIONAL),
        ),
        (
            "index",
            "one_current_slug_per_project",
            "project_slug_aliases",
            Some(CURRENT),
        ),
        (
            "table",
            "project_slug_aliases",
            "project_slug_aliases",
            Some(ALIASES),
        ),
        ("table", "projects", "projects", Some(PROJECTS)),
        (
            "index",
            "sqlite_autoindex_project_slug_aliases_1",
            "project_slug_aliases",
            None,
        ),
        (
            "index",
            "sqlite_autoindex_project_slug_aliases_2",
            "project_slug_aliases",
            None,
        ),
        ("index", "sqlite_autoindex_projects_1", "projects", None),
        ("index", "sqlite_autoindex_projects_2", "projects", None),
        ("index", "sqlite_autoindex_projects_3", "projects", None),
    ];
    if actual.len() != expected.len()
        || actual.iter().zip(expected).any(|(a, e)| {
            a.0 != e.0 || a.1 != e.1 || a.2 != e.2 || a.3.as_deref().map(shape) != e.3.map(shape)
        })
    {
        return Err(ProjectMembershipError::Unavailable);
    }
    columns(
        conn,
        "projects",
        &[
            ("slug", "TEXT", 0, None, 1),
            ("display_name", "TEXT", 1, None, 0),
            ("member_repos", "TEXT", 1, Some("'[]'"), 0),
            ("member_paths", "TEXT", 1, Some("'[]'"), 0),
            ("provisional", "INTEGER", 1, Some("0"), 0),
            ("created_at", "TEXT", 1, None, 0),
            ("updated_at", "TEXT", 1, None, 0),
            ("project_id", "TEXT", 1, None, 0),
            ("current_slug", "TEXT", 1, None, 0),
            ("identity_state", "TEXT", 1, None, 0),
        ],
    )?;
    columns(
        conn,
        "project_slug_aliases",
        &[
            ("slug", "TEXT", 0, None, 1),
            ("project_id", "TEXT", 1, None, 0),
            ("status", "TEXT", 1, None, 0),
            ("claimed_at", "TEXT", 1, None, 0),
            ("retired_at", "TEXT", 0, None, 0),
        ],
    )?;
    indexes(
        conn,
        "projects",
        &[
            ("idx_projects_provisional", 0, "c", 0, &[(4, "provisional")]),
            ("sqlite_autoindex_projects_1", 1, "pk", 0, &[(0, "slug")]),
            (
                "sqlite_autoindex_projects_2",
                1,
                "u",
                0,
                &[(7, "project_id")],
            ),
            (
                "sqlite_autoindex_projects_3",
                1,
                "u",
                0,
                &[(8, "current_slug")],
            ),
        ],
    )?;
    indexes(
        conn,
        "project_slug_aliases",
        &[
            (
                "one_current_slug_per_project",
                1,
                "c",
                1,
                &[(1, "project_id")],
            ),
            (
                "sqlite_autoindex_project_slug_aliases_1",
                1,
                "pk",
                0,
                &[(0, "slug")],
            ),
            (
                "sqlite_autoindex_project_slug_aliases_2",
                1,
                "u",
                0,
                &[(1, "project_id"), (0, "slug")],
            ),
        ],
    )?;
    Ok(())
}
type Column<'a> = (&'a str, &'a str, i64, Option<&'a str>, i64);
fn columns(conn: &Connection, table: &str, expected: &[Column<'_>]) -> Result<()> {
    let mut q=conn.prepare("SELECT cid,name,type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo(?1) ORDER BY cid LIMIT 11")?;
    let actual = q
        .query_map([table], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if actual.len() != expected.len()
        || actual.iter().zip(expected).enumerate().any(|(i, (a, e))| {
            a.0 != i as i64
                || a.1 != e.0
                || a.2 != e.1
                || a.3 != e.2
                || a.4.as_deref() != e.3
                || a.5 != e.4
                || a.6 != 0
        })
    {
        return Err(ProjectMembershipError::Unavailable);
    }
    Ok(())
}
type Index<'a> = (&'a str, i64, &'a str, i64, &'a [(i64, &'a str)]);
fn indexes(conn: &Connection, table: &str, expected: &[Index<'_>]) -> Result<()> {
    let mut q = conn.prepare(
        "SELECT name,\"unique\",origin,partial FROM pragma_index_list(?1) ORDER BY name LIMIT 5",
    )?;
    let actual = q
        .query_map([table], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if actual.len() != expected.len() {
        return Err(ProjectMembershipError::Unavailable);
    }
    for (a, e) in actual.iter().zip(expected) {
        if a.0 != e.0 || a.1 != e.1 || a.2 != e.2 || a.3 != e.3 {
            return Err(ProjectMembershipError::Unavailable);
        }
        let mut q=conn.prepare("SELECT seqno,cid,name,\"desc\",coll,\"key\" FROM pragma_index_xinfo(?1) ORDER BY seqno LIMIT 4")?;
        let cols = q
            .query_map([e.0], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if cols.len() != e.4.len() + 1 {
            return Err(ProjectMembershipError::Unavailable);
        }
        for (i, c) in cols.iter().enumerate() {
            let last = i == e.4.len();
            if c.0 != i as i64
                || c.1 != if last { -1 } else { e.4[i].0 }
                || c.2.as_deref() != if last { None } else { Some(e.4[i].1) }
                || c.3 != 0
                || c.4 != "BINARY"
                || c.5 != if last { 0 } else { 1 }
            {
                return Err(ProjectMembershipError::Unavailable);
            }
        }
    }
    Ok(())
}

//! Strict existing-project membership snapshots and conditional local updates.
use rusqlite::{Connection, params};
mod rows;
mod schema;
type Result<T> = std::result::Result<T, ProjectMembershipError>;
impl From<rusqlite::Error> for ProjectMembershipError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProjectMembershipError {
    #[error("Invalid project membership request")]
    InvalidInput,
    #[error("Project no longer exists")]
    NotFound,
    #[error("Project membership changed; read it again")]
    Conflict,
    #[error("Project membership is unavailable")]
    Unavailable,
    #[error("Project membership outcome is unknown; read it again")]
    OutcomeUnknown,
    #[error("Project connection is unusable; retain ownership and restart")]
    UnusableConnection,
}
#[derive(Debug)]
pub struct ProjectMembershipSnapshot {
    raw: rows::ProjectRow,
    aliases: Vec<rows::Alias>,
    repositories: Vec<String>,
    fingerprint: String,
}
impl ProjectMembershipSnapshot {
    pub fn project_id(&self) -> &str {
        &self.raw.project_id
    }
    pub fn display_name(&self) -> &str {
        &self.raw.display_name
    }
    pub fn repositories(&self) -> &[String] {
        &self.repositories
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}
#[derive(Debug)]
pub enum ProjectMembershipUpdate {
    Applied(ProjectMembershipSnapshot),
    Unchanged(ProjectMembershipSnapshot),
}
fn entry(conn: &Connection, project_id: &str) -> Result<()> {
    if project_id.len() != 36
        || !crate::identity::is_canonical_project_id(project_id)
        || !conn.is_autocommit()
    {
        return Err(ProjectMembershipError::InvalidInput);
    }
    Ok(())
}
/// Clean only a transaction this API began; callers must quarantine after this error.
fn cleanup(conn: &Connection) -> Result<()> {
    if !conn.is_autocommit() && (conn.execute_batch("ROLLBACK").is_err() || !conn.is_autocommit()) {
        return Err(ProjectMembershipError::UnusableConnection);
    }
    Ok(())
}
fn begin(conn: &Connection, sql: &str) -> Result<()> {
    if conn.execute_batch(sql).is_err() {
        cleanup(conn)?;
        return Err(ProjectMembershipError::Unavailable);
    }
    Ok(())
}
/// Requires exclusive caller ownership and no existing transaction. Owns one checked read snapshot.
/// UnusableConnection requires retaining/quarantining the caller's actual connection owner.
pub fn inspect_project_membership(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<ProjectMembershipSnapshot>> {
    entry(conn, project_id)?;
    begin(conn, "BEGIN DEFERRED")?;
    let result = rows::read(conn, project_id);
    cleanup(conn)?;
    result
}
/// Existing-ID-only full-content CAS. Only member_repos and updated_at may change.
/// A reported failure never authorizes replay; OutcomeUnknown requires an authoritative read.
/// UnusableConnection requires retaining/quarantining the caller's actual connection owner.
pub fn replace_project_repositories(
    conn: &Connection,
    project_id: &str,
    expected_fingerprint: &str,
    repositories: &[String],
) -> Result<ProjectMembershipUpdate> {
    entry(conn, project_id)?;
    if expected_fingerprint.len() != 64
        || !expected_fingerprint
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ProjectMembershipError::InvalidInput);
    }
    rows::input_array(repositories)?;
    begin(conn, "BEGIN IMMEDIATE")?;
    let prepared = (|| {
        let before = rows::read(conn, project_id)?.ok_or(ProjectMembershipError::NotFound)?;
        if before.fingerprint() != expected_fingerprint {
            return Err(ProjectMembershipError::Conflict);
        }
        if before.repositories() == repositories {
            return Ok((false, before));
        }
        let json = serde_json::to_string(repositories)
            .map_err(|_| ProjectMembershipError::InvalidInput)?;
        if json.len() > 262144 {
            return Err(ProjectMembershipError::InvalidInput);
        }
        let mut intended = before;
        intended.raw.member_repos = json;
        intended.raw.updated_at = crate::now_iso();
        intended.repositories = repositories.to_vec();
        intended.fingerprint = rows::fingerprint(&intended.raw, &intended.aliases);
        if conn.execute(
            "UPDATE main.projects SET member_repos=?2,updated_at=?3 WHERE project_id=?1",
            params![
                project_id,
                intended.raw.member_repos,
                intended.raw.updated_at
            ],
        )? != 1
        {
            return Err(ProjectMembershipError::Unavailable);
        }
        let after = rows::read(conn, project_id)?.ok_or(ProjectMembershipError::Unavailable)?;
        if after.fingerprint() != intended.fingerprint() {
            return Err(ProjectMembershipError::Unavailable);
        }
        Ok((true, intended))
    })();
    let (changed, intended) = match prepared {
        Ok(v) => v,
        Err(e) => {
            cleanup(conn)?;
            return Err(e);
        }
    };
    if !changed {
        cleanup(conn)?;
        return Ok(ProjectMembershipUpdate::Unchanged(intended));
    }
    if conn.execute_batch("COMMIT").is_ok() {
        if !conn.is_autocommit() {
            cleanup(conn)?;
            return Err(ProjectMembershipError::UnusableConnection);
        }
        return Ok(ProjectMembershipUpdate::Applied(intended));
    }
    cleanup(conn)?;
    // Fresh evidence uses the same caller-held connection, never a second connection or repeat UPDATE.
    match inspect_project_membership(conn, project_id) {
        Ok(Some(current)) if current.fingerprint() == intended.fingerprint() => {
            Ok(ProjectMembershipUpdate::Applied(current))
        }
        Ok(Some(current)) if current.fingerprint() == expected_fingerprint => {
            Err(ProjectMembershipError::Unavailable)
        }
        Err(ProjectMembershipError::UnusableConnection) => {
            Err(ProjectMembershipError::UnusableConnection)
        }
        _ => Err(ProjectMembershipError::OutcomeUnknown),
    }
}
#[cfg(test)]
mod tests;

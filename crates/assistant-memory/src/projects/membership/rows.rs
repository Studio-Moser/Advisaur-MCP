use super::*;
use rusqlite::{Row, types::ValueRef};
use serde::de::{self, DeserializeSeed, SeqAccess, Visitor};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug)]
pub(super) struct ProjectRow {
    pub slug: String,
    pub display_name: String,
    pub member_repos: String,
    pub member_paths: String,
    pub provisional: i64,
    pub created_at: String,
    pub updated_at: String,
    pub project_id: String,
    pub current_slug: String,
    pub identity_state: String,
}
#[derive(Debug)]
pub(super) struct Alias {
    slug: String,
    project_id: String,
    status: String,
    claimed_at: String,
    retired_at: Option<String>,
}
pub(super) fn input_array(values: &[String]) -> Result<()> {
    if values.len() > 100
        || values.iter().any(|s| s.len() > 2048)
        || values.iter().map(String::len).sum::<usize>() > 32768
    {
        return Err(ProjectMembershipError::InvalidInput);
    }
    Ok(())
}
struct Strings;
impl<'de> DeserializeSeed<'de> for Strings {
    type Value = Vec<String>;
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        d: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct Array;
        impl<'de> Visitor<'de> for Array {
            type Value = Vec<String>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded string array")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                let mut bytes = 0;
                while let Some(value) = seq.next_element_seed(BoundedString)? {
                    bytes += value.len();
                    if out.len() == 100 || bytes > 32768 {
                        return Err(de::Error::custom("array bound"));
                    }
                    out.push(value);
                }
                Ok(out)
            }
        }
        d.deserialize_seq(Array)
    }
}
struct BoundedString;
impl<'de> DeserializeSeed<'de> for BoundedString {
    type Value = String;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> std::result::Result<String, D::Error> {
        struct Text;
        impl Visitor<'_> for Text {
            type Value = String;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded string")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<String, E> {
                if value.len() > 2048 {
                    return Err(E::custom("string bound"));
                }
                Ok(value.to_owned())
            }
        }
        d.deserialize_str(Text)
    }
}
fn array(raw: &str) -> Result<Vec<String>> {
    let mut decoder = serde_json::Deserializer::from_str(raw);
    let out = Strings
        .deserialize(&mut decoder)
        .map_err(|_| ProjectMembershipError::Unavailable)?;
    decoder
        .end()
        .map_err(|_| ProjectMembershipError::Unavailable)?;
    Ok(out)
}
fn text(row: &Row<'_>, i: usize, max: usize, nonempty: bool) -> Result<String> {
    match row.get_ref(i)? {
        ValueRef::Text(bytes) if bytes.len() <= max && (!nonempty || !bytes.is_empty()) => {
            Ok(std::str::from_utf8(bytes)
                .map_err(|_| ProjectMembershipError::Unavailable)?
                .to_owned())
        }
        _ => Err(ProjectMembershipError::Unavailable),
    }
}
fn guarded(name: &str, max: usize) -> String {
    format!(
        "CASE WHEN typeof({name})='text' AND length(CAST({name} AS BLOB))<={max} THEN {name} ELSE NULL END"
    )
}
pub(super) fn read(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<ProjectMembershipSnapshot>> {
    schema::check(conn)?;
    let fields = [
        ("slug", 128),
        ("display_name", 512),
        ("member_repos", 262144),
        ("member_paths", 262144),
        ("created_at", 128),
        ("updated_at", 128),
        ("project_id", 36),
        ("current_slug", 128),
        ("identity_state", 10),
    ]
    .map(|(name, max)| guarded(name, max));
    let mut query=conn.prepare(&format!("SELECT {},CASE WHEN typeof(provisional)='integer' THEN provisional ELSE NULL END FROM main.projects WHERE project_id=?1 LIMIT 2",fields.join(",")))?;
    let mut records = query.query([project_id])?;
    let Some(row) = records.next()? else {
        return Ok(None);
    };
    let provisional = match row.get_ref(9)? {
        ValueRef::Integer(v @ 0..=1) => v,
        _ => return Err(ProjectMembershipError::Unavailable),
    };
    let raw = ProjectRow {
        slug: text(row, 0, 128, true)?,
        display_name: text(row, 1, 512, false)?,
        member_repos: text(row, 2, 262144, true)?,
        member_paths: text(row, 3, 262144, true)?,
        created_at: text(row, 4, 128, true)?,
        updated_at: text(row, 5, 128, true)?,
        project_id: text(row, 6, 36, true)?,
        current_slug: text(row, 7, 128, true)?,
        identity_state: text(row, 8, 10, true)?,
        provisional,
    };
    if records.next()?.is_some() {
        return Err(ProjectMembershipError::Unavailable);
    }
    drop(records);
    drop(query);
    if !crate::identity::is_valid_project_slug(&raw.slug)
        || !crate::identity::is_valid_project_slug(&raw.current_slug)
        || !crate::identity::is_canonical_project_id(&raw.project_id)
        || raw.project_id != project_id
        || (raw.slug != raw.project_id
            && crate::identity::derive_existing_project_id(&raw.slug).as_deref()
                != Some(&raw.project_id))
        || !matches!(
            raw.identity_state.as_str(),
            "local_only" | "pending" | "active" | "collision"
        )
    {
        return Err(ProjectMembershipError::Unavailable);
    }
    let repositories = array(&raw.member_repos)?;
    array(&raw.member_paths)?;
    // Two indexed identity checks plus one owner-index scan keep the 101-row sentinel bounded.
    if conn.query_row("SELECT EXISTS(SELECT 1 FROM main.project_slug_aliases WHERE slug IN (?2,?3) AND project_id IS NOT ?1)",rusqlite::params![project_id,raw.slug,raw.current_slug],|r|r.get::<_,bool>(0))? { return Err(ProjectMembershipError::Unavailable); }
    let mut query=conn.prepare(&format!("SELECT {},{},{},{},CASE WHEN retired_at IS NULL THEN NULL WHEN typeof(retired_at)='text' AND length(CAST(retired_at AS BLOB))<=128 THEN retired_at ELSE X'' END FROM main.project_slug_aliases WHERE project_id=?1 ORDER BY slug COLLATE BINARY LIMIT 101",guarded("slug",128),guarded("project_id",36),guarded("status",9),guarded("claimed_at",128)))?;
    let mut records = query.query([project_id])?;
    let mut aliases = Vec::new();
    let mut bytes = 0;
    while let Some(row) = records.next()? {
        if aliases.len() == 100 {
            return Err(ProjectMembershipError::Unavailable);
        }
        let alias = Alias {
            slug: text(row, 0, 128, true)?,
            project_id: text(row, 1, 36, true)?,
            status: text(row, 2, 9, true)?,
            claimed_at: text(row, 3, 128, true)?,
            retired_at: match row.get_ref(4)? {
                ValueRef::Null => None,
                _ => Some(text(row, 4, 128, true)?),
            },
        };
        if !crate::identity::is_valid_project_slug(&alias.slug)
            || !crate::identity::is_canonical_project_id(&alias.project_id)
            || alias.project_id != project_id
            || !matches!(alias.status.as_str(), "tentative" | "current" | "retired")
        {
            return Err(ProjectMembershipError::Unavailable);
        }
        bytes += alias.slug.len()
            + alias.project_id.len()
            + alias.status.len()
            + alias.claimed_at.len()
            + alias.retired_at.as_ref().map_or(0, String::len);
        if bytes > 65536 {
            return Err(ProjectMembershipError::Unavailable);
        }
        aliases.push(alias);
    }
    let fingerprint = fingerprint(&raw, &aliases);
    Ok(Some(ProjectMembershipSnapshot {
        raw,
        aliases,
        repositories,
        fingerprint,
    }))
}
fn hash_text(h: &mut Sha256, s: &str) {
    h.update([2]);
    h.update((s.len() as u64).to_be_bytes());
    h.update(s.as_bytes());
}
pub(super) fn fingerprint(p: &ProjectRow, aliases: &[Alias]) -> String {
    let mut h = Sha256::new();
    h.update(b"shelby-project-membership-snapshot-v1\0");
    for value in [&p.slug, &p.display_name, &p.member_repos, &p.member_paths] {
        hash_text(&mut h, value);
    }
    h.update([1]);
    h.update(p.provisional.to_be_bytes());
    for value in [
        &p.created_at,
        &p.updated_at,
        &p.project_id,
        &p.current_slug,
        &p.identity_state,
    ] {
        hash_text(&mut h, value);
    }
    h.update((aliases.len() as u64).to_be_bytes());
    for a in aliases {
        for value in [&a.slug, &a.project_id, &a.status, &a.claimed_at] {
            hash_text(&mut h, value);
        }
        match &a.retired_at {
            Some(value) => hash_text(&mut h, value),
            None => h.update([0]),
        }
    }
    format!("{:x}", h.finalize())
}

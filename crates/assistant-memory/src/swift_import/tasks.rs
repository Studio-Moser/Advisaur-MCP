//! Local tasks remain validated source projections, never canonical entities.
use super::*;
use serde::Deserialize;
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedLocalTask {
    pub source_key: String,
    pub task_id: String,
    pub project_id: String,
    pub title: String,
    pub state: String,
    pub origin: String,
    pub triage_state: String,
    pub updated_at: String,
    pub fingerprint: String,
    pub source_payload: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedTaskExcluded {
    pub external_tracker: u32,
    pub epic: u32,
    pub dismissed: u32,
    pub snoozed: u32,
    pub parent: u32,
    pub dependencies: u32,
    pub labels: u32,
    pub external_origin: u32,
}
impl PreparedTaskExcluded {
    fn total(&self) -> Result<u32> {
        let counts = [
            self.external_tracker,
            self.epic,
            self.dismissed,
            self.snoozed,
            self.parent,
            self.dependencies,
            self.labels,
            self.external_origin,
        ];
        if counts.iter().any(|v| *v > 2000) {
            return Err(invalid("task counts"));
        }
        Ok(counts.iter().sum())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedTaskOmissions {
    pub url: u32,
    pub origin_session_id: u32,
    pub last_seen_at: u32,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedTaskPolicy {
    pub due_dates: String,
    pub sessions: String,
    pub omitted_source_fields: Vec<String>,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedTaskSelection {
    pub count: u32,
    pub supported_count: u32,
    pub unselected_count: u32,
    pub excluded: PreparedTaskExcluded,
    pub omissions: PreparedTaskOmissions,
    pub task_policy: PreparedTaskPolicy,
}
fn project_task(t: &wire::LocalTask) -> Result<PreparedLocalTask> {
    let id = uuid(&t.external_id)?;
    let project = uuid(&t.project_id)?;
    let source_key = format!("workItem:local:{id}");
    if t.source_key.0 != source_key
        || &*t.tracker != "local"
        || &*t.kind != "todo"
        || t.title.trim().is_empty()
        || t.title.chars().count() > 500
        || !matches!(t.state.to_ascii_lowercase().as_str(), "open" | "closed")
        || t.snooze_until.is_some()
        || t.parent_external_id.is_some()
        || t.omissions.url > 1
        || t.omissions.origin_session_id > 1
        || t.omissions.last_seen_at > 1
    {
        return Err(invalid("local task"));
    }
    one_of(&t.origin, &["manual", "agent_spawned"], "task origin")?;
    one_of(&t.triage_state, &["inbox", "accepted"], "task triage")?;
    date(&t.updated_at)?;
    if uuid(&t.project_alias).is_err() {
        slug(&t.project_alias)?;
    }
    let mut identity = serde_json::to_value(t).map_err(|_| invalid("serialization"))?;
    identity["externalId"] = json!(id);
    identity["projectId"] = json!(project);
    identity["sourceKey"] = json!(source_key);
    Ok(PreparedLocalTask {
        source_key,
        task_id: id,
        project_id: project,
        title: t.title.0.clone(),
        state: t.state.0.clone(),
        origin: t.origin.0.clone(),
        triage_state: t.triage_state.0.clone(),
        updated_at: t.updated_at.0.clone(),
        fingerprint: hash(&json!(["shelby-swift-local-task-v1", identity]))?,
        source_payload: encoded(t)?,
    })
}
pub(super) fn prepare(
    package: &wire::Package,
    projects: &BTreeMap<String, String>,
    aliases: &BTreeMap<String, (String, String)>,
) -> Result<(Vec<PreparedLocalTask>, Option<PreparedTaskSelection>)> {
    let category = &package.categories.tasks;
    if package.version == 1 {
        if package.local_tasks.is_some()
            || package.task_policy.is_some()
            || category.count != 0
            || &*category.status != "notSelected"
            || category.supported_count.is_some()
            || category.unselected_count.is_some()
            || category.excluded.is_some()
        {
            return Err(invalid("v1 task fields"));
        }
        return Ok((vec![], None));
    }
    let tasks = package
        .local_tasks
        .as_ref()
        .ok_or_else(|| invalid("task selection"))?;
    let policy = package
        .task_policy
        .as_ref()
        .ok_or_else(|| invalid("task policy"))?;
    let fields: BTreeSet<&str> = policy.omitted_source_fields.iter().map(|s| &**s).collect();
    if &*policy.due_dates != "notInferred"
        || &*policy.sessions != "notRead"
        || policy.omitted_source_fields.len() != 4
        || fields != BTreeSet::from(["url", "originSessionId", "lastSeenAt", "sessionLinks"])
    {
        return Err(invalid("task policy"));
    }
    let supported = category
        .supported_count
        .ok_or_else(|| invalid("task counts"))?;
    let unselected = category
        .unselected_count
        .ok_or_else(|| invalid("task counts"))?;
    let excluded = category
        .excluded
        .clone()
        .ok_or_else(|| invalid("task counts"))?;
    if category.count > 2000
        || supported > 2000
        || unselected > 2000
        || category.count as usize != tasks.len()
        || &*category.status != "selectedSubset"
        || supported != category.count + unselected
        || supported + excluded.total()? > 2000
    {
        return Err(invalid("task counts"));
    }
    let mut result = Vec::new();
    let mut keys = BTreeSet::new();
    let mut omissions = PreparedTaskOmissions::default();
    for t in tasks.iter() {
        let projected = project_task(t)?;
        let alias_owned = matches!(aliases.get(&t.project_alias.0),Some((owner,_)) if owner==&projected.project_id);
        let frozen_owned = package.projects.iter().any(|p| {
            uuid(&p.source_id).ok().as_ref() == Some(&projected.project_id)
                && uuid(&p.legacy_slug).ok() == Some(projected.project_id.clone())
                && uuid(&t.project_alias).ok() == Some(projected.project_id.clone())
        });
        if !projects.contains_key(&projected.project_id)
            || (!alias_owned && !frozen_owned)
            || !keys.insert(projected.source_key.clone())
        {
            return Err(invalid("task project or duplicate identity"));
        }
        omissions.url += t.omissions.url;
        omissions.origin_session_id += t.omissions.origin_session_id;
        omissions.last_seen_at += t.omissions.last_seen_at;
        result.push(projected);
    }
    Ok((
        result,
        Some(PreparedTaskSelection {
            count: category.count,
            supported_count: supported,
            unselected_count: unselected,
            excluded,
            omissions,
            task_policy: PreparedTaskPolicy {
                due_dates: policy.due_dates.0.clone(),
                sessions: policy.sessions.0.clone(),
                omitted_source_fields: fields.into_iter().map(str::to_owned).collect(),
            },
        }),
    ))
}

/// Validate inert stored task provenance and recompute its intrinsic fingerprint.
/// This does not authorize import or establish selected-project/alias ownership.
/// Only `prepare` performs that cross-record check for an immutable PreparedImport.
pub fn inspect_local_task_source(payload: &str) -> Result<PreparedLocalTask> {
    if payload.len() > 32 * 1024 {
        return Err(invalid("task source size"));
    }
    let task: wire::LocalTask =
        serde_json::from_str(payload).map_err(|_| invalid("task source shape or bounds"))?;
    project_task(&task)
}

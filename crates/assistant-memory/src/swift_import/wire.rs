//! Allocation-bounded wire fields; object derives reject duplicates and unknown keys.
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, SeqAccess, Visitor},
};
use std::{fmt, ops::Deref};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(super) struct Text<const N: usize>(pub String);
impl<const N: usize> Deref for Text<N> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl<'de, const N: usize> Deserialize<'de> for Text<N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<const N: usize>;
        impl<const N: usize> Visitor<'_> for V<N> {
            type Value = Text<N>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a bounded string")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                if s.len() > N {
                    return Err(E::custom("string limit"));
                }
                Ok(Text(s.to_owned()))
            }
        }
        d.deserialize_str(V::<N>)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub(super) struct List<T, const N: usize>(pub Vec<T>);
impl<T, const N: usize> Deref for List<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}
impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for List<T, N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<T, const N: usize>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for V<T, N> {
            type Value = List<T, N>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a bounded array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut result = Vec::new();
                while result.len() < N {
                    match seq.next_element()? {
                        Some(item) => result.push(item),
                        None => return Ok(List(result)),
                    }
                }
                // Do not deserialize/allocate the over-limit element.
                struct Reject;
                impl<'de> Deserialize<'de> for Reject {
                    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
                        Err(de::Error::custom("array limit"))
                    }
                }
                if seq.next_element::<Reject>()?.is_some() {
                    unreachable!();
                }
                Ok(List(result))
            }
        }
        d.deserialize_seq(V::<T, N>(std::marker::PhantomData))
    }
}

pub(super) type Label = Text<4096>;
pub(super) type Id = Text<512>;
pub(super) type Date = Text<64>;
pub(super) type Labels = List<Label, 256>;
pub(super) fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
pub(super) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Package {
    pub format: Label,
    pub version: u32,
    pub source_product: Label,
    pub source_schema: u32,
    #[serde(deserialize_with = "nullable")]
    pub source_app_version: Option<Label>,
    pub adapter_reference: Label,
    pub export_id: Id,
    pub exported_at: Date,
    pub selected_scopes: List<Id, 202>,
    pub projects: List<Project, 200>,
    pub aliases: List<Alias, 2000>,
    pub memories: List<Thought, 1000>,
    pub edges: List<Edge, 5000>,
    pub categories: Categories,
    pub policy: Policy,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub local_tasks: Option<List<LocalTask, 2000>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub task_policy: Option<TaskPolicy>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub chat_archive: Option<super::chats::wire::Archive>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Project {
    pub source_id: Id,
    pub legacy_slug: Id,
    pub current_slug: Id,
    pub display_name: Label,
    #[serde(deserialize_with = "nullable")]
    pub display_name_override: Option<Label>,
    pub source_identity_state: Label,
    pub pinned: bool,
    pub archived: bool,
    pub provisional: bool,
    pub created_at: Date,
    pub updated_at: Date,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Alias {
    pub slug: Id,
    pub project_id: Id,
    pub status: Label,
    pub claimed_at: Date,
    #[serde(deserialize_with = "nullable")]
    pub retired_at: Option<Date>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Thought {
    pub source_id: Id,
    pub content: Text<1048576>,
    #[serde(deserialize_with = "nullable")]
    pub summary: Option<Text<65536>>,
    pub metadata: Metadata,
    pub omissions: Omissions,
    #[serde(deserialize_with = "nullable")]
    pub source: Option<Label>,
    #[serde(deserialize_with = "nullable")]
    pub source_agent: Option<Label>,
    pub source_trust: Label,
    pub visibility: Label,
    #[serde(deserialize_with = "nullable")]
    pub project_id: Option<Id>,
    #[serde(deserialize_with = "nullable")]
    pub project_alias: Option<Id>,
    pub created_at: Date,
    pub updated_at: Date,
    #[serde(deserialize_with = "nullable")]
    pub last_confirmed_at: Option<Date>,
    pub reinforcement_count: u64,
    #[serde(deserialize_with = "nullable")]
    pub consolidated_into: Option<Id>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metadata {
    #[serde(deserialize_with = "nullable")]
    pub r#type: Option<Label>,
    #[serde(deserialize_with = "nullable")]
    pub topics: Option<Labels>,
    #[serde(deserialize_with = "nullable")]
    pub people: Option<Labels>,
    #[serde(deserialize_with = "nullable")]
    pub action_items: Option<Labels>,
    #[serde(deserialize_with = "nullable")]
    pub dates: Option<Labels>,
    pub extra: Extra,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Extra {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub sensitivity: Option<Label>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub status: Option<Label>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub archived: Option<Label>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub contradiction_resolved: Option<Label>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub contradiction_kept: Option<Label>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Omissions {
    #[serde(
        default,
        rename = "metadata.unknown",
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub unknown: Option<u32>,
    #[serde(
        default,
        rename = "metadata.extra.briefAuthority",
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub authority: Option<u32>,
    #[serde(
        default,
        rename = "metadata.extra.unsupported",
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub unsupported: Option<u32>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Edge {
    pub source_id: Text<{ crate::edges::MAX_EDGE_ID_BYTES }>,
    pub source_memory_id: Id,
    pub target_memory_id: Id,
    pub r#type: Label,
    pub source_weight: f64,
    pub created_at: Date,
    pub updated_at: Date,
    #[serde(deserialize_with = "nullable")]
    pub project_id: Option<Id>,
    #[serde(deserialize_with = "nullable")]
    pub project_alias: Option<Id>,
    #[serde(deserialize_with = "nullable")]
    pub valid_from: Option<Date>,
    #[serde(deserialize_with = "nullable")]
    pub valid_until: Option<Date>,
    pub metadata: Claim,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Claim {
    #[serde(deserialize_with = "nullable")]
    pub claim: Option<Label>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Categories {
    pub projects: Category,
    pub memories: Category,
    pub edges: Category,
    pub tasks: TaskCategory,
    pub conversations: ChatCategory,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Category {
    pub count: u32,
    pub status: Label,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Policy {
    pub automatic_eligibility: Label,
    pub trust: Label,
    pub omitted_source_fields: List<Label, 5>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TaskCategory {
    pub count: u32,
    pub status: Label,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub supported_count: Option<u32>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub unselected_count: Option<u32>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub excluded: Option<super::PreparedTaskExcluded>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TaskPolicy {
    pub due_dates: Text<32>,
    pub sessions: Text<32>,
    pub omitted_source_fields: List<Text<32>, 4>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LocalTask {
    pub source_key: Text<64>,
    pub tracker: Text<16>,
    pub external_id: Text<36>,
    pub project_id: Text<36>,
    pub project_alias: Id,
    pub kind: Text<16>,
    pub title: Text<2000>,
    pub state: Text<6>,
    pub origin: Text<16>,
    pub triage_state: Text<8>,
    pub updated_at: Date,
    #[serde(deserialize_with = "nullable")]
    pub snooze_until: Option<Date>,
    pub labels: List<Text<4096>, 0>,
    pub relations: List<Text<0>, 0>,
    #[serde(deserialize_with = "nullable")]
    pub parent_external_id: Option<Id>,
    pub omissions: super::PreparedTaskOmissions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ChatCategory {
    pub count: u32,
    pub status: Label,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub catalog_count: Option<u32>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub unselected_count: Option<u32>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub item_count: Option<u32>,
}

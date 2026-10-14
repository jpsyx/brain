//! SHA-256 over deterministic, secret-free removal inputs.

use super::model::{Identity, IntegrationSnapshot, Options};
use super::snapshot::Snapshot;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct TargetKey {
    pub csv: String,
    pub task_uuid: String,
    pub task_id: String,
}

#[derive(Serialize)]
pub(crate) struct CanonicalDocument<'a> {
    pub schema_version: u32,
    pub workspace_uuid: &'a str,
    pub actor_id: &'a str,
    pub execution_date: &'a str,
    pub options: &'a Options,
    pub targets: BTreeSet<TargetKey>,
    pub tasks: &'a Option<Vec<u8>>,
    pub habits: &'a Option<Vec<u8>>,
    pub projects: BTreeMap<&'a str, &'a [u8]>,
    pub integration: &'a IntegrationSnapshot,
    pub managed_habits_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintenance: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub purge_notes: BTreeMap<&'a str, &'a Option<Vec<u8>>>,
}

impl<'a> CanonicalDocument<'a> {
    pub(crate) fn new(snapshot: &'a Snapshot, options: &'a Options, targets: &[Identity]) -> Self {
        Self {
            schema_version: 1,
            workspace_uuid: &snapshot.workspace_uuid,
            actor_id: &snapshot.actor_id,
            execution_date: &snapshot.execution_date,
            options,
            targets: targets
                .iter()
                .map(|identity| TargetKey {
                    csv: identity.csv.as_str().into(),
                    task_uuid: identity.task_uuid.to_string(),
                    task_id: identity.task_id.clone(),
                })
                .collect(),
            tasks: &snapshot.tasks.before,
            habits: &snapshot.habits.before,
            projects: snapshot
                .projects
                .iter()
                .map(|(path, project)| (path.as_str(), project.before.as_slice()))
                .collect(),
            integration: &snapshot.integration,
            managed_habits_enabled: snapshot.managed_habits_enabled,
            maintenance: None,
            purge_notes: BTreeMap::new(),
        }
    }
}

pub(crate) fn digest<T: serde::Serialize>(value: &T) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(value)?;
    let result = Sha256::digest(bytes);
    let mut hex = String::with_capacity(result.len() * 2);
    for byte in result {
        write!(hex, "{byte:02x}")?;
    }
    Ok(hex)
}

impl CanonicalDocument<'_> {
    pub(super) fn maintenance(
        mut self,
        intent: Option<&super::maintenance::MaintenanceIntent>,
    ) -> Self {
        use super::maintenance::MaintenanceIntent;
        let key = |identity: &Identity| TargetKey {
            csv: identity.csv.as_str().into(),
            task_uuid: identity.task_uuid.to_string(),
            task_id: identity.task_id.clone(),
        };
        self.maintenance = intent.map(|intent| match intent {
            MaintenanceIntent::BacklogPurge { cutoff } => serde_json::json!({"kind": "backlog_purge", "cutoff": cutoff}),
            MaintenanceIntent::BacklogDedupe { replacements } => {
                let pairs: BTreeSet<_> = replacements.iter().map(|pair| (key(&pair.duplicate), key(&pair.survivor))).collect();
                serde_json::json!({"kind": "backlog_dedupe", "replacements": pairs.into_iter().map(|(duplicate, survivor)| serde_json::json!({"duplicate": duplicate, "survivor": survivor})).collect::<Vec<_>>()})
            }
        });
        self
    }
}

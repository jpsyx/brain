//! Final rollout verification across every portable identity boundary.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use super::{MigrationState, discover_state};
use crate::config::Config;
use crate::users::{UserId, Users, UsersStore};
use crate::workspace::{CommandContext, WorkspaceManifest};

pub(super) fn manifest(context: &CommandContext) -> Result<()> {
    match WorkspaceManifest::load(context.workspace.root(), env!("CARGO_PKG_VERSION")) {
        Ok(manifest) if manifest.workspace_id() == context.workspace.id() => Ok(()),
        Ok(manifest) => bail!(
            "portable manifest UUID {} does not match selected workspace UUID {}",
            manifest.workspace_id(),
            context.workspace.id()
        ),
        Err(crate::workspace::ManifestError::Io {
            kind: std::io::ErrorKind::NotFound,
            ..
        }) => {
            WorkspaceManifest::new(context.workspace.id()).write_new(context.workspace.root())?;
            manifest(context)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn completed(
    context: &CommandContext,
    sync_config: &crate::sync::config::SyncConfig,
) -> Result<()> {
    manifest(context)?;
    let registry = crate::workspace::RegistryStore::load_from(context.registry_store.path())?;
    let selected = registry.select(Some(context.workspace.name().as_str()))?;
    if selected.record().workspace_id != context.workspace.id()
        || selected.record().root != context.workspace.root()
    {
        bail!("registry identity changed during workspace migration");
    }
    let users = UsersStore::load(&context.workspace)?;
    let local = UserId::parse(context.workspace.local_user_id())?;
    if users.user(&local).is_none() {
        bail!("local user {local} is not a portable workspace member");
    }
    if discover_state(context.workspace.root())? != MigrationState::Current {
        bail!("task schema verification did not reach the current schema");
    }
    csv_identity_and_assignments(context.workspace.root(), &users)?;
    triage(
        context.workspace.root(),
        &Config::try_load(&context.workspace)?,
    )?;
    derived(context.workspace.root())?;
    if sync_config.is_configured() {
        let remote = crate::sync::remote::build_remote(sync_config);
        crate::sync::identity::require_remote_identity(
            context.workspace.root(),
            context.workspace.id(),
            &remote,
        )?;
    }
    Ok(())
}

fn csv_identity_and_assignments(root: &Path, users: &Users) -> Result<()> {
    let mut uuids = BTreeSet::new();
    for name in ["tasks.csv", "habits.csv"] {
        let path = root.join("tasks").join(name);
        let mut reader = csv::Reader::from_path(&path)?;
        let headers = reader.headers()?.clone();
        let uuid_index = headers
            .iter()
            .position(|header| header == "task_uuid")
            .ok_or_else(|| anyhow!("{} has no task_uuid column", path.display()))?;
        let assignment_index = headers.iter().position(|header| header == "assigned_to");
        for row in reader.records() {
            let row = row?;
            let uuid = row.get(uuid_index).unwrap_or_default();
            uuid::Uuid::parse_str(uuid).context("validate migrated task UUID")?;
            if !uuids.insert(uuid.to_owned()) {
                bail!("duplicate task UUID {uuid} across portable task files");
            }
            if let Some(index) = assignment_index {
                let assignment = row.get(index).unwrap_or_default().trim();
                if !assignment.is_empty()
                    && UserId::parse(assignment)
                        .ok()
                        .is_none_or(|id| users.user(&id).is_none())
                {
                    bail!("task assignment {assignment} is not a portable workspace member");
                }
            }
        }
    }
    Ok(())
}

fn triage(root: &Path, config: &Config) -> Result<()> {
    let path = root.join("tasks/habits.csv");
    let mut reader = csv::Reader::from_path(&path)?;
    let headers = reader.headers()?.clone();
    let index = headers.iter().position(|header| header == "system_key");
    let mut managed = BTreeSet::new();
    if let Some(index) = index {
        for row in reader.records() {
            let value = row?.get(index).unwrap_or_default().to_owned();
            if crate::tasks::triage_habits::is_managed_system_key(&value) {
                managed.insert(value);
            }
        }
    }
    let expected = if config.enable_triage_habits { 2 } else { 0 };
    if managed.len() != expected {
        bail!("managed triage configuration is inconsistent with portable habits");
    }
    Ok(())
}

fn derived(root: &Path) -> Result<()> {
    for (source, index) in [
        ("projects", "projects/projects-lookup.csv"),
        ("resources", "resources/zotero-lookup.csv"),
    ] {
        if root.join(source).is_dir() && !root.join(index).is_file() {
            bail!(
                "derived index {} was not rebuilt",
                root.join(index).display()
            );
        }
    }
    Ok(())
}

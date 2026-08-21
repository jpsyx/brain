//! Safe first-run initialization for a workspace root: creating it, filling it
//! from a configured sync, and seeding PARA when there is nothing to pull.
//!
//! A registry record can name a root this machine has never had — `env.json`
//! rides between a user's machines, so registering a workspace on one of them
//! registers it on all of them. Brain treats that as setup to perform, not an
//! error to report: the whole point of `brain -w family` is to work on the
//! machine you just typed it on.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};

use super::WorkspaceContext;
use crate::sync::args::Direction;

const TASKS_HEADER: &str = "task_uuid,task_id,task_name,task_type,status,waiting_since,priority,due_date,hard_deadline,start_date,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue,system_key\n";
const HABITS_HEADER: &str = "task_uuid,task_id,task_name,status,priority,due_date,hard_deadline,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched,system_key\n";
const PROJECTS_HEADER: &str = "name,namespace,title,status,priority,due,directory\n";
const RESOURCES_HEADER: &str = "zotero_key,title,authors,year,item_type,collection,directory,has_pdf,has_html,has_summary,has_other_notes,annotation_count,tags\n";

const PARA_DIRECTORIES: [&str; 5] = ["projects", "areas", "resources", "archive", "tasks"];
const INFRASTRUCTURE_DIRECTORIES: [&str; 6] = [
    ".brain",
    ".config",
    ".claude",
    ".codex",
    ".opencode",
    ".git",
];

/// What a registered workspace root needs before a command can use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootSetup {
    /// The root is already there.
    Ready,
    /// The root is missing but its parent exists, so Brain creates it.
    Create,
    /// The parent is missing too. An unmounted volume or a mistyped root looks
    /// exactly like this, and silently creating an empty workspace over it
    /// would read as data loss, so Brain reports it instead.
    Unavailable,
}

/// Decide what to do about a registered root. Pure.
#[must_use]
pub(crate) const fn root_setup(root_exists: bool, parent_exists: bool) -> RootSetup {
    if root_exists {
        RootSetup::Ready
    } else if parent_exists {
        RootSetup::Create
    } else {
        RootSetup::Unavailable
    }
}

/// The one-time sync a workspace needs before a command uses it, if any. Pure.
///
/// `ever_synced` is whether **this machine** has ever completed a sync for this
/// workspace, which is the difference between "join an established workspace"
/// and "establish it".
#[must_use]
pub(crate) const fn startup_sync_direction(
    sync_configured: bool,
    ever_synced: bool,
    local_is_empty: bool,
) -> Option<Direction> {
    if !sync_configured {
        return None;
    }
    if !ever_synced {
        // First sync from this machine. It must move data both ways: local
        // content created before sync was configured has never been uploaded,
        // and a pull-only run would leave it stranded here forever.
        return Some(Direction::Both);
    }
    if local_is_empty {
        // An established machine with an emptied root: the remote is the truth.
        return Some(Direction::Pull);
    }
    // Nothing one-time left to do; the ordinary startup pull and the
    // change-triggered push own the steady state.
    None
}

/// Whether an invocation should perform the one-time setup sync and PARA
/// seeding, or only ensure the root directory exists. Pure.
///
/// A command that is itself about syncing owns the network for that run: doing
/// a setup sync first would sync twice, and seeding PARA ahead of the user's own
/// pull would manufacture empty CSVs for it to reconcile. A command that manages
/// the *registry* is not asking to use a workspace at all, and must not write
/// portable config as a side effect of renaming or re-defaulting one.
#[must_use]
pub(crate) const fn performs_setup_sync(invocation: super::Invocation) -> bool {
    !matches!(
        invocation,
        super::Invocation::Sync
            | super::Invocation::SyncStatus
            | super::Invocation::Check
            | super::Invocation::WorkspaceRename
            | super::Invocation::WorkspaceAlias
            | super::Invocation::WorkspaceDefault
            | super::Invocation::WorkspaceList
            | super::Invocation::WorkspaceMigrate
    )
}

/// Make a registered workspace root usable: create it when missing, fill it
/// from the configured sync, and seed PARA when there is nothing to pull.
///
/// Runs before readiness for every ordinary command, so any entry point —
/// `brain`, `brain tasks today`, `brain sync status` — leaves the machine set
/// up rather than reporting what the user should have run first. Idempotent:
/// once the root exists, has content, and has synced at least once, this is a
/// pair of cheap filesystem checks.
pub(crate) fn initialize_workspace_directory(
    workspace: &WorkspaceContext,
    registry_store: &super::RegistryStore,
    may_populate: bool,
) -> Result<()> {
    let root = workspace.root();
    match root_setup(root.is_dir(), root.parent().is_some_and(Path::is_dir)) {
        RootSetup::Ready => {}
        RootSetup::Create => {
            std::fs::create_dir(root)
                .with_context(|| format!("create workspace root {}", root.display()))?;
            eprintln!(
                "{}",
                crate::theme::Theme::active().info(&format!(
                    "Created the workspace root {} for {}",
                    root.display(),
                    workspace.name().as_str()
                ))
            );
        }
        RootSetup::Unavailable => anyhow::bail!(
            "workspace root {} is unavailable and its parent directory does not exist either; \
             restore it (or the volume holding it) or detach the workspace",
            root.display()
        ),
    }

    if !may_populate {
        return Ok(());
    }

    // Sync needs an actor, and a workspace that has never had a local person
    // selected has none yet. That is readiness's problem to report, with its own
    // actionable message, so setup does what it can and leaves sync to the next
    // command rather than failing here with an identity error.
    let command =
        super::CommandContext::new_read_only(Arc::new(workspace.clone()), registry_store.clone())
            .ok();
    let config = command
        .as_ref()
        .map(crate::sync::config::SyncConfig::load)
        .unwrap_or_default();
    let local_is_empty = is_empty_workspace(root)?;
    let direction = startup_sync_direction(
        config.is_configured(),
        has_synced_before(workspace),
        local_is_empty,
    );
    // Portable identity has to exist before the first sync, because the sync's
    // identity gate reads it. Adopt the remote's manifest rather than minting
    // one: `WorkspaceManifest::new` issues a fresh `receiver_ingress_id`, and the
    // manifest is excluded from bisync, so a locally minted one would fork
    // portable identity permanently. Only a remote with no manifest of its own
    // falls back to the registry UUID.
    resolve_portable_identity(workspace, &config)?;

    // The task store must exist before the first sync, not after it. The sync's
    // CSV lane reads `tasks/SCHEMA.json` to decide how to merge, and both CSVs
    // plus the document are excluded from bisync, so no sync can bring them
    // down. Seeding them afterwards left a joining machine syncing as `Legacy`
    // against a `Current` remote: it refused, and `tasks/` stayed empty, so even
    // the migration it suggested had nothing to read.
    // Unconditional: a workspace can be non-empty and still have no task store
    // (a machine that pulled content before this was fixed looks exactly like
    // that), and seeding only empty roots would leave those permanently stuck.
    // Every write is write-only-when-absent, so existing content is untouched.
    seed_task_store(root)?;
    resolve_task_schema_document(workspace, &config)?;

    if let (Some(direction), Some(command)) = (direction, command.as_ref()) {
        eprintln!(
            "{}",
            crate::theme::Theme::active().info(&format!(
                "Setting up {} from its configured sync…",
                workspace.name().as_str()
            ))
        );
        if !crate::command::sync::run_startup_sync(command, direction)? {
            anyhow::bail!(
                "workspace setup stopped because the configured sync did not complete; \
                 run `brain sync -w {}` once the remote is reachable",
                workspace.name().as_str()
            );
        }
    }

    // Whatever the sync did or did not bring down, a root that was empty when
    // Brain found it still needs its PARA skeleton. The decision uses the
    // emptiness captured *before* the task store was seeded above; re-checking
    // here would see Brain's own files and skip the rest of the scaffolding.
    if !local_is_empty {
        return Ok(());
    }
    seed_empty_workspace(workspace)?;
    eprintln!(
        "{}",
        crate::theme::Theme::active().success("Initialized the empty workspace")
    );
    if let Some(command) = command.as_ref()
        && config.is_configured()
        && !crate::command::sync::run_startup_sync(command, Direction::Push)?
    {
        anyhow::bail!(
            "workspace initialization completed locally, but the configured sync push did not complete"
        );
    }
    Ok(())
}

/// Give this machine the workspace's portable manifest before anything reads it.
///
/// A configured remote that already carries one is the authority: adopting it
/// keeps `receiver_ingress_id` identical across machines, which minting cannot
/// do and which bisync will never repair, since the manifest is excluded from
/// it. The registry UUID is the fallback only when no remote manifest exists.
fn resolve_portable_identity(
    workspace: &WorkspaceContext,
    config: &crate::sync::config::SyncConfig,
) -> Result<()> {
    let root = workspace.root();
    let manifest_path = super::WorkspaceManifest::path(root);
    if manifest_path.exists() {
        return Ok(());
    }
    if config.is_configured() {
        let remote = crate::sync::remote::build_remote(config);
        let adoption = crate::sync::identity::adopt_remote_manifest(root, workspace.id(), &remote)?;
        if adoption == crate::sync::identity::ManifestAdoption::Adopted {
            eprintln!(
                "{}",
                crate::theme::Theme::active().success(&format!(
                    "Adopted {}'s portable identity from the remote",
                    workspace.name().as_str()
                ))
            );
            return Ok(());
        }
    }
    std::fs::create_dir_all(root.join(".config"))?;
    super::WorkspaceManifest::new(workspace.id())
        .write_new(root)
        .with_context(|| format!("write the portable manifest at {}", manifest_path.display()))?;
    Ok(())
}

/// Whether this machine has ever completed a sync for this workspace.
///
/// An unreadable or absent journal reads as "never", which is the safe answer:
/// it schedules an establishing run rather than assuming the remote already has
/// this machine's content.
fn has_synced_before(workspace: &WorkspaceContext) -> bool {
    crate::sync::journal::Journal::open(&workspace.paths().sync_journal())
        .ok()
        .and_then(|journal| journal.latest_id().ok().flatten())
        .is_some()
}

mod seed;

#[cfg(test)]
use seed::{contains_file, is_empty_workspace_inner};
pub(crate) use seed::{initialize_if_empty, is_empty_workspace};
use seed::{resolve_task_schema_document, seed_empty_workspace, seed_task_store};

#[cfg(test)]
mod tests;

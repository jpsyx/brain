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
const INFRASTRUCTURE_DIRECTORIES: [&str; 5] = [".config", ".claude", ".codex", ".opencode", ".git"];

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

    // The portable manifest is pure identity Brain already knows: the record
    // carries the workspace UUID, so a machine joining an unsynced workspace
    // does not need to be sent to `brain workspace repair --manifest` for it.
    // Written only when absent, so a manifest that just arrived over sync — the
    // authoritative one — is never replaced.
    let manifest_path = super::WorkspaceManifest::path(root);
    if !manifest_path.exists() {
        std::fs::create_dir_all(root.join(".config"))?;
        super::WorkspaceManifest::new(workspace.id())
            .write_new(root)
            .with_context(|| {
                format!("write the portable manifest at {}", manifest_path.display())
            })?;
    }

    // Every schema decision reads `tasks/SCHEMA.json`, so a workspace without
    // one cannot sync at all. Seeded here rather than only in
    // `initialize_if_empty` because a workspace created before Brain shipped the
    // document is no longer empty and would never get it. Write-only-when-absent
    // like the manifest above: a document that arrived over sync is the
    // authoritative one.
    crate::tasks::schema::ensure_schema_document(root)?;

    // Whatever the sync did or did not bring down, an empty root still needs
    // its PARA skeleton. A workspace that just pulled content is no longer
    // empty, so this is a no-op there.
    if !initialize_if_empty(workspace)? {
        return Ok(());
    }
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

/// Initialize the selected workspace when it contains only Brain's own setup
/// directories and no user content. Existing files are never overwritten.
pub(crate) fn initialize_if_empty(workspace: &WorkspaceContext) -> Result<bool> {
    if !is_empty_workspace(workspace.root())? {
        return Ok(false);
    }

    for directory in PARA_DIRECTORIES {
        std::fs::create_dir_all(workspace.root().join(directory))?;
    }
    write_if_missing(&workspace.root().join(".config/config.json"), b"{}\n")?;
    write_if_missing(
        &workspace.root().join("tasks/tasks.csv"),
        TASKS_HEADER.as_bytes(),
    )?;
    write_if_missing(
        &workspace.root().join("tasks/habits.csv"),
        HABITS_HEADER.as_bytes(),
    )?;
    write_if_missing(
        &workspace.root().join("projects/projects-lookup.csv"),
        PROJECTS_HEADER.as_bytes(),
    )?;
    write_if_missing(
        &workspace.root().join("resources/zotero-lookup.csv"),
        RESOURCES_HEADER.as_bytes(),
    )?;
    write_if_missing(&workspace.root().join("tasks/.tasks_next_id"), b"1\n")?;
    write_if_missing(&workspace.root().join("tasks/.habits_next_id"), b"1\n")?;
    crate::tasks::schema::ensure_schema_document(workspace.root())?;
    // Honor an existing answer instead of forcing the default on: a workspace
    // can carry portable config (a synced or freshly configured one does)
    // before it has any content, and seeding must not overwrite a deliberate
    // `enable_triage_habits: false`. A workspace with no config at all still
    // gets the default, which is on.
    let enable_triage_habits = crate::config::Config::load(workspace).enable_triage_habits;
    crate::tasks::triage_habits::apply_triage_habits_config(workspace, enable_triage_habits)?;
    Ok(true)
}

pub(crate) fn is_empty_workspace(root: &Path) -> Result<bool> {
    is_empty_workspace_inner(root)
}

fn is_empty_workspace_inner(root: &Path) -> Result<bool> {
    let entries = std::fs::read_dir(root)
        .with_context(|| format!("inspect workspace root {}", root.display()))?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if INFRASTRUCTURE_DIRECTORIES.contains(&name.as_ref()) || name == ".DS_Store" {
            continue;
        }
        if PARA_DIRECTORIES.contains(&name.as_ref()) {
            if contains_file(entry.path())? {
                return Ok(false);
            }
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

fn contains_file(path: std::path::PathBuf) -> Result<bool> {
    if path.is_file() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Ok(false);
    }
    for entry in std::fs::read_dir(path)? {
        if contains_file(entry?.path())? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_if_missing(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    std::io::Write::write_all(&mut file, bytes)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        RootSetup, contains_file, is_empty_workspace_inner, performs_setup_sync, root_setup,
        startup_sync_direction,
    };
    use crate::sync::args::Direction;

    #[test]
    fn an_existing_root_needs_nothing() {
        assert_eq!(root_setup(true, true), RootSetup::Ready);
        assert_eq!(root_setup(true, false), RootSetup::Ready);
    }

    #[test]
    fn a_registered_root_whose_parent_exists_is_created() {
        // The common case: `env.json` synced from another machine names
        // `~/family`, and this machine simply does not have it yet.
        assert_eq!(root_setup(false, true), RootSetup::Create);
    }

    #[test]
    fn a_root_under_a_missing_parent_is_reported_unavailable() {
        // An unmounted volume must not be silently replaced by an empty
        // workspace; that would look like the data was lost.
        assert_eq!(root_setup(false, false), RootSetup::Unavailable);
    }

    #[test]
    fn a_sync_command_owns_its_own_network_run() {
        // Otherwise `brain sync` would sync twice, and seeding PARA ahead of the
        // user's own pull would manufacture empty CSVs to reconcile.
        assert!(!performs_setup_sync(crate::workspace::Invocation::Sync));
        assert!(!performs_setup_sync(
            crate::workspace::Invocation::SyncStatus
        ));
        assert!(!performs_setup_sync(crate::workspace::Invocation::Check));
    }

    #[test]
    fn registry_management_never_writes_portable_config_as_a_side_effect() {
        // Renaming or re-defaulting a workspace is not a request to use it.
        for invocation in [
            crate::workspace::Invocation::WorkspaceRename,
            crate::workspace::Invocation::WorkspaceAlias,
            crate::workspace::Invocation::WorkspaceDefault,
            crate::workspace::Invocation::WorkspaceList,
            crate::workspace::Invocation::WorkspaceMigrate,
        ] {
            assert!(!performs_setup_sync(invocation), "{invocation:?}");
        }
    }

    #[test]
    fn every_other_command_sets_the_workspace_up_first() {
        for invocation in [
            crate::workspace::Invocation::Tui,
            crate::workspace::Invocation::Tasks,
            crate::workspace::Invocation::Config,
            crate::workspace::Invocation::Env,
            crate::workspace::Invocation::Habits,
        ] {
            assert!(performs_setup_sync(invocation), "{invocation:?}");
        }
    }

    #[test]
    fn without_sync_the_first_run_never_reaches_the_network() {
        assert_eq!(startup_sync_direction(false, false, true), None);
        assert_eq!(startup_sync_direction(false, false, false), None);
    }

    #[test]
    fn the_first_sync_from_a_machine_establishes_both_directions() {
        // Local content that predates sync setup has never been uploaded, and
        // a pull-only startup would never upload it. The establishing run has
        // to move data both ways.
        assert_eq!(
            startup_sync_direction(true, false, false),
            Some(Direction::Both)
        );
        assert_eq!(
            startup_sync_direction(true, false, true),
            Some(Direction::Both)
        );
    }

    #[test]
    fn an_empty_root_on_a_synced_machine_only_pulls() {
        // Nothing local to contribute: the remote is the source of truth.
        assert_eq!(
            startup_sync_direction(true, true, true),
            Some(Direction::Pull)
        );
    }

    #[test]
    fn an_established_populated_workspace_adds_no_extra_startup_sync() {
        // The ordinary startup pull and the change-triggered push already own
        // this case; syncing again here would sync twice on every command.
        assert_eq!(startup_sync_direction(true, true, false), None);
    }

    #[test]
    fn setup_only_directories_are_empty() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".config")).unwrap();
        std::fs::create_dir(root.path().join(".claude")).unwrap();
        std::fs::create_dir(root.path().join("tasks")).unwrap();
        assert!(is_empty_workspace_inner(root.path()).unwrap());
    }

    #[test]
    fn a_user_file_prevents_initialization() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("areas")).unwrap();
        std::fs::write(root.path().join("areas/family.md"), "family").unwrap();
        assert!(!is_empty_workspace_inner(root.path()).unwrap());
    }

    #[test]
    fn nested_empty_para_directories_have_no_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("projects/empty")).unwrap();
        assert!(!contains_file(root.path().join("projects")).unwrap());
    }
}

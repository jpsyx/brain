//! Sync command handler.

use anyhow::Result;

/// Result of crossing one workspace's sync-lock command boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceLockOutcome<T> {
    Ran(T),
    Coalesced,
    Followed,
}

/// Run under one UUID-scoped sync lock or apply the existing busy policy.
#[must_use]
pub fn run_with_workspace_lock<T>(
    paths: &crate::workspace::WorkspacePaths,
    if_idle: bool,
    run: impl FnOnce() -> T,
    follow: impl FnOnce(),
) -> WorkspaceLockOutcome<T> {
    let Some(_guard) = crate::sync::lock::try_acquire(&paths.sync_lock()) else {
        if if_idle {
            return WorkspaceLockOutcome::Coalesced;
        }
        follow();
        return WorkspaceLockOutcome::Followed;
    };
    WorkspaceLockOutcome::Ran(run())
}

pub fn run(args: &crate::cli::SyncArgs, command: &crate::workspace::CommandContext) -> Result<()> {
    use crate::cli::SyncAction;
    let root = command.workspace.root();
    // Sync dispatches before the workspace gate, so it never passes through root
    // initialization. `sync setup` cannot run at all without the task schema
    // document, so seed it here too rather than leaving every sync subcommand
    // dependent on some other command having run first.
    crate::tasks::schema::ensure_schema_document(root)?;
    let cfg = crate::sync::config::SyncConfig::load(command);
    match &args.action {
        Some(SyncAction::Setup { adopt_workspace_id }) => {
            crate::logging::log("sync setup");
            crate::sync::setup::run(command, adopt_workspace_id.as_deref())
        }
        Some(SyncAction::Repair) => {
            crate::logging::log("sync repair");
            run_repair(command, &cfg, args.if_idle)
        }
        Some(SyncAction::Init) => {
            let theme = crate::theme::Theme::active();
            eprintln!(
                "{}",
                theme.warning(
                    "`brain sync init` was renamed to `brain sync repair`; running repair now."
                )
            );
            crate::logging::log("sync init alias -> repair");
            run_repair(command, &cfg, args.if_idle)
        }
        Some(SyncAction::Status) => print_status(command, &cfg),
        Some(SyncAction::Conflicts { json }) => {
            crate::logging::log(format!("sync conflicts json={json}"));
            crate::sync::command::print_conflicts(root, *json)
        }
        Some(SyncAction::Resolve { originals }) => {
            crate::logging::log(format!("sync resolve originals={originals:?}"));
            crate::sync::command::resolve(root, originals)
        }
        None => {
            let direction = crate::sync::command::direction_from_flags(args.push, args.pull)?;
            crate::logging::log(format!(
                "sync run direction={} if_idle={}",
                crate::sync::command::direction_label(direction),
                args.if_idle
            ));
            run_once(command, &cfg, direction, args.if_idle).map(|_| ())
        }
    }
}

pub(crate) fn run_startup_sync(
    command: &crate::workspace::CommandContext,
    direction: crate::sync::args::Direction,
) -> Result<bool> {
    let config = crate::sync::config::SyncConfig::load(command);
    run_once(command, &config, direction, false)
}

fn print_status(
    command: &crate::workspace::CommandContext,
    config: &crate::sync::config::SyncConfig,
) -> Result<()> {
    crate::logging::log("sync status");
    let requirements = crate::workspace::requirements(command)?;
    let sync = requirements
        .iter()
        .filter(|requirement| {
            matches!(
                requirement.scope(),
                crate::workspace::RequirementScope::CloudSync
                    | crate::workspace::RequirementScope::SyncWatcher
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    print!(
        "{}",
        crate::workspace::format_requirements(
            command.workspace.name(),
            &sync,
            crate::theme::Theme::active(),
        )
    );
    let status = sync
        .iter()
        .find(|requirement| requirement.scope() == &crate::workspace::RequirementScope::CloudSync)
        .map(crate::workspace::Requirement::status);
    if matches!(
        status,
        Some(crate::workspace::RequirementStatus::Feature(
            crate::workspace::FeatureStatus::Incomplete
        ))
    ) {
        return Ok(());
    }
    crate::sync::command::print_status(command.workspace.paths(), config, command.workspace.root())
}

fn run_repair(
    command: &crate::workspace::CommandContext,
    config: &crate::sync::config::SyncConfig,
    if_idle: bool,
) -> Result<()> {
    let succeeded = run_once(
        command,
        config,
        crate::sync::args::Direction::Resync,
        if_idle,
    )?;
    reconcile_after_successful_repair(&command.workspace, succeeded)
}

fn reconcile_after_successful_repair(
    workspace: &crate::workspace::WorkspaceContext,
    succeeded: bool,
) -> Result<()> {
    if succeeded {
        let enabled = crate::config::Config::load(workspace).enable_triage_habits;
        crate::tasks::triage_habits::apply_triage_habits_config(workspace, enabled)?;
    }
    Ok(())
}

fn run_once(
    command: &crate::workspace::CommandContext,
    config: &crate::sync::config::SyncConfig,
    direction: crate::sync::args::Direction,
    if_idle: bool,
) -> Result<bool> {
    if !config.is_configured() {
        crate::logging::log("sync not configured");
        println!(
            "{}",
            crate::sync::command::format_unconfigured_sync_guidance(
                direction,
                crate::theme::Theme::active(),
            )
        );
        return Ok(false);
    }
    let theme = crate::theme::Theme::active();
    eprintln!("{}", format_lock_progress(theme));
    crate::logging::log(format!(
        "sync acquire lock {}",
        command.workspace.paths().sync_lock().display()
    ));
    let paths = command.workspace.paths();
    let lock_outcome = run_with_workspace_lock(
        paths,
        if_idle,
        || run_acquired(command, config, direction),
        || {
            crate::logging::log("sync lock busy; following in-flight sync");
            crate::sync::follow::follow_until_done(paths);
        },
    );
    match lock_outcome {
        WorkspaceLockOutcome::Ran(result) => result,
        WorkspaceLockOutcome::Coalesced => {
            crate::logging::log("sync lock busy; if-idle coalesce");
            Ok(false)
        }
        WorkspaceLockOutcome::Followed => Ok(false),
    }
}

fn run_acquired(
    command: &crate::workspace::CommandContext,
    config: &crate::sync::config::SyncConfig,
    direction: crate::sync::args::Direction,
) -> Result<bool> {
    crate::migration::require_no_active_rollout(command.workspace.paths())?;
    let root = command.workspace.root();
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    crate::logging::log(format!("sync start ts={timestamp}"));
    let outcome = crate::sync::command::sync_once(
        command.workspace.paths(),
        command.workspace.id(),
        config,
        root,
        direction,
        (&timestamp, &timestamp, &date),
    )?;
    crate::logging::log(format!("sync outcome={}", outcome.label()));
    let succeeded = matches!(outcome, crate::sync::verify::Outcome::Clean);
    match outcome {
        crate::sync::verify::Outcome::Clean => println!("sync complete."),
        crate::sync::verify::Outcome::NeedsAttention(message)
        | crate::sync::verify::Outcome::Aborted(message) => eprintln!("{message}"),
    }
    Ok(succeeded)
}

#[must_use]
fn format_lock_progress(theme: crate::theme::Theme) -> String {
    theme.info("Acquiring the workspace sync lock…")
}

#[cfg(test)]
mod tests {
    use super::{format_lock_progress, reconcile_after_successful_repair};

    #[test]
    fn successful_repair_restores_missing_managed_triage_definitions() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("family");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::write(
            root.join("tasks/tasks.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tasks/habits.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(root.join(".config/config.json"), "{}\n").unwrap();
        let workspace = crate::workspace::WorkspaceContext::new(
            temporary.path(),
            crate::workspace::WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
            crate::workspace::WorkspaceName::parse("family").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();

        reconcile_after_successful_repair(&workspace, true).unwrap();

        let habits = crate::tasks::task::load_habits(&root.join("tasks/habits.csv")).unwrap();
        assert_eq!(
            habits
                .iter()
                .filter(|habit| habit.is_managed_triage())
                .count(),
            2
        );
    }

    #[test]
    fn lock_progress_names_workspace_scoped_coordination() {
        let line = format_lock_progress(crate::theme::Theme::dark(false));

        assert!(line.contains("workspace sync lock"), "{line}");
    }
}

//! Thin rollout coordinator over the focused durable primitives.

use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;

use super::users::{PreparedUsers, Terminal, prepare as prepare_users};
use super::verify;
use super::{
    JournalRequest, MigrationGate, MigrationGateInput, MigrationJournal, MigrationState, PlanInput,
    Step, backup_directory, backup_portable_data, discover_state, migration_gate, migration_plan,
};
use crate::config::Config;
use crate::tasks::schema::{LegacySemanticSync, TaskSchemaMigration, migrate_inactive};
use crate::workspace::CommandContext;

pub(crate) fn run(
    context: &CommandContext,
    explicit_workspace: bool,
    acknowledged_all_machines_updated: bool,
) -> Result<()> {
    let interactive = std::io::stdin().is_terminal();
    let sync_config = crate::sync::config::SyncConfig::load(context);
    let sync_configured = sync_config.is_configured();
    let journal_path = context.workspace.paths().migration_journal();
    let state = discover_state(context.workspace.root())?;
    if matches!(state, MigrationState::Current) && !journal_path.exists() {
        eprintln!(
            "{}",
            crate::theme::Theme::active().success("Workspace migration is already complete.")
        );
        return Ok(());
    }
    if let MigrationState::NewerRefused { .. } = state {
        migration_plan(PlanInput {
            state,
            sync_configured,
        })?;
    }
    let plan_state = if journal_path.exists() {
        MigrationState::Prepared
    } else {
        state
    };
    let plan = migration_plan(PlanInput {
        state: plan_state,
        sync_configured,
    })?;

    let mut terminal = interactive.then(Terminal::open).transpose()?;
    match migration_gate(MigrationGateInput {
        sync_configured,
        interactive,
        explicit_workspace,
        acknowledged_all_machines_updated,
    })? {
        MigrationGate::Proceed => {}
        MigrationGate::ConfirmAllMachinesUpdated => {
            terminal
                .as_mut()
                .expect("interactive migration has a terminal")
                .confirm_all_machines_updated()?;
        }
    }

    let config = Config::try_load(&context.workspace)?;
    let prepared_users = prepare_users(context, &config, terminal.as_mut())?;

    if sync_configured {
        let remote = crate::sync::remote::build_remote(&sync_config);
        crate::sync::identity::require_remote_identity(
            context.workspace.root(),
            context.workspace.id(),
            &remote,
        )
        .context("migration remote identity preflight")?;
    }

    let now = Utc::now();
    let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let started_at = now.to_rfc3339();
    let backup_base = context.workspace.paths().migration_backups();
    let backup = backup_directory(&backup_base, &timestamp)?;
    let mut journal = if journal_path.exists() {
        MigrationJournal::resume(
            &journal_path,
            context.workspace.id(),
            context.workspace.root(),
            &plan,
        )?
    } else {
        MigrationJournal::open_or_create(JournalRequest {
            path: &journal_path,
            workspace_id: context.workspace.id(),
            workspace_root: context.workspace.root(),
            backup_dir: &backup,
            started_at: &started_at,
            plan: &plan,
        })?
    };
    let retained_backup = journal.backup_dir().to_path_buf();
    if let Err(error) = execute(
        context,
        &sync_config,
        &config,
        &prepared_users,
        &backup_base,
        &mut journal,
    ) {
        let backup_complete = journal.completed(Step::BackupPortableData);
        return Err(with_recovery(
            &error,
            context,
            &retained_backup,
            sync_configured,
            backup_complete,
        ));
    }
    eprintln!(
        "{}",
        crate::theme::Theme::active().success(&format!(
            "Workspace migration complete. Backup retained at {}",
            retained_backup.display()
        ))
    );
    Ok(())
}

fn execute(
    context: &CommandContext,
    sync_config: &crate::sync::config::SyncConfig,
    config: &Config,
    prepared_users: &PreparedUsers,
    backup_base: &Path,
    journal: &mut MigrationJournal,
) -> Result<()> {
    let steps = journal.remaining_steps().to_vec();
    for step in steps {
        let theme = crate::theme::Theme::active();
        eprintln!("{}", theme.info(step_message(step)));
        match step {
            Step::LegacySemanticSync => {
                crate::sync::command::run_legacy_migration_sync(context, sync_config)?;
                journal.record_completed(step)?;
            }
            Step::BackupPortableData => {
                backup_portable_data(context.workspace.root(), backup_base, journal.backup_dir())?;
                journal.record_completed(step)?;
            }
            Step::EnsureWorkspaceManifest => {
                verify::manifest(context)?;
                journal.record_completed(step)?;
            }
            Step::EnsureUsersRegistry => {
                prepared_users.persist(context)?;
                journal.record_completed(step)?;
            }
            Step::MigrateTaskColumnsAndUuids => {
                migrate_inactive(TaskSchemaMigration {
                    workspace_id: context.workspace.id(),
                    workspace_root: context.workspace.root(),
                    task_store_lock: &context.workspace.paths().task_store_lock(),
                    preexisting_backup_base: backup_base,
                    backup_dir: journal.backup_dir(),
                    legacy_semantic_sync: if sync_config.is_configured() {
                        LegacySemanticSync::Complete
                    } else {
                        LegacySemanticSync::NotConfigured
                    },
                })?;
                journal.record_completed(step)?;
            }
            Step::ReconcileManagedTriage => {
                crate::tasks::triage_habits::apply_triage_habits_config(
                    &context.workspace,
                    config.enable_triage_habits,
                )?;
                journal.record_completed(step)?;
            }
            Step::RebuildDerivedData => {
                crate::reindex::run(&context.workspace, &context.actor, true, true, true)?;
                journal.record_completed(step)?;
            }
            Step::Verify => {
                verify::completed(context, sync_config)?;
                journal.record_completed(step)?;
            }
            Step::MarkComplete => journal.mark_complete()?,
        }
    }
    Ok(())
}

fn step_message(step: Step) -> &'static str {
    match step {
        Step::LegacySemanticSync => "Running the final legacy semantic sync...",
        Step::BackupPortableData => "Creating the durable portable-data backup...",
        Step::EnsureWorkspaceManifest => "Verifying the portable workspace identity...",
        Step::EnsureUsersRegistry => "Writing the portable user registry...",
        Step::MigrateTaskColumnsAndUuids => "Migrating task columns and UUID identity...",
        Step::ReconcileManagedTriage => "Reconciling managed triage habits...",
        Step::RebuildDerivedData => "Rebuilding derived indexes...",
        Step::Verify => "Verifying the completed workspace migration...",
        Step::MarkComplete => "Removing the completed migration journal...",
    }
}

fn with_recovery(
    error: &anyhow::Error,
    context: &CommandContext,
    backup: &Path,
    sync_configured: bool,
    backup_complete: bool,
) -> anyhow::Error {
    let instructions = recovery_instructions(
        context.workspace.name().as_str(),
        context.workspace.root(),
        backup,
        sync_configured,
        backup_complete,
        Path::exists,
    );
    anyhow!("workspace migration failed: {error:#}\n\n{instructions}")
}

fn recovery_instructions(
    workspace: &str,
    root: &Path,
    backup: &Path,
    sync_configured: bool,
    backup_complete: bool,
    backup_exists: impl Fn(&Path) -> bool,
) -> String {
    let acknowledgement = if sync_configured {
        " --acknowledge-all-machines-updated"
    } else {
        ""
    };
    let recovery = if backup_complete {
        let mut commands = vec![format!(
            "cp -pR {}/. {}/",
            crate::session::shell_quote(backup.to_string_lossy().as_ref()),
            crate::session::shell_quote(root.to_string_lossy().as_ref())
        )];
        for relative in [
            ".config/config.json",
            ".config/users.json",
            ".config/workspace.json",
            "tasks/.tasks_next_id",
            "tasks/.habits_next_id",
        ] {
            if !backup_exists(&backup.join(relative)) {
                commands.push(format!(
                    "rm -f {}",
                    crate::session::shell_quote(root.join(relative).to_string_lossy().as_ref())
                ));
            }
        }
        commands.push(format!(
            "brain reindex -b {}",
            crate::session::shell_quote(workspace)
        ));
        format!(
            "Restore only to abandon the migration:\n{}",
            commands.join("\n")
        )
    } else {
        "The portable backup step did not complete, so no later rollout replacement ran. Fix the reported error and resume.".to_owned()
    };
    format!(
        "Backup path: {}\nResume: brain workspace migrate -b {}{acknowledgement}\n{recovery}",
        backup.display(),
        crate::session::shell_quote(workspace)
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn recovery_instructions_quote_paths_and_restore_the_original_optional_inventory() {
        let backup = Path::new("/tmp/family's brain/backups/20260806T120000Z");
        let root = Path::new("/tmp/family's brain");

        let instructions =
            super::recovery_instructions("family space", root, backup, true, true, |_| false);

        assert!(instructions.contains(
            "Resume: brain workspace migrate -b 'family space' --acknowledge-all-machines-updated"
        ));
        assert!(instructions.contains(
            "cp -pR '/tmp/family'\\''s brain/backups/20260806T120000Z'/. '/tmp/family'\\''s brain'/"
        ));
        for relative in [
            ".config/config.json",
            ".config/users.json",
            ".config/workspace.json",
            "tasks/.tasks_next_id",
            "tasks/.habits_next_id",
        ] {
            assert!(
                instructions.contains(&format!("rm -f '/tmp/family'\\''s brain/{relative}'")),
                "missing absence restoration for {relative}: {instructions}"
            );
        }
        assert!(instructions.contains("brain reindex -b 'family space'"));
    }
}

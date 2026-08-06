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
    super::with_activation_lock(context.workspace.paths(), || {
        run_locked(
            context,
            explicit_workspace,
            acknowledged_all_machines_updated,
        )
    })
}

fn run_locked(
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

    if sync_configured {
        let remote = crate::sync::remote::build_remote(&sync_config);
        crate::sync::identity::require_remote_identity(
            context.workspace.root(),
            context.workspace.id(),
            &remote,
        )
        .context("migration remote identity preflight")?;
    }

    let prepared = if sync_configured {
        None
    } else {
        let config = Config::try_load(&context.workspace)?;
        let users = prepare_users(context, &config, terminal.as_mut())?;
        Some((config, users))
    };

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
        terminal.as_mut(),
        prepared,
        &backup_base,
        &mut journal,
    ) {
        let backup_complete = journal.completed(Step::BackupPortableData);
        let remote_transition_complete = journal.completed(Step::PublishTaskSchemaTransition);
        return Err(with_recovery(
            &error,
            context,
            &retained_backup,
            sync_configured,
            backup_complete,
            remote_transition_complete,
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
    terminal: Option<&mut Terminal>,
    prepared: Option<(Config, PreparedUsers)>,
    backup_base: &Path,
    journal: &mut MigrationJournal,
) -> Result<()> {
    if journal.remaining_steps().first() == Some(&Step::LegacySemanticSync) {
        let theme = crate::theme::Theme::active();
        eprintln!("{}", theme.info(step_message(Step::LegacySemanticSync)));
        crate::sync::command::run_legacy_migration_sync(context, sync_config)?;
        journal.record_completed(Step::LegacySemanticSync)?;
    }

    let (config, prepared_users) = if let Some(prepared) = prepared {
        prepared
    } else {
        let config = Config::try_load(&context.workspace)?;
        let users = prepare_users(context, &config, terminal)?;
        (config, users)
    };
    let steps = journal.remaining_steps().to_vec();
    for step in steps {
        let theme = crate::theme::Theme::active();
        eprintln!("{}", theme.info(step_message(step)));
        match step {
            Step::LegacySemanticSync => unreachable!("legacy sync is handled before preflight"),
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
                    assignment_rewrites: prepared_users.assignment_rewrites(),
                })?;
                journal.record_completed(step)?;
            }
            Step::PublishTaskSchemaTransition => {
                super::schema_transition::publish_task_schema_transition(context, sync_config)?;
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
                let repaired = crate::tasks::schema::repair_current_duplicate_uuids(
                    context.workspace.root(),
                    context.workspace.id(),
                )?;
                if repaired {
                    eprintln!(
                        "{}",
                        crate::theme::Theme::active()
                            .info("Repairing duplicate task UUIDs from an earlier writer...")
                    );
                    crate::reindex::run(&context.workspace, &context.actor, true, true, true)?;
                }
                if sync_config.is_configured() {
                    super::schema_transition::publish_task_schema_transition(context, sync_config)?;
                }
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
        Step::PublishTaskSchemaTransition => {
            "Publishing task CSVs, baselines, and schema metadata..."
        }
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
    remote_transition_complete: bool,
) -> anyhow::Error {
    let instructions = recovery_instructions(
        context.workspace.name().as_str(),
        backup,
        sync_configured,
        backup_complete,
        remote_transition_complete,
    );
    anyhow!("workspace migration failed: {error:#}\n\n{instructions}")
}

fn recovery_instructions(
    workspace: &str,
    backup: &Path,
    sync_configured: bool,
    backup_complete: bool,
    remote_transition_complete: bool,
) -> String {
    let acknowledgement = if sync_configured {
        " --acknowledge-all-machines-updated"
    } else {
        ""
    };
    let recovery = if remote_transition_complete {
        "The remote task schema transition completed and is durably recorded. Keep the immutable backup for forensic or coordinated manual recovery; resume the migration so local and remote state can be reconciled and verified. Never restore only this machine while the rollout journal exists."
    } else if backup_complete {
        "The immutable backup is retained for forensic or coordinated manual recovery. A remote publication may have completed before its journal record; resume the migration to reconcile the authoritative state. Never restore only this machine while the rollout journal exists."
    } else {
        "The backup path may be incomplete and is retained for forensic inspection. Fix the reported error and resume the journaled migration; never restore only this machine while the rollout journal exists."
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
    fn every_journaled_failure_is_resume_only_even_when_remote_publication_is_ambiguous() {
        let backup = Path::new("/tmp/family's brain/backups/20260806T120000Z");
        for (backup_complete, transition_recorded) in [(false, false), (true, false), (true, true)]
        {
            let instructions = super::recovery_instructions(
                "family space",
                backup,
                true,
                backup_complete,
                transition_recorded,
            );

            assert!(instructions.contains(
                "Resume: brain workspace migrate -b 'family space' --acknowledge-all-machines-updated"
            ));
            assert!(instructions.contains("forensic"), "{instructions}");
            assert!(!instructions.contains("cp -pR"), "{instructions}");
            assert!(!instructions.contains("rm -f"), "{instructions}");
            assert!(!instructions.contains("Restore only"), "{instructions}");
        }
    }

    #[test]
    fn post_transition_recovery_requires_resume_and_never_restores_only_local_files() {
        let backup = Path::new("/tmp/family-brain/backups/20260806T120000Z");
        let instructions = super::recovery_instructions("family", backup, true, true, true);

        assert!(instructions.contains("remote task schema transition completed"));
        assert!(instructions.contains(
            "Resume: brain workspace migrate -b 'family' --acknowledge-all-machines-updated"
        ));
        assert!(!instructions.contains("cp -pR"));
        assert!(!instructions.contains("rm -f"));
        assert!(!instructions.contains("Restore only"));
    }
}

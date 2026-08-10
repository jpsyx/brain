//! `brain sync` orchestration.
//!
//! Ties the pure builders to the rclone shell, the conflict post-pass,
//! verification, and the journal. Kept thin; the tested logic lives in the
//! builders it calls. The `resolve` submodule (`brain sync resolve`) is
//! self-contained enough to live in its own file; its one externally-called
//! entry point is re-exported here so `crate::sync::command::resolve` (called
//! from `main.rs`) keeps resolving unchanged. `ResolveDecision`/
//! `resolve_decision` have no call sites outside `resolve.rs` itself, so they
//! stay at their natural `resolve::` path rather than being re-exported
//! (which `cargo clippy` would flag as unused in this binary crate).

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};

use crate::sync::args::{self, Direction};
use crate::sync::config::SyncConfig;
use crate::sync::conflicts::{self, ConflictGroup};
use crate::sync::csv_sync::{CsvMergeOutcome, CsvSyncError, CsvSyncResult};
use crate::sync::journal::{Journal, SyncRun};
use crate::sync::remote::{Remote, build_remote};
use crate::sync::run::run_rclone;
use crate::sync::verify::{self, Outcome};
use crate::theme::Theme;

mod resolve;
pub use resolve::resolve;

fn sync_task_state(
    csv_sync: impl FnOnce() -> Result<CsvSyncResult, CsvSyncError>,
    counters: impl FnOnce(crate::sync::csv_sync::DisplayIdFloors),
) -> Result<CsvSyncResult, CsvSyncError> {
    let csv = csv_sync()?;
    counters(csv.floors);
    Ok(csv)
}

/// This machine's short hostname for conflict-copy names. Falls back to "host".
#[must_use]
pub fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        })
        .map(|s| s.trim().split('.').next().unwrap_or("host").to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "host".to_owned())
}

/// Run one sync in `dir`. `now` supplies the timestamps + date (injected so the
/// call is testable and to keep clock reads out of pure code). Returns the
/// verified outcome.
pub fn sync_once(
    paths: &crate::workspace::WorkspacePaths,
    workspace_id: crate::workspace::WorkspaceId,
    cfg: &SyncConfig,
    root: &Path,
    dir: Direction,
    now: (&str, &str, &str),
) -> Result<Outcome> {
    sync_once_with_task_state(paths, workspace_id, cfg, root, dir, now, true)
}

fn sync_once_with_task_state(
    paths: &crate::workspace::WorkspacePaths,
    workspace_id: crate::workspace::WorkspaceId,
    cfg: &SyncConfig,
    root: &Path,
    dir: Direction,
    now: (&str, &str, &str),
    reconcile_task_state: bool,
) -> Result<Outcome> {
    if !cfg.is_configured() {
        bail!(
            "sync is not configured — run `{}`",
            crate::workspace::suggest("sync setup")
        );
    }
    let (started_at, finished_at, date) = now;
    let remote = build_remote(cfg);
    let local = root.to_string_lossy().into_owned();
    let theme = Theme::active();
    // The single output sink for this run: everything below is mirrored to
    // `current.log` (so a following `brain sync` and `brain sync status` can
    // observe a detached background sync) and echoed to this process's stderr.
    let reporter = crate::sync::current::Reporter::begin(
        paths,
        direction_label(dir),
        started_at,
        std::process::id(),
    );
    crate::logging::log(format!(
        "sync_once direction={} root={} remote={}",
        direction_label(dir),
        root.display(),
        remote.arg
    ));
    reporter.line(&format_sync_plan(cfg, root, dir, theme));

    if !crate::sync::run::rclone_present() {
        return Ok(Outcome::Aborted(crate::sync::run::missing_rclone_guidance(
            theme,
            "brain sync",
        )));
    }

    reporter.line(&theme.info(sync_progress(dir)));

    reporter.line(&theme.info("Validating the local workspace manifest…"));
    reporter.line(&theme.info("Probing the remote workspace identity…"));
    let verified = crate::sync::identity::require_remote_identity(root, workspace_id, &remote)?;
    let remote = verified.remote();
    reporter.line(&theme.muted(&format!(
        "  found: remote belongs to this workspace ({workspace_id}) → proceeding"
    )));
    let workdir = crate::sync::run::bisync_workdir(paths);
    let _ = std::fs::create_dir_all(&workdir);
    let workdir_arg = workdir.to_string_lossy().into_owned();
    let argv = if dir == Direction::Push {
        args::push_args(cfg, &local, &remote.arg)
    } else {
        args::bisync_args(cfg, &local, &remote.arg, &workdir_arg, dir)
    };

    if should_bootstrap_check_access(dir) {
        crate::logging::log("sync check-access markers");
        reporter.line(&theme.info("Checking the sync safety marker…"));
        crate::sync::check_access::ensure_markers(root, &verified)?;
    }

    // We hold this workspace UUID's sync lock here, so any rclone bisync lock
    // file in the workdir is from a dead, interrupted run — reap it so an
    // earlier crash (TUI quit, power off) never wedges this run.
    crate::sync::run::reap_stale_bisync_locks(&workdir);
    crate::logging::log("sync rclone start");
    reporter.line(&theme.info("Starting rclone sync; live file progress follows…"));
    let mut run = run_rclone(&reporter, &remote.env, &argv);
    crate::logging::log(format!(
        "sync rclone done exit_ok={} transferred={} deleted={} errors={} abort={:?}",
        run.exit_ok, run.transferred, run.deleted, run.errors, run.abort
    ));
    reporter.line(&theme.muted(&format_file_findings(&run)));
    let resumed = if should_auto_resync(dir, run.abort.as_ref()) {
        crate::logging::log("sync auto-resync start");
        reporter.line(&theme.warning(
            "rclone reported that its baseline listing is incomplete; establishing it with a one-time resync…",
        ));
        let resync_argv =
            args::bisync_args(cfg, &local, &remote.arg, &workdir_arg, Direction::Resync);
        run = run_rclone(&reporter, &remote.env, &resync_argv);
        crate::logging::log(format!(
            "sync auto-resync done exit_ok={} transferred={} deleted={} errors={} abort={:?}",
            run.exit_ok, run.transferred, run.deleted, run.errors, run.abort
        ));
        true
    } else {
        false
    };
    let auto_repaired = if should_auto_repair_check_access(dir, run.abort.as_ref()) {
        crate::logging::log("sync auto-repair check-access marker");
        reporter.line(&theme.warning(&format!(
            "The check-access marker is missing; running `{}` automatically to recreate it and re-establish the baseline…",
            crate::workspace::suggest("sync repair")
        )));
        reporter.line(&theme.info("Recreating the local and remote RCLONE_TEST markers…"));
        crate::sync::check_access::ensure_markers(root, &verified)?;
        reporter.line(&theme.info("Rebuilding the rclone baseline; live file progress follows…"));
        let repair_argv =
            args::bisync_args(cfg, &local, &remote.arg, &workdir_arg, Direction::Resync);
        run = run_rclone(&reporter, &remote.env, &repair_argv);
        crate::logging::log(format!(
            "sync auto-repair done exit_ok={} transferred={} deleted={} errors={} abort={:?}",
            run.exit_ok, run.transferred, run.deleted, run.errors, run.abort
        ));
        true
    } else {
        false
    };
    crate::logging::log("sync rename conflict markers");
    let renamed_count = conflicts::rename_markers(root, &hostname(), date);
    let renamed = u64::try_from(renamed_count).unwrap_or(0);
    let leftover = conflicts::leftover_markers(root);
    let outcome = verify::classify(&run, renamed_count, leftover);
    crate::logging::log(format!(
        "sync verify outcome={} renamed_conflicts={} leftover_markers={}",
        outcome.label(),
        renamed_count,
        leftover
    ));

    // The two task CSVs are excluded from bisync and reconciled out-of-band via
    // one preflighted operation. A failure stops publication and prevents the
    // dependent counters from advancing.
    let csv_note = if matches!(outcome, Outcome::Aborted(_)) {
        crate::logging::log("sync csv merge skipped after abort");
        reporter.line(&theme.warning(
            "  decision: skipping the task/habit merge — the file sync aborted, so its result cannot be trusted",
        ));
        String::new()
    } else if !reconcile_task_state {
        crate::logging::log("sync csv merge deferred to migration join");
        reporter.line(&theme.muted(
            "  decision: deferring the task/habit merge to the migration join that owns it",
        ));
        String::new()
    } else {
        crate::logging::log("sync csv merge start");
        reporter.line(&theme.info("Merging task and habit CSVs by row id…"));
        let _task_owner =
            crate::tasks::store_lock::TaskStoreOwner::acquire_path(&paths.task_store_lock())?;
        let csv = sync_task_state(
            || crate::sync::csv_sync::sync_csvs(paths, verified, root, dir),
            |floors| {
                reporter.line(&theme.info("Reconciling task and habit id counters…"));
                let counters = crate::sync::counters::sync_counters(verified, root, dir, floors);
                crate::logging::log(format!("sync id counters {counters:?}"));
            },
        )?;
        let note = format_csv_note(&csv.outcomes);
        crate::logging::log(format!("sync csv merge note={note:?}"));
        reporter.line(&theme.muted(&format!(
            "  found: {}",
            if note.is_empty() {
                "no task or habit rows differed"
            } else {
                note.as_str()
            }
        )));
        note
    };

    reporter.line(&journal_progress(theme));
    let journal = Journal::open(&paths.sync_journal())?;
    crate::logging::log(format!("sync journal {}", paths.sync_journal().display()));
    journal.record(&SyncRun {
        started_at: started_at.to_owned(),
        finished_at: finished_at.to_owned(),
        direction: direction_label(dir).to_owned(),
        outcome: outcome.label().to_owned(),
        transferred: run.transferred,
        deleted: run.deleted,
        conflicts: renamed,
        errors: run.errors,
        note: {
            let base = match &outcome {
                Outcome::Clean => String::new(),
                Outcome::NeedsAttention(m) | Outcome::Aborted(m) => m.clone(),
            };
            let base = if resumed {
                if base.is_empty() {
                    "auto-resumed after interrupted baseline".to_owned()
                } else {
                    format!("auto-resumed after interrupted baseline; {base}")
                }
            } else {
                base
            };
            let base = if auto_repaired {
                if base.is_empty() {
                    "auto-repaired missing check-access marker".to_owned()
                } else {
                    format!("auto-repaired missing check-access marker; {base}")
                }
            } else {
                base
            };
            join_notes(&base, &csv_note)
        },
    })?;
    crate::logging::log("sync journal recorded");
    Ok(outcome)
}

/// Run the rollout's final legacy semantic sync under the workspace sync lock.
pub fn run_legacy_migration_sync(
    context: &crate::workspace::CommandContext,
    config: &SyncConfig,
) -> Result<()> {
    let remote = build_remote(config);
    crate::sync::identity::require_remote_identity(
        context.workspace.root(),
        context.workspace.id(),
        &remote,
    )?;
    let remote_task_state =
        crate::sync::csv_sync::inspect_remote_task_state(context.workspace.paths(), &remote)?;
    let remote_schema =
        crate::sync::csv_merge::remote_schema_status(remote_task_state.schema.as_deref())?;
    let now = Utc::now();
    let started_at = now.to_rfc3339();
    let finished_at = Utc::now().to_rfc3339();
    let date = now.format("%Y-%m-%d").to_string();
    let reconcile_task_state = remote_schema == crate::sync::csv_merge::SchemaStatus::Legacy;
    let outcome = sync_once_with_task_state(
        context.workspace.paths(),
        context.workspace.id(),
        config,
        context.workspace.root(),
        Direction::Both,
        (&started_at, &finished_at, &date),
        reconcile_task_state,
    )?;
    match outcome {
        Outcome::Clean => {}
        Outcome::NeedsAttention(message) | Outcome::Aborted(message) => {
            bail!("final legacy semantic sync was not clean: {message}")
        }
    }
    if remote_schema == crate::sync::csv_merge::SchemaStatus::Current {
        let schema = remote_task_state
            .schema
            .as_deref()
            .expect("current remote schema must be present");
        crate::migration::join_legacy_to_current(context, &remote, schema)?;
    }
    Ok(())
}

mod reporting;
#[cfg(test)]
pub(crate) use reporting::copy_meta_from_fs;
pub use reporting::{
    CopyMeta, conflict_display_paths, conflicts_json, direction_from_flags, direction_label,
    format_csv_note, format_file_findings, format_in_progress, format_last_run, format_sync_plan,
    format_sync_plan_for_remote, format_triggers, format_unconfigured_sync_guidance, join_notes,
    journal_progress, print_conflicts, print_status, should_auto_repair_check_access,
    should_auto_resync, should_bootstrap_check_access, sync_progress,
};

#[cfg(test)]
mod tests;

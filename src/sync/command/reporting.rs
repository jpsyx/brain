/// Summarize the CSV merge outcomes into a journal note segment, e.g.
/// `csv: +3 ~2 -1 (1 soft)`. Empty when nothing was added, merged, deleted, or
/// soft-conflicted, so a clean run stays noise-free.
#[must_use]
pub fn format_csv_note(outcomes: &[CsvMergeOutcome]) -> String {
    let (added, merged, deleted, soft) = outcomes.iter().fold((0, 0, 0, 0), |acc, o| {
        (
            acc.0 + o.added,
            acc.1 + o.merged,
            acc.2 + o.deleted,
            acc.3 + o.soft_conflicts,
        )
    });
    if added == 0 && merged == 0 && deleted == 0 && soft == 0 {
        return String::new();
    }
    let mut note = format!("csv: +{added} ~{merged} -{deleted}");
    if soft > 0 {
        let _ = write!(note, " ({soft} soft)");
    }
    note
}

/// Join two note segments with `; `, dropping either when empty.
#[must_use]
pub fn join_notes(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_owned(),
        (_, true) => a.to_owned(),
        _ => format!("{a}; {b}"),
    }
}

/// Whether an interrupted/missing baseline should trigger one automatic resync.
///
/// True only when the run aborted with `PriorListingMissing` AND this wasn't
/// already a resync (a resync re-establishes the baseline, so never loop on it).
#[must_use]
pub fn should_auto_resync(dir: Direction, abort: Option<&crate::sync::run::AbortKind>) -> bool {
    dir != Direction::Resync
        && matches!(
            abort,
            Some(crate::sync::run::AbortKind::PriorListingMissing)
        )
}

/// Whether a normal sync should automatically run the narrow, low-risk repair
/// for a missing check-access marker. A resync never retries itself.
#[must_use]
pub fn should_auto_repair_check_access(
    dir: Direction,
    abort: Option<&crate::sync::run::AbortKind>,
) -> bool {
    dir != Direction::Resync && matches!(abort, Some(crate::sync::run::AbortKind::CheckAccess))
}

/// Whether this sync run should create/repair the check-access markers before
/// invoking rclone.
#[must_use]
pub fn should_bootstrap_check_access(dir: Direction) -> bool {
    dir == Direction::Resync
}

#[must_use]
pub fn format_sync_plan(cfg: &SyncConfig, root: &Path, dir: Direction, theme: Theme) -> String {
    let remote = build_remote(cfg);
    format_sync_plan_for_remote(root, &remote, dir, theme)
}

#[must_use]
pub fn format_sync_plan_for_remote(
    root: &Path,
    remote: &Remote,
    dir: Direction,
    theme: Theme,
) -> String {
    let heading = match dir {
        Direction::Both => "Syncing brain",
        Direction::Push => "Pushing local brain changes",
        Direction::Pull => "Pulling remote brain changes",
        Direction::Resync => "Repairing cloud sync metadata",
    };
    format!(
        "{}\n  {} {}\n  {} {}",
        theme.heading(heading),
        theme.muted("local:"),
        theme.value(&root.display().to_string()),
        theme.muted("remote:"),
        theme.value(&remote.arg),
    )
}

#[must_use]
pub fn sync_progress(dir: Direction) -> &'static str {
    match dir {
        Direction::Both => "Comparing local and remote changes, then syncing both directions…",
        Direction::Push => {
            "Uploading local additions and edits without downloading remote changes…"
        }
        Direction::Pull => "Comparing local and remote changes, then pulling remote changes…",
        Direction::Resync => "Checking the sync marker and rebuilding the rclone baseline…",
    }
}

#[must_use]
pub fn journal_progress(theme: Theme) -> String {
    theme.info("Recording this run in the workspace sync journal…")
}

#[must_use]
pub fn direction_label(dir: Direction) -> &'static str {
    match dir {
        Direction::Both => "both",
        Direction::Push => "push",
        Direction::Pull => "pull",
        Direction::Resync => "resync",
    }
}

/// Map the `--push`/`--pull` flags to a `Direction` for a bare `brain sync`.
pub fn direction_from_flags(push: bool, pull: bool) -> Result<Direction> {
    match (push, pull) {
        (true, true) => bail!("--push and --pull are mutually exclusive"),
        (true, false) => Ok(Direction::Push),
        (false, true) => Ok(Direction::Pull),
        (false, false) => Ok(Direction::Both),
    }
}

/// User-facing guidance for sync commands run before `brain sync setup`.
#[must_use]
pub fn format_unconfigured_sync_guidance(dir: Direction, theme: Theme) -> String {
    let setup = theme.accent("brain sync setup");
    if dir == Direction::Resync {
        return format!(
            "{}\n\n`{}` only repairs an existing sync setup: it recreates the RCLONE_TEST marker and re-establishes the rclone baseline. It does not collect Backblaze credentials or enable cloud sync.\n\nRun `{setup}`.",
            theme.warning("Cloud sync is not set up yet."),
            theme.accent("brain sync repair"),
        );
    }
    format!(
        "{}\n\nRun `{setup}` to connect a private Backblaze B2 bucket, save this machine's sync credentials, create the RCLONE_TEST marker, and establish the first baseline.",
        theme.warning("Cloud sync is not set up yet."),
    )
}

/// Format the "a sync is running right now" status line (pure).
#[must_use]
pub fn format_in_progress(state: &crate::sync::current::CurrentState, theme: Theme) -> String {
    format!(
        "{} {} · started {} · {}",
        theme.info("syncing now:"),
        theme.accent(&state.direction),
        theme.value(&state.started_at),
        theme.muted(&format!("pid {}", state.pid)),
    )
}

/// Format the status line for the most recent journal run (pure).
#[must_use]
pub fn format_last_run(run: Option<&SyncRun>, theme: Theme) -> String {
    run.map_or_else(
        || "no syncs yet — run `brain sync`.".to_owned(),
        |r| {
            let outcome = match r.outcome.as_str() {
                "clean" => theme.success(&r.outcome),
                "needs_attention" => theme.warning(&r.outcome),
                "aborted" => theme.error(&r.outcome),
                _ => r.outcome.clone(),
            };
            format!(
                "last sync: {} · {} · {} · {}↑ {}↓ {} conflicts{}",
                theme.muted(&r.finished_at),
                theme.accent(&r.direction),
                outcome,
                theme.accent(&r.transferred.to_string()),
                theme.accent(&r.deleted.to_string()),
                theme.accent(&r.conflicts.to_string()),
                if r.note.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", theme.muted(&r.note))
                },
            )
        },
    )
}

/// Format the configured auto-sync triggers. The flags are honored once the
/// trigger/watcher phase lands; `status` shows them so the setup is visible.
#[must_use]
pub fn format_triggers(cfg: &SyncConfig, theme: Theme) -> String {
    let yn = |b: bool| {
        if b {
            theme.success("on")
        } else {
            theme.muted("off")
        }
    };
    let watch_on = cfg.watch_effective();
    let debounce = if watch_on {
        format!(
            " {}",
            theme.muted(&format!("({}ms debounce)", cfg.debounce_ms))
        )
    } else {
        String::new()
    };
    format!(
        "{} startup-pull {} · change-push {}{} · message-pull {}",
        theme.muted("triggers:"),
        yn(cfg.is_configured()),
        yn(watch_on),
        debounce,
        theme.success("after 2h"),
    )
}

/// Print `brain sync status`.
pub fn print_status(
    paths: &crate::workspace::WorkspacePaths,
    cfg: &SyncConfig,
    root: &Path,
) -> Result<()> {
    let theme = Theme::active();
    if !cfg.is_configured() {
        crate::logging::log("sync status unconfigured");
        println!(
            "{}",
            format_unconfigured_sync_guidance(Direction::Both, theme)
        );
        return Ok(());
    }
    crate::logging::log(format!(
        "sync status journal={} root={}",
        paths.sync_journal().display(),
        root.display()
    ));
    // Surface a sync happening right now (in a detached background process or
    // another shell) above the last completed run, so status always answers
    // "is anything syncing?" first.
    if let Some(state) = crate::sync::current::read_state(paths) {
        if crate::server::lifecycle::pid_alive(state.pid) {
            crate::logging::log("sync status in-progress");
            println!("{}", format_in_progress(&state, theme));
        }
    }
    let recent = Journal::recent_read_only(&paths.sync_journal(), 1)?;
    println!("{}", format_last_run(recent.first(), theme));
    println!("{}", format_triggers(cfg, theme));
    let conflicts = conflicts::list_conflicts(root);
    let count = conflicts.len();
    crate::logging::log(format!("sync status conflicts={count}"));
    let label = if count > 0 {
        theme.warning("open conflicts:")
    } else {
        theme.muted("open conflicts:")
    };
    println!("{} {}", label, theme.accent(&count.to_string()));
    Ok(())
}

/// Per-copy filesystem metadata, injected so [`conflicts_json`] stays pure.
pub struct CopyMeta {
    pub modified: Option<String>,
    pub bytes: Option<u64>,
}

/// Build the `brain sync conflicts --json` value. Pure.
///
/// `meta(rel_path)` supplies each copy's metadata (`None` fields serialize to
/// JSON `null`); `exists(original)` says whether the canonical file is
/// present. An empty `groups` slice builds an empty JSON array.
#[must_use]
pub fn conflicts_json(
    groups: &[ConflictGroup],
    meta: impl Fn(&Path) -> CopyMeta,
    exists: impl Fn(&Path) -> bool,
) -> serde_json::Value {
    let value: Vec<serde_json::Value> = groups
        .iter()
        .map(|g| {
            let copies: Vec<serde_json::Value> = g
                .copies
                .iter()
                .map(|c| {
                    let m = meta(&c.path);
                    serde_json::json!({
                        "path": c.path.display().to_string(),
                        "host": c.host,
                        "date": c.date,
                        "modified": m.modified,
                        "bytes": m.bytes,
                    })
                })
                .collect();
            serde_json::json!({
                "original": g.original.display().to_string(),
                "original_exists": exists(&g.original),
                "copies": copies,
            })
        })
        .collect();
    serde_json::Value::Array(value)
}

/// Read a copy's mtime/size off disk for [`conflicts_json`]; missing file or
/// unreadable mtime degrades to `None` rather than failing the whole command.
pub(crate) fn copy_meta_from_fs(root: &Path, rel: &Path) -> CopyMeta {
    let Ok(m) = fs::metadata(root.join(rel)) else {
        return CopyMeta {
            modified: None,
            bytes: None,
        };
    };
    let modified = m.modified().ok().map(|t| {
        DateTime::<Utc>::from(t)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    });
    CopyMeta {
        modified,
        bytes: Some(m.len()),
    }
}

/// Human-list paths for `brain sync conflicts`.
///
/// Built from the same strict grouping parser as `--json` so both surfaces
/// agree on what is a real friendly conflict copy.
#[must_use]
pub fn conflict_display_paths(files: &[conflicts::ConflictFile]) -> Vec<std::path::PathBuf> {
    conflicts::group_conflicts(files)
        .into_iter()
        .flat_map(|group| group.copies.into_iter().map(|copy| copy.path))
        .collect()
}

/// Print `brain sync conflicts`. `json == true` emits the structured
/// `conflicts_json` shape to stdout; otherwise this is the themed human list
/// rendered through the same strict grouping path.
pub fn print_conflicts(root: &Path, json: bool) -> Result<()> {
    let theme = Theme::active();
    crate::logging::log(format!(
        "sync conflicts scan root={} json={json}",
        root.display()
    ));
    let conflicts = conflicts::list_conflicts(root);
    crate::logging::log(format!("sync conflicts raw_copies={}", conflicts.len()));
    if json {
        let groups = conflicts::group_conflicts(&conflicts);
        let meta = |rel: &Path| copy_meta_from_fs(root, rel);
        let exists = |rel: &Path| root.join(rel).exists();
        let value = conflicts_json(&groups, meta, exists);
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        let display_paths = conflict_display_paths(&conflicts);
        if display_paths.is_empty() {
            println!("{}", theme.muted("no open conflict copies."));
        } else {
            for path in display_paths {
                println!("{}", theme.value(&path.display().to_string()));
            }
        }
    }
    Ok(())
}
use super::*;

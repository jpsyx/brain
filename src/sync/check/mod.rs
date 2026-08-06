//! `brain check`: a read-only, themed report of pending sync changes (what a
//! `brain sync` would push/pull), built on a dry-run `rclone bisync`.

use std::path::Path;

use crate::sync::config::SyncConfig;
use crate::sync::csv_sync::{CSVS, baseline_path, remote_csv_arg};
use crate::sync::progress::{self, Change, Side};
use crate::theme::Theme;

mod csv;

pub use csv::{
    CsvCheckError, CsvPending, CsvSideDiff, collect_csv_pending_with_fetch, csv_pending_from_texts,
    diff_csv_rows, format_csv_check_error,
};

#[derive(Debug, Clone, Copy)]
enum CsvDirection {
    Push,
    Pull,
}

/// Build the themed `brain check` report from the detected changes. Pure.
/// `push` = local changes to upload, `pull` = remote changes to download.
#[must_use]
pub fn format_report(push: &[String], pull: &[String], csv: &[CsvPending], theme: Theme) -> String {
    let push_count = pending_count(push, csv, CsvDirection::Push);
    let pull_count = pending_count(pull, csv, CsvDirection::Pull);
    let unchecked = unchecked_remote_csvs(csv);
    if push_count == 0 && pull_count == 0 && unchecked.is_empty() {
        return theme.success("✓ In sync — nothing to push or pull.");
    }

    let mut lines = Vec::new();
    for (label, side, count, direction) in [
        ("push", push, push_count, CsvDirection::Push),
        ("pull", pull, pull_count, CsvDirection::Pull),
    ] {
        if count == 0 {
            continue;
        }
        lines.push(theme.heading(&format!("Changes to {label} ({count}):")));
        for summary in progress::summarize(side) {
            lines.push(format!("  {}", theme.value(&summary)));
        }
        for summary in csv_summaries(csv, direction) {
            lines.push(format!("  {}", theme.value(&summary)));
        }
    }

    if !unchecked.is_empty() {
        lines.push(theme.warning(&format!(
            "Could not check remote CSV changes for {}.",
            unchecked.join(", ")
        )));
    }

    if push_count == 0 && pull_count == 0 {
        return lines.join("\n");
    }

    if has_csv_deltas(csv) {
        lines.push(String::new());
        lines.push(theme.muted(
            "CSV rows are compared against this machine's cached baseline; pull rows are not proof that another machine made the change. brain sync will merge tasks.csv/habits.csv by id.",
        ));
    }

    let brain_sync = theme.accent("brain sync");
    let suggestion = match (push_count == 0, pull_count == 0) {
        (false, false) => format!("Run `{brain_sync}` to push and pull all changes."),
        (false, true) => format!("Run `{brain_sync}` to push your changes."),
        (true, false) => format!("Run `{brain_sync}` to pull the latest changes."),
        (true, true) => unreachable!("early-returned above when both sides are empty"),
    };
    lines.push(String::new());
    lines.push(suggestion);

    lines.join("\n")
}

fn pending_count(files: &[String], csv: &[CsvPending], direction: CsvDirection) -> usize {
    files.len()
        + csv
            .iter()
            .filter_map(|pending| csv_diff(pending, direction))
            .map(CsvSideDiff::total)
            .sum::<usize>()
}

fn csv_diff(csv: &CsvPending, direction: CsvDirection) -> Option<CsvSideDiff> {
    match direction {
        CsvDirection::Push => Some(csv.push),
        CsvDirection::Pull => csv.pull,
    }
}

fn csv_summaries(csv: &[CsvPending], direction: CsvDirection) -> Vec<String> {
    csv.iter()
        .filter_map(|pending| {
            let diff = csv_diff(pending, direction)?;
            (!diff.is_empty()).then(|| {
                format!(
                    "{}: +{} ~{} -{} rows",
                    pending.name, diff.added, diff.changed, diff.deleted
                )
            })
        })
        .collect()
}

fn unchecked_remote_csvs(csv: &[CsvPending]) -> Vec<String> {
    csv.iter()
        .filter(|pending| pending.pull.is_none())
        .map(|pending| pending.name.clone())
        .collect()
}

fn has_csv_deltas(csv: &[CsvPending]) -> bool {
    csv.iter().any(|pending| {
        !pending.push.is_empty() || pending.pull.is_some_and(|diff| !diff.is_empty())
    })
}

fn collect_csv_pending(
    paths: &crate::workspace::WorkspacePaths,
    root: &Path,
    remote_env: &[(String, String)],
    remote_arg: &str,
) -> Result<Vec<CsvPending>, CsvCheckError> {
    crate::logging::log(format!("check csv pending root={}", root.display()));
    collect_csv_pending_with_fetch(
        root,
        &CSVS,
        |name| {
            let path = baseline_path(paths, name);
            crate::logging::log(format!("check csv baseline {}", path.display()));
            match std::fs::read_to_string(path) {
                Ok(text) => Ok(text),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                Err(error) => Err(format!("reading baseline: {error}")),
            }
        },
        |rel| {
            crate::logging::log(format!("check csv remote {rel}"));
            fetch_remote_csv(paths, remote_env, remote_arg, rel)
        },
    )
}

fn fetch_remote_csv(
    paths: &crate::workspace::WorkspacePaths,
    remote_env: &[(String, String)],
    remote_arg: &str,
    rel: &str,
) -> Option<String> {
    let temporary_dir = paths.sync_dir().join("tmp");
    let _ = std::fs::create_dir_all(&temporary_dir);
    let tmp = temporary_dir.join(format!(
        "csv-check-{}-{}",
        std::process::id(),
        rel.replace('/', "_")
    ));
    let args = [
        "copyto".to_owned(),
        remote_csv_arg(remote_arg, rel),
        tmp.to_string_lossy().into_owned(),
    ];
    crate::logging::log(format!(
        "check fetch remote csv rel={rel} tmp={}",
        tmp.display()
    ));
    let (ok, _) = crate::sync::run::run_rclone_capture(remote_env, &args);
    let text = ok.then(|| std::fs::read_to_string(&tmp).ok()).flatten();
    let _ = std::fs::remove_file(&tmp);
    text
}

/// Run `brain check`: dry-run bisync, classify pending changes, print the report.
///
/// Thin IO shell; the report text itself is built by [`format_report`].
/// Detection errors remain themed warnings, but an unverified remote identity
/// is a hard refusal before any remote data reads.
pub fn run(
    paths: &crate::workspace::WorkspacePaths,
    workspace_id: crate::workspace::WorkspaceId,
    cfg: &SyncConfig,
    root: &std::path::Path,
) -> anyhow::Result<()> {
    let theme = Theme::active();
    if !cfg.is_configured() {
        crate::logging::log("check unconfigured");
        println!(
            "{}",
            crate::sync::command::format_unconfigured_sync_guidance(
                crate::sync::args::Direction::Both,
                theme,
            )
        );
        return Ok(());
    }
    let remote = crate::sync::remote::build_remote(cfg);
    crate::logging::log(format!(
        "check root={} remote={}",
        root.display(),
        remote.arg
    ));
    println!(
        "{}",
        theme.muted("Validating the local workspace manifest…")
    );
    println!("{}", theme.muted("Probing the remote workspace identity…"));
    let verified = crate::sync::identity::require_remote_identity(root, workspace_id, &remote)?;
    let remote = verified.remote();
    let local = root.to_string_lossy().into_owned();
    let workdir = crate::sync::run::bisync_workdir(paths);
    let mut args = crate::sync::args::bisync_args(
        cfg,
        &local,
        &remote.arg,
        &workdir.to_string_lossy(),
        crate::sync::args::Direction::Both,
    );
    args.push("--dry-run".into());
    println!(
        "{}",
        theme.muted("Checking file changes with rclone dry-run…")
    );
    crate::logging::log("check rclone dry-run start");
    let (exit_ok, output) = crate::sync::run::run_rclone_capture(&remote.env, &args);
    crate::logging::log(format!(
        "check rclone dry-run done exit_ok={} output_bytes={}",
        exit_ok,
        output.len()
    ));
    // No baseline yet? bisync aborts with prior-listing-missing.
    if !exit_ok && (output.contains("cannot find prior") || output.contains("Must run --resync")) {
        crate::logging::log("check missing baseline");
        println!(
            "{}",
            theme.warning("No sync baseline yet — run `brain sync` to establish it.")
        );
        return Ok(());
    }
    let changes: Vec<Change> = output
        .lines()
        .filter_map(|l| progress::classify_change(&progress::strip(l)))
        .collect();
    let push: Vec<String> = changes
        .iter()
        .filter(|c| c.side == Side::Push)
        .map(|c| c.path.clone())
        .collect();
    let pull: Vec<String> = changes
        .iter()
        .filter(|c| c.side == Side::Pull)
        .map(|c| c.path.clone())
        .collect();
    crate::logging::log(format!(
        "check file changes push={} pull={}",
        push.len(),
        pull.len()
    ));
    println!("{}", theme.muted("Checking task and habit CSV baselines…"));
    match collect_csv_pending(paths, root, &remote.env, &remote.arg) {
        Ok(csv) => {
            crate::logging::log(format!("check csv files={}", csv.len()));
            println!("{}", format_report(&push, &pull, &csv, theme));
        }
        Err(error) => {
            crate::logging::log(format!("check csv failed: {error}"));
            println!("{}", format_csv_check_error(&error, theme));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

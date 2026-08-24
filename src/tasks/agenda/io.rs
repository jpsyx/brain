//! The thin, best-effort filesystem shell around [`super::sync_markdown`].
//!
//! Best-effort is deliberate: the CSVs are the source of truth and the agenda
//! is downstream, so a missing agenda, an unreadable file, or a broken PDF
//! renderer must never fail the mutation that already succeeded. Every failure
//! is logged and swallowed.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::NaiveDate;

use super::{Action, Snapshot, sync_markdown};
use crate::tasks::complete::read_csv;

/// What a sync did, so the caller can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// No agenda file for that date — nothing to keep in sync.
    NoAgenda,
    /// The agenda was already accurate.
    Unchanged,
    /// The agenda was rewritten; `pdf` is true when a printable was regenerated.
    Updated { pdf: bool },
}

/// Everything one sync touches, resolved up front so the whole shell is
/// testable against a temporary directory.
#[derive(Debug, Clone)]
pub(crate) struct Targets {
    /// The day's agenda markdown.
    pub(crate) markdown: PathBuf,
    /// The printable, regenerated only when it already exists.
    pub(crate) pdf: PathBuf,
    /// The `markdown-to-pdf` command, when one is configured and usable.
    pub(crate) renderer: Option<PathBuf>,
    /// The workspace's `tasks/` directory, holding both CSVs.
    pub(crate) tasks_dir: PathBuf,
}

/// The agenda markdown for `date`, inside the configured directory.
pub(crate) fn markdown_path(directory: &Path, date: NaiveDate) -> PathBuf {
    directory.join(format!("{date}.md"))
}

/// Resolve the day's targets from the selected workspace's configuration.
pub(crate) fn resolve_targets(
    command: &crate::workspace::CommandContext,
    date: NaiveDate,
) -> Targets {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let agenda_dir = crate::settings::resolve_one(&command.workspace, "agenda_dir").map_or_else(
        || home.join("Downloads"),
        |raw| crate::paths::expand_tilde_with_home(&raw, &home),
    );
    let markdown_dir = crate::env::resolve_one(command, "agenda_markdown_dir").map_or_else(
        || PathBuf::from("/tmp"),
        |raw| crate::paths::expand_tilde_with_home(&raw, &home),
    );
    Targets {
        markdown: markdown_path(&markdown_dir, date),
        pdf: agenda_dir.join(format!("agenda-{date}.pdf")),
        renderer: crate::settings::markdown_to_pdf_command(command).ok(),
        tasks_dir: command.workspace.root().join("tasks"),
    }
}

/// Sync the day's agenda after a mutation to `task_id`.
pub(crate) fn sync_after_mutation(
    command: &crate::workspace::CommandContext,
    task_id: &str,
    action: Action,
    today: NaiveDate,
) -> Outcome {
    sync_targets(&resolve_targets(command, today), task_id, action, today)
}

/// Read, sync, write, and (only when a printable already exists) regenerate.
pub(crate) fn sync_targets(
    targets: &Targets,
    task_id: &str,
    action: Action,
    today: NaiveDate,
) -> Outcome {
    if !targets.markdown.exists() {
        return Outcome::NoAgenda;
    }
    let Ok(text) = std::fs::read_to_string(&targets.markdown) else {
        crate::logging::log(format!(
            "agenda sync: could not read {}",
            targets.markdown.display()
        ));
        return Outcome::NoAgenda;
    };
    let (Ok(tasks), Ok(habits)) = (
        read_csv(&targets.tasks_dir.join("tasks.csv")),
        read_csv(&targets.tasks_dir.join("habits.csv")),
    ) else {
        crate::logging::log("agenda sync: could not read the task CSVs");
        return Outcome::NoAgenda;
    };
    let snapshot = Snapshot {
        tasks: &tasks.rows,
        habits: &habits.rows,
    };
    let synced = sync_markdown(&text, task_id, action, &snapshot, today);
    if synced == text {
        return Outcome::Unchanged;
    }
    if let Err(error) = std::fs::write(&targets.markdown, &synced) {
        crate::logging::log(format!(
            "agenda sync: could not write {}: {error}",
            targets.markdown.display()
        ));
        return Outcome::NoAgenda;
    }
    Outcome::Updated {
        pdf: regenerate_pdf(targets, &synced),
    }
}

/// Strip HTML comments before rendering.
///
/// `markdown-to-pdf` is a bespoke line-based renderer with no concept of HTML,
/// so a comment (the appendix idempotency marker, most often) would print as
/// literal visible text. The marker has to stay in the *source* file — whoever
/// bakes the appendix greps for it — so only the rendered copy is stripped.
pub(super) fn strip_html_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("<!--") {
        let Some(close) = rest[open..].find("-->") else {
            break;
        };
        out.push_str(rest[..open].trim_end_matches([' ', '\t']));
        rest = &rest[open + close + "-->".len()..];
    }
    out.push_str(rest);
    out
}

/// Regenerate the printable **only if one already exists**: a CSV mutation
/// isn't a request for a fresh printout, but a printout on disk must stay
/// current. Returns whether a PDF was rebuilt.
fn regenerate_pdf(targets: &Targets, markdown: &str) -> bool {
    if !targets.pdf.exists() {
        return false;
    }
    let Some(renderer) = &targets.renderer else {
        crate::logging::log(
            "agenda sync: no markdown-to-pdf command configured; skipping PDF regen",
        );
        return false;
    };
    let stripped = strip_html_comments(markdown);
    let source = if stripped == markdown {
        targets.markdown.clone()
    } else {
        let staged = targets.markdown.with_extension("render.md");
        if std::fs::write(&staged, &stripped).is_err() {
            crate::logging::log("agenda sync: could not stage the PDF source");
            return false;
        }
        staged
    };
    // The renderer writes a `-vN` variant rather than overwriting, so the stale
    // printable has to go first for the path to stay stable.
    let _ = std::fs::remove_file(&targets.pdf);
    let status = Command::new(renderer)
        .arg(&source)
        .arg("--out")
        .arg(&targets.pdf)
        .arg("--agenda")
        .status();
    if source != targets.markdown {
        let _ = std::fs::remove_file(&source);
    }
    match status {
        Ok(status) if status.success() => true,
        Ok(status) => {
            crate::logging::log(format!("agenda sync: PDF regen exited {status}"));
            false
        }
        Err(error) => {
            crate::logging::log(format!("agenda sync: PDF regen failed: {error}"));
            false
        }
    }
}

//! `brain check`: a read-only, themed report of pending sync changes (what a
//! `brain sync` would push/pull), built on a dry-run `rclone bisync`.

use crate::sync::config::SyncConfig;
use crate::sync::progress::{self, Change, Side};
use crate::theme::Theme;

/// Build the themed `brain check` report from the detected changes. Pure.
/// `push` = local changes to upload, `pull` = remote changes to download.
#[must_use]
pub fn format_report(push: &[String], pull: &[String], theme: Theme) -> String {
    if push.is_empty() && pull.is_empty() {
        return theme.success("✓ In sync — nothing to push or pull.");
    }

    let mut lines = Vec::new();
    for (label, side) in [("push", push), ("pull", pull)] {
        if side.is_empty() {
            continue;
        }
        lines.push(theme.heading(&format!("Changes to {label} ({}):", side.len())));
        for summary in progress::summarize(side) {
            lines.push(format!("  {}", theme.value(&summary)));
        }
    }

    let brain_sync = theme.accent("brain sync");
    let suggestion = match (push.is_empty(), pull.is_empty()) {
        (false, false) => format!("Run `{brain_sync}` to push and pull all changes."),
        (false, true) => format!("Run `{brain_sync}` to push your changes."),
        (true, false) => format!("Run `{brain_sync}` to pull the latest changes."),
        (true, true) => unreachable!("early-returned above when both sides are empty"),
    };
    lines.push(String::new());
    lines.push(suggestion);

    lines.join("\n")
}

/// Run `brain check`: dry-run bisync, classify pending changes, print the report.
///
/// Thin IO shell; the report text itself is built by [`format_report`]. Never
/// fails: rclone/IO errors surface as a themed warning rather than a hard
/// error, since this is a read-only convenience report, not a sync.
pub fn run(cfg: &SyncConfig, root: &std::path::Path) {
    let theme = Theme::active();
    if !cfg.is_configured() {
        println!("{}", theme.warning("sync is not configured — run `brain sync setup`."));
        return;
    }
    let remote = crate::sync::remote::build_remote(cfg);
    let local = root.to_string_lossy().into_owned();
    let mut args =
        crate::sync::args::bisync_args(cfg, &local, &remote.arg, crate::sync::args::Direction::Both);
    args.push("--dry-run".into());
    println!("{}", theme.muted("Checking for changes…"));
    let (exit_ok, output) = crate::sync::run::run_rclone_capture(&remote.env, &args);
    // No baseline yet? bisync aborts with prior-listing-missing.
    if !exit_ok && (output.contains("cannot find prior") || output.contains("Must run --resync")) {
        println!("{}", theme.warning("No sync baseline yet — run `brain sync` to establish it."));
        return;
    }
    let changes: Vec<Change> =
        output.lines().filter_map(|l| progress::classify_change(&progress::strip(l))).collect();
    let push: Vec<String> =
        changes.iter().filter(|c| c.side == Side::Push).map(|c| c.path.clone()).collect();
    let pull: Vec<String> =
        changes.iter().filter(|c| c.side == Side::Pull).map(|c| c.path.clone()).collect();
    println!("{}", format_report(&push, &pull, theme));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_sync_when_nothing_to_push_or_pull() {
        let t = Theme::dark(false);
        let report = format_report(&[], &[], t);
        assert!(report.contains("In sync"), "{report:?}");
        assert!(!report.contains("brain sync"), "{report:?}");
    }

    #[test]
    fn push_only_reports_count_summary_and_push_suggestion() {
        let t = Theme::dark(false);
        let push = vec!["notes/a.md".to_string(), "notes/b.md".to_string()];
        let report = format_report(&push, &[], t);
        assert!(report.contains("Changes to push (2)"), "{report:?}");
        assert!(report.contains("2 changes in notes/"), "{report:?}");
        assert!(report.contains("Run `brain sync` to push your changes."), "{report:?}");
    }

    #[test]
    fn pull_only_reports_pull_suggestion() {
        let t = Theme::dark(false);
        let pull = vec!["remote-added.md".to_string()];
        let report = format_report(&[], &pull, t);
        assert!(report.contains("Changes to pull (1)"), "{report:?}");
        assert!(report.contains("Run `brain sync` to pull the latest changes."), "{report:?}");
    }

    #[test]
    fn both_sides_report_push_and_pull_suggestion() {
        let t = Theme::dark(false);
        let push = vec!["a.md".to_string()];
        let pull = vec!["b.md".to_string()];
        let report = format_report(&push, &pull, t);
        assert!(report.contains("Changes to push (1)"), "{report:?}");
        assert!(report.contains("Changes to pull (1)"), "{report:?}");
        assert!(report.contains("Run `brain sync` to push and pull all changes."), "{report:?}");
    }

    #[test]
    fn colored_suggestion_wraps_brain_sync_in_accent() {
        let t = Theme::dark(true);
        let push = vec!["a.md".to_string()];
        let report = format_report(&push, &[], t);
        assert!(report.contains("\x1b[96mbrain sync\x1b[0m"), "{report:?}");
    }
}

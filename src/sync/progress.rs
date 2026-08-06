//! Pure parsing of `rclone bisync` log output into structured events.
//!
//! This is the foundation for a clean live file-list during `brain sync`
//! and a `brain check` summary. No IO here: callers strip ANSI/prefix with
//! [`strip`], classify each already-stripped line with [`classify_applied`]
//! (apply phase) or [`classify_change`] (detection phase), and roll up
//! detected paths with [`summarize`].

/// Strip ANSI SGR escapes and a leading rclone `<timestamp> LEVEL  : ` prefix
/// from one raw log line.
///
/// If there's no such prefix, only the ANSI escapes are stripped. Trailing
/// whitespace is trimmed either way.
#[must_use]
pub fn strip(raw: &str) -> String {
    let no_ansi = strip_ansi(raw);
    strip_prefix(&no_ansi).trim_end().to_string()
}

/// Remove ANSI SGR escape sequences (`\x1b[...m`) from `s`.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            for esc in chars.by_ref() {
                if esc == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Strip a leading `<timestamp> LEVEL  : ` prefix, e.g.
/// `2026/07/25 15:59:55 INFO  : `. If the line doesn't start with that shape,
/// return it unchanged.
fn strip_prefix(s: &str) -> &str {
    let Some((prefix, rest)) = s.split_once(": ") else {
        return s;
    };
    // `prefix` should look like "<date> <time> LEVEL " (e.g.
    // "2026/07/25 15:59:55 INFO ") — date and time separated by a space, then
    // a level word. Validate loosely: at least 3 whitespace-separated
    // segments, first one contains '/', second contains ':'.
    let mut parts = prefix.split_whitespace();
    let (Some(date), Some(time), Some(_level)) = (parts.next(), parts.next(), parts.next()) else {
        return s;
    };
    if date.contains('/') && time.contains(':') {
        rest
    } else {
        s
    }
}

/// One event from the rclone bisync apply phase (what actually synced).
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    /// A file that was copied, with its path.
    Copied(String),
    /// A file that was deleted, with its path.
    Deleted(String),
    /// The stats one-liner text, e.g. `"32 B / 32 B, 100%, 0 B/s, ETA -"`.
    Progress(String),
    /// `"Bisync successful"`.
    Done,
    /// Aborted: too many deletes (safety guard).
    AbortMaxDelete,
    /// Aborted: cannot find prior listing / must run `--resync`.
    AbortPriorListing,
}

/// Classify one already-stripped apply-phase line. Returns `None` for noise.
#[must_use]
pub fn classify_applied(clean: &str) -> Option<Applied> {
    if let Some(path) = clean
        .strip_suffix("Deleted")
        .and_then(|s| s.strip_suffix(": "))
    {
        return Some(Applied::Deleted(path.to_string()));
    }
    if let Some((path, rest)) = clean.split_once(": Copied") {
        let _ = rest; // trailing detail text (new / replaced existing / server-side copy) is ignored
        return Some(Applied::Copied(path.to_string()));
    }
    if clean.contains(" / ") && clean.contains("%,") && clean.contains("ETA") {
        return Some(Applied::Progress(clean.to_string()));
    }
    if clean == "Bisync successful" {
        return Some(Applied::Done);
    }
    if clean.contains("too many deletes") {
        return Some(Applied::AbortMaxDelete);
    }
    if clean.contains("cannot find prior") || clean.contains("Must run --resync") {
        return Some(Applied::AbortPriorListing);
    }
    None
}

/// Render an apply event as a clean, themed display line for the live sync
/// output, or `None` to suppress (progress ticks and `Done` are handled by the
/// caller). Copies/deletes show the file that synced.
#[must_use]
pub fn render_applied(event: &Applied, theme: crate::theme::Theme) -> Option<String> {
    match event {
        Applied::Copied(path) => Some(format!("  {} {}", theme.success("✓"), theme.value(path))),
        Applied::Deleted(path) => Some(format!(
            "  {} {} {}",
            theme.error("✗"),
            theme.muted(path),
            theme.muted("(deleted)")
        )),
        Applied::Progress(s) => Some(format!("  {}", theme.muted(s))),
        Applied::Done | Applied::AbortMaxDelete | Applied::AbortPriorListing => None,
    }
}

/// Which side of the bisync a detected change came from.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Side {
    /// Path1: local, pushed to the remote.
    Push,
    /// Path2: remote, pulled to local.
    Pull,
}

/// What kind of change was detected.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ChangeKind {
    /// A new file.
    New,
    /// An existing file that changed (size and/or time).
    Changed,
    /// A file that was deleted.
    Deleted,
}

/// A single detected (not-yet-applied) change, from the bisync detection
/// phase.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub side: Side,
    pub kind: ChangeKind,
    pub path: String,
}

/// Classify one already-stripped detection line, else `None`.
#[must_use]
pub fn classify_change(clean: &str) -> Option<Change> {
    if clean.contains("Queue") {
        return None;
    }
    let side = if clean.contains("Path1") {
        Side::Push
    } else if clean.contains("Path2") {
        Side::Pull
    } else {
        return None;
    };
    let kind = if clean.contains("File is new") {
        ChangeKind::New
    } else if clean.contains("File changed") {
        ChangeKind::Changed
    } else if clean.contains("File was deleted") {
        ChangeKind::Deleted
    } else {
        return None;
    };
    let path = clean.rsplit(" - ").next()?.trim();
    if path.is_empty() {
        return None;
    }
    Some(Change {
        side,
        kind,
        path: path.to_string(),
    })
}

/// Group changed paths into short human sentences by top-level segment, most
/// changes first.
///
/// A path with a `/` groups under its first segment as a directory (`"N
/// changes in notes/"`); a top-level file is named directly (`"1 change to
/// top.md"`). Singular/plural is correct (`"1 change"` vs `"N changes"`).
/// Groups are sorted by count descending, then by name ascending for ties.
#[must_use]
pub fn summarize(paths: &[String]) -> Vec<String> {
    use std::collections::BTreeMap;

    // key: either a top-level dir name ("notes") or a whole top-level file
    // name ("top.md"); value: (count, is_dir).
    let mut groups: BTreeMap<String, (usize, bool)> = BTreeMap::new();
    for path in paths {
        let (key, is_dir) = path
            .split_once('/')
            .map_or_else(|| (path.clone(), false), |(dir, _)| (dir.to_string(), true));
        let entry = groups.entry(key).or_insert((0, is_dir));
        entry.0 += 1;
    }

    let mut rows: Vec<(String, usize, bool)> = groups
        .into_iter()
        .map(|(name, (count, is_dir))| (name, count, is_dir))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    rows.into_iter()
        .map(|(name, count, is_dir)| {
            let noun = if count == 1 { "change" } else { "changes" };
            if is_dir {
                format!("{count} {noun} in {name}/")
            } else {
                format!("{count} {noun} to {name}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_ansi_and_timestamp_level_prefix() {
        let raw = "\x1b[36m2026/07/25 15:59:55\x1b[0m INFO  : \x1b[32mbrandnew.md\x1b[0m: Copied (server-side copy)";
        assert_eq!(strip(raw), "brandnew.md: Copied (server-side copy)");
    }

    #[test]
    fn strip_with_no_prefix_just_removes_ansi() {
        let raw = "\x1b[32mBisync successful\x1b[0m";
        assert_eq!(strip(raw), "Bisync successful");
    }

    #[test]
    fn strip_trims_trailing_whitespace() {
        let raw = "2026/07/25 15:59:55 NOTICE: Bisync successful   \n";
        assert_eq!(strip(raw), "Bisync successful");
    }

    #[test]
    fn classify_applied_copied_variants_all_map_to_copied_with_path() {
        assert_eq!(
            classify_applied("brandnew.md: Copied (server-side copy)"),
            Some(Applied::Copied("brandnew.md".to_string()))
        );
        assert_eq!(
            classify_applied("notes/one.md: Copied (new)"),
            Some(Applied::Copied("notes/one.md".to_string()))
        );
        assert_eq!(
            classify_applied("changeme.md: Copied (replaced existing)"),
            Some(Applied::Copied("changeme.md".to_string()))
        );
    }

    #[test]
    fn classify_applied_deleted() {
        assert_eq!(
            classify_applied("deleteme.md: Deleted"),
            Some(Applied::Deleted("deleteme.md".to_string()))
        );
    }

    #[test]
    fn classify_applied_progress_stats_line() {
        let line = "32 B / 32 B, 100%, 0 B/s, ETA -";
        assert_eq!(
            classify_applied(line),
            Some(Applied::Progress(line.to_string()))
        );
    }

    #[test]
    fn classify_applied_done() {
        assert_eq!(classify_applied("Bisync successful"), Some(Applied::Done));
    }

    #[test]
    fn classify_applied_abort_max_delete() {
        assert_eq!(
            classify_applied("Safety abort: too many deletes (>50%, 1 of 1)..."),
            Some(Applied::AbortMaxDelete)
        );
    }

    #[test]
    fn classify_applied_abort_prior_listing() {
        assert_eq!(
            classify_applied("Bisync critical error: cannot find prior Path1 or Path2 listings"),
            Some(Applied::AbortPriorListing)
        );
        assert_eq!(
            classify_applied("Bisync aborted. Must run --resync to recover."),
            Some(Applied::AbortPriorListing)
        );
    }

    #[test]
    fn classify_applied_noise_is_none() {
        assert_eq!(classify_applied("Some unrelated log line"), None);
    }

    #[test]
    fn render_applied_copied_plain() {
        let t = crate::theme::Theme::dark(false);
        assert_eq!(
            render_applied(&Applied::Copied("notes/x.md".to_string()), t),
            Some("  ✓ notes/x.md".to_string())
        );
    }

    #[test]
    fn render_applied_deleted_plain() {
        let t = crate::theme::Theme::dark(false);
        assert_eq!(
            render_applied(&Applied::Deleted("notes/x.md".to_string()), t),
            Some("  ✗ notes/x.md (deleted)".to_string())
        );
    }

    #[test]
    fn render_applied_progress_plain() {
        let t = crate::theme::Theme::dark(false);
        let line = "32 B / 32 B, 100%, 0 B/s, ETA -";
        assert_eq!(
            render_applied(&Applied::Progress(line.to_string()), t),
            Some(format!("  {line}"))
        );
    }

    #[test]
    fn render_applied_done_and_aborts_are_none() {
        let t = crate::theme::Theme::dark(false);
        assert_eq!(render_applied(&Applied::Done, t), None);
        assert_eq!(render_applied(&Applied::AbortMaxDelete, t), None);
        assert_eq!(render_applied(&Applied::AbortPriorListing, t), None);
    }

    #[test]
    fn render_applied_copied_colored_contains_success_ansi() {
        let t = crate::theme::Theme::dark(true);
        let rendered = render_applied(&Applied::Copied("notes/x.md".to_string()), t).unwrap();
        assert!(
            rendered.contains("\x1b[92m"),
            "expected green success ANSI in {rendered:?}"
        );
    }

    #[test]
    fn classify_change_path1_file_changed() {
        assert_eq!(
            classify_change(
                "- Path1    File changed: size (larger), time (newer) - resources/r1.md"
            ),
            Some(Change {
                side: Side::Push,
                kind: ChangeKind::Changed,
                path: "resources/r1.md".to_string()
            })
        );
    }

    #[test]
    fn classify_change_path1_file_is_new() {
        assert_eq!(
            classify_change("- Path1    File is new               - notes/n3.md"),
            Some(Change {
                side: Side::Push,
                kind: ChangeKind::New,
                path: "notes/n3.md".to_string()
            })
        );
    }

    #[test]
    fn classify_change_path1_file_was_deleted() {
        assert_eq!(
            classify_change("- Path1    File was deleted          - deleteme.md"),
            Some(Change {
                side: Side::Push,
                kind: ChangeKind::Deleted,
                path: "deleteme.md".to_string()
            })
        );
    }

    #[test]
    fn classify_change_path2_file_is_new() {
        assert_eq!(
            classify_change("- Path2    File is new               - remote-added.md"),
            Some(Change {
                side: Side::Pull,
                kind: ChangeKind::New,
                path: "remote-added.md".to_string()
            })
        );
    }

    #[test]
    fn classify_change_path2_file_was_deleted() {
        assert_eq!(
            classify_change("- Path2    File was deleted          - top.md"),
            Some(Change {
                side: Side::Pull,
                kind: ChangeKind::Deleted,
                path: "top.md".to_string()
            })
        );
    }

    #[test]
    fn classify_change_queue_line_is_none() {
        assert_eq!(classify_change("Queue copy to Path2: notes/n3.md"), None);
        assert_eq!(
            classify_change("- Path1    Queue copy to Path2                - notes/n3.md"),
            None
        );
    }

    #[test]
    fn summarize_groups_by_top_level_dir_count_desc() {
        let paths = vec![
            "notes/n3.md".to_string(),
            "notes/n4.md".to_string(),
            "resources/r1.md".to_string(),
        ];
        assert_eq!(
            summarize(&paths),
            vec![
                "2 changes in notes/".to_string(),
                "1 change in resources/".to_string()
            ]
        );
    }

    #[test]
    fn summarize_top_level_file_named_directly() {
        assert_eq!(
            summarize(&["top.md".to_string()]),
            vec!["1 change to top.md".to_string()]
        );
    }

    #[test]
    fn summarize_mixes_dirs_and_files_sorted_by_count_then_name() {
        let paths = vec![
            "a/x.md".to_string(),
            "a/y.md".to_string(),
            "a/z.md".to_string(),
            "b/one.md".to_string(),
            "solo.md".to_string(),
        ];
        assert_eq!(
            summarize(&paths),
            vec![
                "3 changes in a/".to_string(),
                "1 change in b/".to_string(),
                "1 change to solo.md".to_string()
            ]
        );
    }
}

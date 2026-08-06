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
mod tests;

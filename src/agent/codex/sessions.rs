//! Deciding whether a Codex session is still resumable.
//!
//! Codex records every session as a rollout file under
//! `<sessions>/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`, and `codex resume
//! <uuid>` reopens one. Brain therefore validates a stored id the same way the
//! Claude adapter validates a transcript: the session is resumable when its
//! rollout is on disk. Nothing else can answer the question — Codex exposes no
//! machine-readable session listing.
//!
//! The filename UUID is the authority. Across 400 real rollouts it equalled the
//! `session_meta` payload's `id` every time, and for top-level sessions — the
//! only kind Brain registers, since the session-start bridge ignores payloads
//! carrying a parent — that `id` also equalled `session_id`. So matching the
//! filename resolves the id Brain stored regardless of which field Codex
//! reported it under.

use std::path::{Path, PathBuf};

/// Day directories are searched newest-first, so a live session is found in the
/// first directory examined instead of after walking years of history.
const ROLLOUT_PREFIX: &str = "rollout-";
const ROLLOUT_SUFFIX: &str = ".jsonl";

/// Whether one rollout filename belongs to this session id. Pure.
///
/// The id must occupy the whole trailing segment: a prefix match would let
/// `019f-abc` claim `…-019f-abcdef.jsonl`, resuming a stranger's session.
#[must_use]
pub(super) fn rollout_matches(file_name: &str, session_id: &str) -> bool {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return false;
    }
    let Some(rest) = file_name.strip_prefix(ROLLOUT_PREFIX) else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(ROLLOUT_SUFFIX) else {
        return false;
    };
    stem.strip_suffix(session_id)
        .is_some_and(|before| before.ends_with('-'))
}

/// The rollout file for this session id, searching newest day first.
#[must_use]
pub(super) fn find_rollout(sessions_root: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.trim().is_empty() {
        return None;
    }
    find_in_directory(sessions_root, session_id, 3)
}

/// Walk `<year>/<month>/<day>` newest-first, recursing at most `depth` levels.
///
/// Depth is bounded rather than unbounded so an unexpected directory deep in the
/// tree cannot turn a resume check into a full-disk scan.
fn find_in_directory(directory: &Path, session_id: &str, depth: u8) -> Option<PathBuf> {
    let mut directories = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(directory).ok()?.flatten().collect();
    // Reverse lexical order is reverse chronological for zero-padded date parts.
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries.into_iter().rev() {
        let path = entry.path();
        if path.is_dir() {
            directories.push(path);
            continue;
        }
        let name = entry.file_name();
        if rollout_matches(&name.to_string_lossy(), session_id) {
            return Some(path);
        }
    }
    if depth == 0 {
        return None;
    }
    directories
        .into_iter()
        .find_map(|child| find_in_directory(&child, session_id, depth - 1))
}

#[cfg(test)]
mod tests;

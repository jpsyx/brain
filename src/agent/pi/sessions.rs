//! Read-only discovery of pi's on-disk sessions.
//!
//! pi writes one JSONL file per session under a directory derived from the
//! working directory, and writes it lazily: a session that never took a turn
//! leaves no file at all. That makes file presence exactly the evidence Brain
//! needs before offering a session for resume.

use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// Overrides pi reads before falling back to `<home>/.pi/agent/sessions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct SessionDirOverrides<'a> {
    /// `PI_CODING_AGENT_DIR`: relocates pi's whole configuration directory.
    pub(super) agent_dir: Option<&'a str>,
    /// `PI_CODING_AGENT_SESSION_DIR`: replaces the per-directory tree with one
    /// flat directory holding every session.
    pub(super) session_dir: Option<&'a str>,
}

/// Owned form of [`SessionDirOverrides`], read once from the environment the
/// Brain process inherited, which is the environment its pi children run in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ProcessSessionDirOverrides {
    agent_dir: Option<String>,
    session_dir: Option<String>,
}

impl ProcessSessionDirOverrides {
    pub(super) fn from_environment() -> Self {
        #[cfg(test)]
        if let Some(session_dir) = TEST_SESSION_DIR.with(|current| current.borrow().clone()) {
            return Self::for_session_dir(Some(session_dir));
        }
        Self {
            agent_dir: std::env::var("PI_CODING_AGENT_DIR").ok(),
            session_dir: std::env::var("PI_CODING_AGENT_SESSION_DIR").ok(),
        }
    }

    /// Discovery pinned to one directory, for tests.
    #[cfg(test)]
    pub(super) fn for_session_dir(session_dir: Option<std::path::PathBuf>) -> Self {
        Self {
            agent_dir: None,
            session_dir: session_dir.map(|dir| dir.display().to_string()),
        }
    }

    pub(super) fn as_overrides(&self) -> SessionDirOverrides<'_> {
        SessionDirOverrides {
            agent_dir: self.agent_dir.as_deref(),
            session_dir: self.session_dir.as_deref(),
        }
    }
}

/// pi's directory name for one working directory: the path with its leading
/// separator dropped and every separator or drive colon replaced by `-`,
/// wrapped in `--`.
#[must_use]
pub(super) fn encoded_directory_name(cwd: &Path) -> String {
    let encoded = cwd
        .to_string_lossy()
        .trim_start_matches(['/', '\\'])
        .replace(['/', '\\', ':'], "-");
    format!("--{encoded}--")
}

/// Where pi keeps this working directory's sessions, or `None` when no home
/// directory resolves and no override names one.
#[must_use]
pub(super) fn session_directory(
    cwd: &Path,
    home: Option<&Path>,
    overrides: SessionDirOverrides<'_>,
) -> Option<PathBuf> {
    // An explicit session directory is flat: pi stores every session in it and
    // filters by the working directory recorded in each file's header.
    if let Some(session_dir) = overrides.session_dir.map(str::trim).filter(|raw| !raw.is_empty()) {
        return Some(expand(session_dir, home));
    }
    let agent_dir = match overrides.agent_dir.map(str::trim).filter(|raw| !raw.is_empty()) {
        Some(raw) => expand(raw, home),
        None => home?.join(".pi").join("agent"),
    };
    Some(
        agent_dir
            .join("sessions")
            .join(encoded_directory_name(cwd)),
    )
}

/// Whether one session file name belongs to exactly this session id.
///
/// pi names a session `<timestamp>_<session id>.jsonl`, so a suffix match on
/// the separator keeps a longer id from matching a shorter one.
#[must_use]
pub(super) fn session_file_matches(file_name: &str, session_id: &str) -> bool {
    file_name
        .strip_suffix(".jsonl")
        .and_then(|stem| stem.strip_suffix(session_id))
        .is_some_and(|prefix| prefix.ends_with('_'))
}

/// Whether pi still holds a session file for `session_id` in `directory`.
#[must_use]
pub(super) fn session_exists(directory: &Path, session_id: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| session_file_matches(name, session_id))
    })
}

fn expand(raw: &str, home: Option<&Path>) -> PathBuf {
    home.map_or_else(
        || PathBuf::from(raw),
        |home| crate::paths::expand_tilde_with_home(raw, home),
    )
}

#[cfg(test)]
thread_local! {
    static TEST_SESSION_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Point every pi adapter built on this thread at one session directory.
#[cfg(test)]
pub(crate) fn override_sessions_dir_for_test(session_dir: &Path) -> impl Drop {
    let previous =
        TEST_SESSION_DIR.with(|current| current.replace(Some(session_dir.to_path_buf())));
    TestSessionDirOverride { previous }
}

#[cfg(test)]
struct TestSessionDirOverride {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl Drop for TestSessionDirOverride {
    fn drop(&mut self) {
        TEST_SESSION_DIR.with(|current| {
            current.replace(self.previous.take());
        });
    }
}

/// The file name pi would give a session recorded at `timestamp`.
#[cfg(test)]
#[must_use]
pub(crate) fn session_file_name(timestamp: &str, session_id: &str) -> String {
    format!("{timestamp}_{session_id}.jsonl")
}

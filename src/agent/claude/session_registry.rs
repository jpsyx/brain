//! Claude's live-session registry (`~/.claude/sessions/<pid>.json`).
//!
//! Claude writes one entry per running process naming the session that process
//! owns. `--resume` refuses a session another live process holds ("currently
//! running as a background agent"), so the registry is the second half of
//! Claude's resume evidence: the transcript says the conversation exists, the
//! registry says whether anyone else is still in it.

use std::path::Path;

use crate::state::PidAlive;

/// One process's claim on a Claude session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionClaim {
    pub pid: i32,
    pub session_id: String,
}

/// Whether a *live* process already owns `session`. A claim whose process has
/// exited is a leftover file, not a hold.
#[must_use]
pub(crate) fn session_is_held_by_live_process(
    claims: &[SessionClaim],
    session: &str,
    pid_alive: PidAlive,
) -> bool {
    claims
        .iter()
        .any(|claim| claim.session_id == session && pid_alive(claim.pid))
}

/// Every claim the registry directory currently records. A missing directory,
/// an unreadable file, or an entry in a shape we don't recognize contributes no
/// claim: absent evidence must never make a resumable session look held.
pub(crate) fn read_session_claims(directory: &Path) -> Vec<SessionClaim> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| parse_claim(&std::fs::read_to_string(path).ok()?))
        .collect()
}

fn parse_claim(contents: &str) -> Option<SessionClaim> {
    let entry: serde_json::Value = serde_json::from_str(contents).ok()?;
    Some(SessionClaim {
        pid: i32::try_from(entry.get("pid")?.as_i64()?).ok()?,
        session_id: entry.get("sessionId")?.as_str()?.to_owned(),
    })
}

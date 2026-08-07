//! Pure builders for launching the brain panel's agent child.
//!
//! brain decides *what* session to run (resume a prior one, or start a
//! fresh one with a chosen id) and *how* to invoke the agent. The actual
//! spawning + DB locking lives in `tui`; everything here is pure so it can
//! be unit-tested without a PTY, a DB, or a real agent CLI.

use std::path::Path;

pub use crate::agent::AgentKind;

/// What the brain panel should launch this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Resume an existing frontend session by ID.
    Resume(String),
    /// Start a new frontend session with a brain-chosen ID.
    Fresh(String),
}

impl Plan {
    /// Decide the launch plan: resume the most-recently-active free session
    /// when one exists, otherwise start a fresh session with `new_id`.
    #[must_use]
    pub fn decide(resume_candidate: Option<String>, new_id: String) -> Self {
        resume_candidate.map_or(Self::Fresh(new_id), Self::Resume)
    }
}

/// Single-quote a string for safe inclusion in a `sh -c` command line.
#[must_use]
pub fn shell_quote(s: &str) -> String {
    crate::agent::frontend::shell_quote(s)
}

/// Build the shell command handed to the PTY.
///
/// We cd into the brain root so the child resolves project-local settings and
/// paths. `llm_cmd` is the user-configurable launch command, spliced in
/// verbatim so it may carry its own flags; brain appends the frontend-specific
/// resume arguments after it. brain never relies on a shell alias for this.
///
/// When `prompt` is `Some(non-empty)`, it's appended as a single quoted
/// argument; the agent submits it on launch and stays interactive, so the
/// conversation is already seeded with the request (used by the tasks view's
/// Defer / Start / Remove / agenda / triage / message-about-task actions).
/// The brain-search view always passes `None` — its panel is typed into by
/// hand.
///
/// # Errors
///
/// Returns [`crate::agent::AgentError::EmptySessionId`] when the compatibility
/// plan contains a blank session ID.
pub fn build_llm_command(
    brain_root: &Path,
    agent_kind: AgentKind,
    llm_cmd: &str,
    plan: &Plan,
    prompt: Option<&str>,
) -> Result<String, crate::agent::AgentError> {
    let session_plan = match plan {
        Plan::Resume(id) => crate::agent::SessionPlan::resume(crate::agent::AgentSession::new(id)?),
        Plan::Fresh(id) => crate::agent::SessionPlan::fresh(crate::agent::AgentSession::new(id)?),
    };
    let command = crate::agent::build_command(agent_kind, llm_cmd, &session_plan, prompt);
    Ok(format!(
        "cd {} && {command}",
        shell_quote(&brain_root.display().to_string())
    ))
}

/// Claude's project-dir name for a working directory.
///
/// Claude stores a session transcript at
/// `~/.claude/projects/<project-dir-name>/<session-id>.jsonl`, where the
/// project-dir name is the cwd with `/` and `.` replaced by `-`. brain
/// always runs claude in `<brain_root>`, so this names that directory (e.g.
/// `/Users/x/brain` → `-Users-x-brain`). Used to check a session actually
/// persisted a transcript before we hand its id to `claude --resume`.
#[must_use]
pub fn project_dir_name(brain_root: &Path) -> String {
    crate::agent::claude_project_dir_name(brain_root)
}

/// Selected workspace/actor identity plus session vars injected into the agent.
///
/// The SessionStart and Stop hooks use the matching UUID-scoped DB/response
/// paths, including after `/new` / `/clear` re-sessions.
#[must_use]
pub fn env_for(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &crate::actor::ActorContext,
    agent_kind: AgentKind,
    instance: &str,
    pid: i32,
    db_path: &Path,
    response_id: &str,
) -> Vec<(String, String)> {
    let mut env = workspace
        .integration_env(actor)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Vec<_>>();
    env.extend([
        (
            "BRAIN_AGENT_KIND".to_owned(),
            agent_kind.as_str().to_owned(),
        ),
        ("BRAIN_INSTANCE_ID".to_owned(), instance.to_owned()),
        ("BRAIN_PID".to_owned(), pid.to_string()),
        ("BRAIN_STATE_DB".to_owned(), db_path.display().to_string()),
        ("BRAIN_RESPONSE_ID".to_owned(), response_id.to_owned()),
        (
            "BRAIN_RESPONSE_DIR".to_owned(),
            workspace.paths().responses_dir().display().to_string(),
        ),
    ]);
    env
}

/// Env vars injected into the *ephemeral* daily-triage session.
///
/// Unlike [`env_for`], this deliberately **omits** `BRAIN_INSTANCE_ID` and
/// `BRAIN_STATE_DB`. The SessionStart hook no-ops without them, so the triage
/// session is never recorded in the session DB and can never be resumed — it is
/// ephemeral by construction (if the shell closes mid-triage the session is
/// simply lost, and the daily-triage nudge fires again next launch). In their
/// place it carries the completion channel the `/triage` skill reports through:
/// the brain-server done URL and the one-time token brain matches when the
/// signal comes back (see [`crate::triage_signal`]).
#[must_use]
pub fn env_for_triage(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &crate::actor::ActorContext,
    agent_kind: AgentKind,
    done_url: &str,
    token: &str,
) -> Vec<(String, String)> {
    let mut env = workspace
        .integration_env(actor)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Vec<_>>();
    env.extend([
        (
            "BRAIN_AGENT_KIND".to_owned(),
            agent_kind.as_str().to_owned(),
        ),
        ("BRAIN_TRIAGE_DONE_URL".to_owned(), done_url.to_owned()),
        ("BRAIN_TRIAGE_TOKEN".to_owned(), token.to_owned()),
    ]);
    env
}

#[cfg(test)]
mod tests;

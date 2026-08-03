//! Pure builders for launching the brain panel's agent child.
//!
//! brain decides *what* session to run (resume a prior one, or start a
//! fresh one with a chosen id) and *how* to invoke the agent. The actual
//! spawning + DB locking lives in `tui`; everything here is pure so it can
//! be unit-tested without a PTY, a DB, or a real agent CLI.

use std::path::Path;

/// Which agent frontend the brain panel is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    /// Claude Code.
    Claude,
    /// OpenAI Codex.
    Codex,
}

impl AgentKind {
    /// Human label for UI copy.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// Stable state-database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

/// What the brain panel should launch this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Resume an existing Claude session by id (`claude --resume <id>`).
    Resume(String),
    /// Start a new Claude session with a brain-chosen id
    /// (`claude --session-id <uuid>`).
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
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
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
#[must_use]
pub fn build_llm_command(
    brain_root: &Path,
    agent_kind: AgentKind,
    llm_cmd: &str,
    plan: &Plan,
    prompt: Option<&str>,
) -> String {
    let mut parts = vec![
        "cd".to_owned(),
        shell_quote(&brain_root.display().to_string()),
        "&&".to_owned(),
        llm_cmd.trim().to_owned(),
    ];
    match (agent_kind, plan) {
        (AgentKind::Claude, Plan::Resume(id)) => {
            parts.push("--resume".to_owned());
            parts.push(shell_quote(id));
        }
        (AgentKind::Claude, Plan::Fresh(id)) => {
            parts.push("--session-id".to_owned());
            parts.push(shell_quote(id));
        }
        (AgentKind::Codex, Plan::Resume(id)) => {
            parts.push("resume".to_owned());
            parts.push(shell_quote(id));
        }
        (AgentKind::Codex, Plan::Fresh(_)) => {}
    }
    if let Some(p) = prompt {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            parts.push(shell_quote(trimmed));
        }
    }
    parts.join(" ")
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
    brain_root.to_string_lossy().replace(['/', '.'], "-")
}

/// Selected workspace/actor identity plus session vars injected into Claude.
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
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace() -> crate::workspace::WorkspaceContext {
        crate::workspace::WorkspaceContext::new(
            std::path::Path::new("/home/tester"),
            crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
                .expect("valid id"),
            crate::workspace::WorkspaceName::parse("family").expect("valid name"),
            std::path::Path::new("/home/tester/family"),
            "pablo",
            std::path::Path::new("/home/tester"),
        )
        .expect("context")
    }

    fn actor() -> crate::actor::ActorContext {
        let users = crate::users::Users {
            schema_version: crate::users::USERS_SCHEMA_VERSION,
            users: vec![crate::users::User {
                id: crate::users::UserId::parse("pablo").unwrap(),
                name: "Pablo".to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            }],
        };
        crate::actor::resolve_actor(
            &crate::users::UserId::parse("pablo").unwrap(),
            crate::actor::RequestIdentity::Local,
            &users,
        )
        .unwrap()
    }

    #[test]
    fn decide_resumes_when_a_candidate_exists() {
        let plan = Plan::decide(Some("abc-123".to_owned()), "new-id".to_owned());
        assert_eq!(plan, Plan::Resume("abc-123".to_owned()));
    }

    #[test]
    fn decide_starts_fresh_when_nothing_to_resume() {
        let plan = Plan::decide(None, "new-id".to_owned());
        assert_eq!(plan, Plan::Fresh("new-id".to_owned()));
    }

    #[test]
    fn fresh_command_uses_session_id_flag() {
        let cmd = build_llm_command(
            &PathBuf::from("/Users/x/brain"),
            AgentKind::Claude,
            "claude",
            &Plan::Fresh("uuid-1".to_owned()),
            None,
        );
        assert!(cmd.starts_with("cd '/Users/x/brain' && claude"));
        assert!(cmd.contains("--session-id 'uuid-1'"));
        assert!(!cmd.contains("--resume"));
    }

    #[test]
    fn resume_command_uses_resume_flag() {
        let cmd = build_llm_command(
            &PathBuf::from("/Users/x/brain"),
            AgentKind::Claude,
            "claude",
            &Plan::Resume("sess-9".to_owned()),
            None,
        );
        assert!(cmd.contains("--resume 'sess-9'"));
        assert!(!cmd.contains("--session-id"));
    }

    #[test]
    fn configured_command_is_spliced_in_before_brains_own_flags() {
        // The configured command may carry its own flags; brain's --resume must
        // come after them, and the command is not shell-quoted (the shell
        // interprets its flags).
        let cmd = build_llm_command(
            &PathBuf::from("/Users/x/brain"),
            AgentKind::Claude,
            "claude --dangerously-skip-permissions",
            &Plan::Resume("sess-9".to_owned()),
            None,
        );
        assert_eq!(
            cmd,
            "cd '/Users/x/brain' && claude --dangerously-skip-permissions --resume 'sess-9'"
        );
    }

    #[test]
    fn prompt_is_appended_as_a_quoted_arg() {
        let cmd = build_llm_command(
            &PathBuf::from("/Users/x/brain"),
            AgentKind::Claude,
            "claude",
            &Plan::Fresh("uuid-1".to_owned()),
            Some("Defer T123 by 7 days"),
        );
        assert!(cmd.ends_with("'Defer T123 by 7 days'"));
    }

    #[test]
    fn empty_prompt_adds_no_trailing_arg() {
        let cmd = build_llm_command(
            &PathBuf::from("/Users/x/brain"),
            AgentKind::Claude,
            "claude",
            &Plan::Resume("sess-9".to_owned()),
            Some("   "),
        );
        assert!(cmd.ends_with("--resume 'sess-9'"));
        assert!(!cmd.contains("''"));
    }

    #[test]
    fn prompt_with_a_single_quote_is_escaped() {
        let cmd = build_llm_command(
            &PathBuf::from("/Users/x/brain"),
            AgentKind::Claude,
            "claude",
            &Plan::Fresh("u".to_owned()),
            Some("don't break"),
        );
        assert!(cmd.contains(r"'don'\''t break'"));
    }

    #[test]
    fn codex_resume_uses_resume_subcommand() {
        let cmd = build_llm_command(
            &PathBuf::from("/Users/x/brain"),
            AgentKind::Codex,
            "codex",
            &Plan::Resume("sess-9".to_owned()),
            None,
        );
        assert_eq!(cmd, "cd '/Users/x/brain' && codex resume 'sess-9'");
    }

    #[test]
    fn codex_fresh_uses_configured_base_command_without_claude_flags() {
        let cmd = build_llm_command(
            &PathBuf::from("/Users/x/brain"),
            AgentKind::Codex,
            "codex --model gpt-5",
            &Plan::Fresh("uuid-1".to_owned()),
            Some("Start here"),
        );
        assert_eq!(
            cmd,
            "cd '/Users/x/brain' && codex --model gpt-5 'Start here'"
        );
        assert!(!cmd.contains("--session-id"));
        assert!(!cmd.contains("--resume"));
    }

    #[test]
    fn project_dir_name_mangles_slashes_to_dashes() {
        assert_eq!(
            project_dir_name(&PathBuf::from("/Users/x/brain")),
            "-Users-x-brain"
        );
        // Dots are mangled too (claude's convention).
        assert_eq!(
            project_dir_name(&PathBuf::from("/Users/x/.brain")),
            "-Users-x--brain"
        );
    }

    #[test]
    fn env_carries_instance_pid_and_db_path() {
        let env = env_for(
            &workspace(),
            &actor(),
            AgentKind::Claude,
            "inst-1",
            4321,
            &PathBuf::from("/tmp/state.db"),
            "response-1",
        );
        assert!(env.contains(&("BRAIN_INSTANCE_ID".to_owned(), "inst-1".to_owned())));
        assert!(env.contains(&("BRAIN_PID".to_owned(), "4321".to_owned())));
        assert!(env.contains(&("BRAIN_STATE_DB".to_owned(), "/tmp/state.db".to_owned())));
        assert!(env.contains(&("BRAIN_ACTOR_ID".to_owned(), "pablo".to_owned())));
        assert!(env.contains(&("BRAIN_CHANNEL".to_owned(), "interactive".to_owned())));
        assert!(env.contains(&("BRAIN_AGENT_KIND".to_owned(), "claude".to_owned())));
        assert!(env.contains(&("BRAIN_RESPONSE_ID".to_owned(), "response-1".to_owned())));
    }

    #[test]
    fn triage_env_carries_done_url_and_token() {
        let env = env_for_triage(
            &workspace(),
            &actor(),
            AgentKind::Claude,
            "http://127.0.0.1:8787/triage/done",
            "tok-9",
        );
        assert!(env.contains(&(
            "BRAIN_TRIAGE_DONE_URL".to_owned(),
            "http://127.0.0.1:8787/triage/done".to_owned()
        )));
        assert!(env.contains(&("BRAIN_TRIAGE_TOKEN".to_owned(), "tok-9".to_owned())));
    }

    #[test]
    fn triage_env_omits_the_tracking_vars_so_the_session_stays_ephemeral() {
        // The SessionStart hook keys off BRAIN_INSTANCE_ID / BRAIN_STATE_DB;
        // their absence is exactly what keeps the triage session out of the DB.
        let env = env_for_triage(
            &workspace(),
            &actor(),
            AgentKind::Claude,
            "http://127.0.0.1:8787/triage/done",
            "tok-9",
        );
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(!keys.contains(&"BRAIN_INSTANCE_ID"));
        assert!(!keys.contains(&"BRAIN_STATE_DB"));
    }
}

//! Pure builders for launching the brain panel's `claude` child.
//!
//! brain decides *what* session to run (resume a prior one, or start a
//! fresh one with a chosen id) and *how* to invoke claude. The actual
//! spawning + DB locking lives in `tui`; everything here is pure so it can
//! be unit-tested without a PTY, a DB, or a real claude.

use std::path::Path;

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

/// Build the shell command handed to the PTY:
/// `cd <root> && claude --resume <id> [<prompt>]` (resume) or
/// `cd <root> && claude --session-id <id> [<prompt>]` (fresh).
///
/// We cd into the brain root so claude resolves that directory's
/// `.claude/settings.json` (where the SessionStart hook is wired), and we
/// invoke `claude` directly rather than the `cl` alias so we control the
/// `--resume` / `--session-id` flag.
///
/// When `prompt` is `Some(non-empty)`, it's appended as a single quoted
/// argument; claude submits it on launch and stays interactive, so the
/// conversation is already seeded with the request (used by the tasks view's
/// Defer / Start / Remove / agenda / triage / message-about-task actions).
/// The brain-search view always passes `None` — its panel is typed into by
/// hand.
#[must_use]
pub fn build_claude_command(brain_root: &Path, plan: &Plan, prompt: Option<&str>) -> String {
    let mut parts = vec![
        "cd".to_owned(),
        shell_quote(&brain_root.display().to_string()),
        "&&".to_owned(),
        "claude".to_owned(),
    ];
    match plan {
        Plan::Resume(id) => {
            parts.push("--resume".to_owned());
            parts.push(shell_quote(id));
        }
        Plan::Fresh(id) => {
            parts.push("--session-id".to_owned());
            parts.push(shell_quote(id));
        }
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

/// Env vars injected into the claude child so the SessionStart hook can
/// attribute the session to this brain shell (and update the DB on
/// `/new` / `/clear` re-sessions).
#[must_use]
pub fn env_for(instance: &str, pid: i32, db_path: &Path) -> Vec<(String, String)> {
    vec![
        ("BRAIN_INSTANCE_ID".to_owned(), instance.to_owned()),
        ("BRAIN_PID".to_owned(), pid.to_string()),
        (
            "BRAIN_STATE_DB".to_owned(),
            db_path.display().to_string(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
        let cmd = build_claude_command(
            &PathBuf::from("/Users/x/brain"),
            &Plan::Fresh("uuid-1".to_owned()),
            None,
        );
        assert!(cmd.starts_with("cd '/Users/x/brain' && claude"));
        assert!(cmd.contains("--session-id 'uuid-1'"));
        assert!(!cmd.contains("--resume"));
    }

    #[test]
    fn resume_command_uses_resume_flag() {
        let cmd = build_claude_command(
            &PathBuf::from("/Users/x/brain"),
            &Plan::Resume("sess-9".to_owned()),
            None,
        );
        assert!(cmd.contains("--resume 'sess-9'"));
        assert!(!cmd.contains("--session-id"));
    }

    #[test]
    fn prompt_is_appended_as_a_quoted_arg() {
        let cmd = build_claude_command(
            &PathBuf::from("/Users/x/brain"),
            &Plan::Fresh("uuid-1".to_owned()),
            Some("Defer T123 by 7 days"),
        );
        assert!(cmd.ends_with("'Defer T123 by 7 days'"));
    }

    #[test]
    fn empty_prompt_adds_no_trailing_arg() {
        let cmd = build_claude_command(
            &PathBuf::from("/Users/x/brain"),
            &Plan::Resume("sess-9".to_owned()),
            Some("   "),
        );
        assert!(cmd.ends_with("--resume 'sess-9'"));
        assert!(!cmd.contains("''"));
    }

    #[test]
    fn prompt_with_a_single_quote_is_escaped() {
        let cmd = build_claude_command(
            &PathBuf::from("/Users/x/brain"),
            &Plan::Fresh("u".to_owned()),
            Some("don't break"),
        );
        assert!(cmd.contains(r"'don'\''t break'"));
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
        let env = env_for("inst-1", 4321, &PathBuf::from("/tmp/state.db"));
        assert!(env.contains(&("BRAIN_INSTANCE_ID".to_owned(), "inst-1".to_owned())));
        assert!(env.contains(&("BRAIN_PID".to_owned(), "4321".to_owned())));
        assert!(env.contains(&("BRAIN_STATE_DB".to_owned(), "/tmp/state.db".to_owned())));
    }
}

//! Claude's resume-eligibility evidence: a transcript on disk is necessary but
//! not sufficient. Claude refuses `--resume` for a session another live process
//! still owns (a background agent, or a second attached CLI), so brain must
//! read that claim as "not resumable" and fall through to the next candidate.

use std::path::PathBuf;

use crate::agent::{
    AgentFrontend, AgentSession, ClaudeFrontend,
    claude::{SessionClaim, session_is_held_by_live_process},
};

const fn always_alive(_pid: i32) -> bool {
    true
}

const fn never_alive(_pid: i32) -> bool {
    false
}

fn claim(pid: i32, session_id: &str) -> SessionClaim {
    SessionClaim {
        pid,
        session_id: session_id.to_owned(),
    }
}

/// Write a `~/.claude`-shaped tree: one transcript for `session`, plus the
/// registry entries that claim sessions for a running process.
fn claude_home(session: &str, claims: &[(i32, &str)]) -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("claude home");
    let project = home.path().join("projects").join("-workspaces-family brain");
    std::fs::create_dir_all(&project).expect("project dir");
    std::fs::write(project.join(format!("{session}.jsonl")), "{}\n").expect("transcript");
    let sessions = home.path().join("sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    for (pid, claimed) in claims {
        std::fs::write(
            sessions.join(format!("{pid}.json")),
            format!(r#"{{"pid":{pid},"sessionId":"{claimed}","kind":"bg"}}"#),
        )
        .expect("registry entry");
    }
    home
}

fn frontend(home: &tempfile::TempDir, pid_alive: crate::state::PidAlive) -> ClaudeFrontend {
    ClaudeFrontend::new(
        "claude",
        PathBuf::from("/workspaces/family brain"),
        home.path().join("projects"),
    )
    .with_pid_probe(pid_alive)
}

#[test]
fn a_session_is_held_only_when_its_own_claim_belongs_to_a_live_process() {
    let claims = [claim(94425, "held"), claim(11, "unrelated")];

    assert!(session_is_held_by_live_process(
        &claims,
        "held",
        always_alive
    ));
    assert!(
        !session_is_held_by_live_process(&claims, "held", never_alive),
        "a claim whose process has exited is stale, not a hold"
    );
    assert!(
        !session_is_held_by_live_process(&claims, "unclaimed", always_alive),
        "a live process holding some other session says nothing about this one"
    );
}

#[test]
fn claude_refuses_to_resume_a_session_a_live_background_agent_still_holds() {
    let home = claude_home("held", &[(94425, "held")]);

    assert_eq!(
        frontend(&home, always_alive)
            .resume_candidate_exists(&AgentSession::new("held").expect("held session")),
        Ok(false)
    );
}

#[test]
fn claude_resumes_a_session_whose_only_registry_claim_is_a_dead_process() {
    let home = claude_home("free", &[(94425, "free")]);

    assert_eq!(
        frontend(&home, never_alive)
            .resume_candidate_exists(&AgentSession::new("free").expect("free session")),
        Ok(true)
    );
}

#[test]
fn claude_resumes_a_session_no_registry_entry_claims() {
    let home = claude_home("free", &[(94425, "someone else")]);

    assert_eq!(
        frontend(&home, always_alive)
            .resume_candidate_exists(&AgentSession::new("free").expect("free session")),
        Ok(true)
    );
}

use std::path::Path;

use super::{PiFrontend, SkillSelection};
use crate::agent::{AgentFrontend, AgentSession, SessionPlan};

const ID: &str = "0199a1f4-1c0f-7a3b-9d21-6f0f9a0c4e11";

fn session() -> AgentSession {
    AgentSession::new(ID).expect("a nonempty session id")
}

fn tree_with_session() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory
            .path()
            .join(format!("2026-09-16T11-02-03-000Z_{ID}.jsonl")),
        b"{\"type\":\"session\"}\n",
    )
    .unwrap();
    directory
}

/// Both channels read the same evidence, so SMS and email can never disagree
/// with the interactive panel about whether a session can be picked back up.
#[test]
fn a_recorded_session_is_resumable_for_every_channel() {
    let directory = tree_with_session();
    let frontend = PiFrontend::new("pi").with_session_dir(Some(directory.path()));

    assert!(frontend.resume_candidate_exists(&session()).unwrap());
    assert!(frontend.can_resume_response_session(&session()).unwrap());
}

/// pi writes a session file lazily, so a session that never took a turn leaves
/// nothing behind and must not be offered for resume.
#[test]
fn a_session_pi_never_wrote_is_resumable_for_no_channel() {
    let directory = tempfile::tempdir().unwrap();
    let frontend = PiFrontend::new("pi").with_session_dir(Some(directory.path()));

    assert!(!frontend.resume_candidate_exists(&session()).unwrap());
    assert!(!frontend.can_resume_response_session(&session()).unwrap());
}

/// `--session-id` opens an existing session and creates a missing one, so both
/// plans emit the same flag and Brain's chosen id stays authoritative.
#[test]
fn both_plans_name_the_brain_chosen_session_id() {
    let fresh = PiFrontend::command_for(
        "pi",
        &SessionPlan::fresh(AgentSession::new("fresh-1").unwrap()),
        None,
    );
    let resumed = PiFrontend::command_for(
        "pi",
        &SessionPlan::resume(AgentSession::new("resume-1").unwrap()),
        None,
    );

    assert_eq!(fresh, "pi --no-approve --session-id 'fresh-1'");
    assert_eq!(resumed, "pi --no-approve --session-id 'resume-1'");
}

/// A restricted plan switches pi's own discovery off and names exactly the
/// rendered selection; an unrestricted one adds Brain's workspace skills to
/// whatever pi would otherwise find.
#[test]
fn skill_selection_is_exact_only_when_brain_restricts_it() {
    let exact = PiFrontend::command_for_with_capabilities(
        "pi",
        &SessionPlan::fresh(AgentSession::new("fresh-1").unwrap()),
        None,
        Some("stay inside the workspace"),
        &SkillSelection::Exactly(Path::new("/cache/selected skills")),
        Some(Path::new("/root/.brain/hooks/pi_brain_extension.ts")),
    );
    let additive = PiFrontend::command_for_with_capabilities(
        "pi",
        &SessionPlan::fresh(AgentSession::new("fresh-1").unwrap()),
        None,
        None,
        &SkillSelection::WorkspaceAndDiscovered("/root/.agents/skills".into()),
        None,
    );

    assert_eq!(
        exact,
        "pi --no-approve --no-skills --skill '/cache/selected skills' \
         --append-system-prompt 'stay inside the workspace' \
         --extension '/root/.brain/hooks/pi_brain_extension.ts' --session-id 'fresh-1'"
    );
    assert_eq!(
        additive,
        "pi --no-approve --skill '/root/.agents/skills' --session-id 'fresh-1'"
    );
}

/// pi has no MCP mechanism, and its skill selection is exact only when Brain
/// appends the flags itself.
#[test]
fn capability_evidence_never_claims_an_mcp_mechanism_pi_does_not_have() {
    assert_eq!(
        PiFrontend::capability_evidence("pi"),
        crate::access::EnforcementEvidence::strict_skills_without_mcp_support()
    );
    assert_eq!(
        PiFrontend::capability_evidence("sh -c 'exec pi'"),
        crate::access::EnforcementEvidence::without_mcp_support()
    );
}
use std::{path::Path, sync::Arc};

use crate::{
    actor::{ActorContext, RequestIdentity},
    agent::{
        AgentAction, AgentController, AgentError, AgentFrontend, AgentKind, AgentSession,
        AgentTransport, CompletionStrategy, HookMetadata, InputSequence, LaunchRequest, LaunchSpec,
        SessionPlan,
    },
    server::receiver::{AttachmentRef, Channel, InboundJob},
    state::{
        Db, ReceiverConversation, ReceiverConversationIdentity, ReceiverJob, ReceiverSessionBinding,
    },
    users::{PhoneIdentity, USERS_SCHEMA_VERSION, User, UserId, Users},
    workspace::{WorkspaceContext, WorkspaceId, WorkspaceName},
};

use super::planning::{RECOVERY_PROMPT_BUDGET_BYTES, plan_receiver_launch};

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const CURRENT_PROMPT: &str = "Review the attached photo and remember its subject.";
const TASK_CAPTURE_POLICY: &str = "If the message asks to add, create, capture, remember, or track a task, create it in Brain's task system; do not perform the task now unless the sender explicitly asks you to.";
const RESUME_PROMPT: &str = concat!(
    "If the message asks to add, create, capture, remember, or track a task, ",
    "create it in Brain's task system; do not perform the task now unless the ",
    "sender explicitly asks you to.",
    "\n\n## Current authenticated message\n",
    "Review the attached photo and remember its subject.",
    "\n\nAttachment references:\n",
    "- source=\"https://attachments.example.test/photo\", ",
    "provider_id=\"media-1\", content_type=\"image/png\", filename=\"photo.png\"",
);

#[derive(Clone, Copy)]
enum ProbeOutcome {
    Exists,
    Missing,
    Error,
}

#[derive(Clone, Copy)]
enum ClaimOutcome {
    Claimed,
    Rejected,
    Error,
}

#[derive(Clone, Copy, Debug)]
enum BindingKind {
    Matching,
    OtherFrontend,
    Absent,
}

struct PlanningCase {
    name: &'static str,
    binding: BindingKind,
    probe: ProbeOutcome,
    claim: ClaimOutcome,
    transcript: &'static str,
    expects_resume: bool,
}

struct ProbeFrontend {
    kind: AgentKind,
    outcome: ProbeOutcome,
}

impl AgentFrontend for ProbeFrontend {
    fn kind(&self) -> AgentKind {
        self.kind
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        Ok(LaunchSpec::new(
            "receiver-test-agent",
            request.workspace().root().to_path_buf(),
            Vec::new(),
            HookMetadata::none(),
        ))
    }

    fn input_for(&self, _action: AgentAction<'_>) -> Result<InputSequence, AgentError> {
        Ok(InputSequence::bytes([]))
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn resume_candidate_exists(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        match self.outcome {
            ProbeOutcome::Exists => Ok(true),
            ProbeOutcome::Missing => Ok(false),
            ProbeOutcome::Error => Err(AgentError::Frontend("history probe failed".to_owned())),
        }
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        Ok(session.as_str().to_owned())
    }

    fn can_resume_response_session(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Ok(false)
    }
}

struct NullTransport;

impl AgentTransport for NullTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        Ok(())
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        false
    }

    fn shutdown(&mut self) {}
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse(WORKSPACE_ID).expect("workspace ID")
}

fn receiver_actor() -> ActorContext {
    let user_id = UserId::parse("test-user").expect("user ID");
    let users = Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![User {
            id: user_id.clone(),
            name: "Test user".to_owned(),
            phones: vec![PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: Vec::new(),
            response_email: None,
        }],
    };
    crate::actor::resolve_actor(
        &user_id,
        RequestIdentity::Sms {
            from: "+12125550100",
        },
        &users,
    )
    .expect("receiver actor")
}

fn controller(kind: AgentKind, probe: ProbeOutcome) -> AgentController {
    let workspace = WorkspaceContext::new(
        Path::new("/home/tester"),
        workspace_id(),
        WorkspaceName::parse("family").expect("workspace name"),
        Path::new("/workspaces/family"),
        "test-user",
        Path::new("/home/tester"),
    )
    .expect("workspace context");
    AgentController::new(
        Arc::new(workspace),
        receiver_actor(),
        Box::new(ProbeFrontend {
            kind,
            outcome: probe,
        }),
        Box::new(NullTransport),
    )
}

fn other_frontend(kind: AgentKind) -> AgentKind {
    match kind {
        AgentKind::Claude => AgentKind::Codex,
        AgentKind::Codex | AgentKind::OpenCode => AgentKind::Claude,
    }
}

fn durable_fixture(
    kind: AgentKind,
    binding_kind: BindingKind,
    transcript: &str,
) -> (ReceiverJob, ReceiverConversation) {
    durable_fixture_with_prompt(kind, binding_kind, transcript, CURRENT_PROMPT)
}

fn durable_fixture_with_prompt(
    kind: AgentKind,
    binding_kind: BindingKind,
    transcript: &str,
    prompt: &str,
) -> (ReceiverJob, ReceiverConversation) {
    durable_fixture_with_input(
        kind,
        binding_kind,
        transcript,
        prompt,
        vec![AttachmentRef {
            url: "https://attachments.example.test/photo".to_owned(),
            provider_id: Some("media-1".to_owned()),
            content_type: Some("image/png".to_owned()),
            filename: Some("photo.png".to_owned()),
        }],
    )
}

fn durable_fixture_with_input(
    kind: AgentKind,
    binding_kind: BindingKind,
    transcript: &str,
    prompt: &str,
    attachments: Vec<AttachmentRef>,
) -> (ReceiverJob, ReceiverConversation) {
    let db = Db::open_in_memory().expect("receiver state");
    let actor = receiver_actor();
    let inbound = InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: workspace_id(),
        actor,
        channel: Channel::Sms,
        authenticated_sender: "+12125550100".to_owned(),
        prompt: prompt.to_owned(),
        attachments,
        received_at_unix_ms: 100,
        provider_id: Some("provider-1".to_owned()),
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    };
    let identity = ReceiverConversationIdentity::sms(
        workspace_id(),
        UserId::parse("test-user").expect("user ID"),
    );
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job");
    let binding = match binding_kind {
        BindingKind::Matching => {
            Some(ReceiverSessionBinding::new(kind, "native-session").expect("session binding"))
        }
        BindingKind::OtherFrontend => Some(
            ReceiverSessionBinding::new(other_frontend(kind), "native-session")
                .expect("session binding"),
        ),
        BindingKind::Absent => None,
    };
    db.update_receiver_conversation(
        accepted.conversation_id(),
        transcript,
        binding.as_ref(),
        101,
    )
    .expect("update receiver conversation");
    let job = db
        .receiver_job(accepted.job_id())
        .expect("load durable receiver job")
        .expect("durable receiver job");
    let conversation = db
        .receiver_conversation(accepted.conversation_id())
        .expect("load receiver conversation")
        .expect("receiver conversation");
    (job, conversation)
}

fn fresh_session() -> AgentSession {
    AgentSession::new("fresh-session").expect("fresh session")
}

#[test]
fn receiver_launch_planning_is_conservative_for_every_frontend() {
    let cases = [
        PlanningCase {
            name: "matching resumable binding",
            binding: BindingKind::Matching,
            probe: ProbeOutcome::Exists,
            claim: ClaimOutcome::Claimed,
            transcript: "old portable context",
            expects_resume: true,
        },
        PlanningCase {
            name: "missing or invalid native history",
            binding: BindingKind::Matching,
            probe: ProbeOutcome::Missing,
            claim: ClaimOutcome::Claimed,
            transcript: "old portable context",
            expects_resume: false,
        },
        PlanningCase {
            name: "frontend change",
            binding: BindingKind::OtherFrontend,
            probe: ProbeOutcome::Exists,
            claim: ClaimOutcome::Claimed,
            transcript: "old portable context",
            expects_resume: false,
        },
        PlanningCase {
            name: "resumability probe error",
            binding: BindingKind::Matching,
            probe: ProbeOutcome::Error,
            claim: ClaimOutcome::Claimed,
            transcript: "old portable context",
            expects_resume: false,
        },
        PlanningCase {
            name: "matching session cannot be claimed",
            binding: BindingKind::Matching,
            probe: ProbeOutcome::Exists,
            claim: ClaimOutcome::Rejected,
            transcript: "old portable context",
            expects_resume: false,
        },
        PlanningCase {
            name: "session claim error",
            binding: BindingKind::Matching,
            probe: ProbeOutcome::Exists,
            claim: ClaimOutcome::Error,
            transcript: "old portable context",
            expects_resume: false,
        },
        PlanningCase {
            name: "empty transcript",
            binding: BindingKind::Absent,
            probe: ProbeOutcome::Exists,
            claim: ClaimOutcome::Claimed,
            transcript: "",
            expects_resume: false,
        },
    ];

    for kind in AgentKind::ALL {
        for case in &cases {
            let controller = controller(kind, case.probe);
            let (job, conversation) = durable_fixture(kind, case.binding, case.transcript);
            let plan =
                plan_receiver_launch(&controller, &job, &conversation, fresh_session(), |_| {
                    match case.claim {
                        ClaimOutcome::Claimed => Ok(true),
                        ClaimOutcome::Rejected => Ok(false),
                        ClaimOutcome::Error => anyhow::bail!("session claim failed"),
                    }
                });

            if case.expects_resume {
                assert_eq!(
                    plan.session_plan(),
                    &SessionPlan::resume(
                        AgentSession::new("native-session").expect("native session")
                    ),
                    "{} for {}",
                    case.name,
                    kind.label(),
                );
                assert_eq!(
                    plan.initial_prompt(),
                    RESUME_PROMPT,
                    "{} for {} must omit portable transcript context",
                    case.name,
                    kind.label(),
                );
            } else {
                assert_eq!(
                    plan.session_plan(),
                    &SessionPlan::fresh(fresh_session()),
                    "{} for {}",
                    case.name,
                    kind.label(),
                );
                assert!(
                    plan.initial_prompt()
                        .contains("## Current authenticated message"),
                    "{} for {} must use recovery prompt separation",
                    case.name,
                    kind.label(),
                );
                assert!(
                    plan.initial_prompt().contains(TASK_CAPTURE_POLICY),
                    "{} for {} must retain the shared task-capture policy",
                    case.name,
                    kind.label(),
                );
                assert!(
                    plan.initial_prompt().contains(CURRENT_PROMPT),
                    "{} for {} must retain the current job",
                    case.name,
                    kind.label(),
                );
            }
        }
    }
}

#[test]
fn receiver_launch_recovery_prompt_is_utf8_safe_bounded_and_keeps_newest_context() {
    let transcript = format!(
        "oldest-context\n{}\nnewest-context-📌",
        "older-é🙂\n".repeat(RECOVERY_PROMPT_BUDGET_BYTES)
    );

    for kind in AgentKind::ALL {
        let controller = controller(kind, ProbeOutcome::Missing);
        let (job, conversation) = durable_fixture(kind, BindingKind::Absent, &transcript);
        let plan = plan_receiver_launch(&controller, &job, &conversation, fresh_session(), |_| {
            Ok(true)
        });
        let prompt = plan.initial_prompt();

        assert!(
            prompt.len() <= RECOVERY_PROMPT_BUDGET_BYTES,
            "{}",
            kind.label()
        );
        assert!(prompt.contains("[Earlier portable transcript omitted]"));
        assert!(!prompt.contains("oldest-context"));
        assert!(prompt.contains("newest-context-📌"));
        assert!(prompt.contains("## Current authenticated message\n"));
        assert!(prompt.starts_with(TASK_CAPTURE_POLICY));
        assert!(prompt.contains(CURRENT_PROMPT));
        assert!(prompt.contains("source=\"https://attachments.example.test/photo\""));
    }
}

#[test]
fn receiver_launch_recovery_prompt_preserves_attachments_when_message_is_oversized() {
    let oversized_message = "oversized-message-🙂".repeat(RECOVERY_PROMPT_BUDGET_BYTES);

    for kind in AgentKind::ALL {
        let controller = controller(kind, ProbeOutcome::Missing);
        let (job, conversation) = durable_fixture_with_prompt(
            kind,
            BindingKind::Absent,
            "portable context",
            &oversized_message,
        );
        let plan = plan_receiver_launch(&controller, &job, &conversation, fresh_session(), |_| {
            Ok(true)
        });
        let prompt = plan.initial_prompt();

        assert!(
            prompt.len() <= RECOVERY_PROMPT_BUDGET_BYTES,
            "{}",
            kind.label()
        );
        assert!(prompt.contains("[Current authenticated message truncated]"));
        assert!(prompt.contains("\n\nAttachment references:\n"));
        assert!(prompt.contains("source=\"https://attachments.example.test/photo\""));
        assert!(prompt.contains("provider_id=\"media-1\""));
        assert!(prompt.contains("content_type=\"image/png\""));
        assert!(prompt.contains("filename=\"photo.png\""));
    }
}

#[test]
fn receiver_launch_recovery_prompt_bounds_oversized_attachment_metadata() {
    let oversized_filename = format!(
        "oversized-{}-end.png",
        "attachment-é🙂".repeat(RECOVERY_PROMPT_BUDGET_BYTES)
    );

    for kind in AgentKind::ALL {
        let controller = controller(kind, ProbeOutcome::Missing);
        let (job, conversation) = durable_fixture_with_input(
            kind,
            BindingKind::Absent,
            "portable context",
            CURRENT_PROMPT,
            vec![AttachmentRef {
                url: "https://attachments.example.test/oversized".to_owned(),
                provider_id: Some("media-oversized".to_owned()),
                content_type: Some("image/png".to_owned()),
                filename: Some(oversized_filename.clone()),
            }],
        );
        let plan = plan_receiver_launch(&controller, &job, &conversation, fresh_session(), |_| {
            Ok(true)
        });
        let prompt = plan.initial_prompt();

        assert!(
            prompt.len() <= RECOVERY_PROMPT_BUDGET_BYTES,
            "{}",
            kind.label()
        );
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
        assert!(prompt.contains("\n\nAttachment references:\n"));
        assert!(prompt.contains("source=\"https://attachments.example.test/oversized\""));
        assert!(prompt.contains("[Attachment references truncated]"));
        assert!(!prompt.contains("-end.png"));
    }
}

#[test]
fn receiver_launch_recovery_prompt_reserves_ordinary_context_before_many_attachments() {
    let attachments: Vec<_> = (0..256)
        .map(|index| AttachmentRef {
            url: format!("https://attachments.example.test/item-{index:03}"),
            provider_id: Some(format!("provider-{index:03}")),
            content_type: Some("text/plain".to_owned()),
            filename: Some(format!("item-{index:03}-{}.txt", "x".repeat(512))),
        })
        .collect();

    for kind in AgentKind::ALL {
        let controller = controller(kind, ProbeOutcome::Missing);
        let (job, conversation) = durable_fixture_with_input(
            kind,
            BindingKind::Absent,
            "portable context from the preceding turn",
            CURRENT_PROMPT,
            attachments.clone(),
        );
        let plan = plan_receiver_launch(&controller, &job, &conversation, fresh_session(), |_| {
            Ok(true)
        });
        let prompt = plan.initial_prompt();
        let (_, recovery_sections) = prompt
            .split_once("## Portable transcript\n")
            .expect("portable transcript heading");
        let (transcript, current) = recovery_sections
            .split_once("\n\n## Current authenticated message\n")
            .expect("current authenticated message heading");

        assert_eq!(
            prompt.len(),
            RECOVERY_PROMPT_BUDGET_BYTES,
            "{}",
            kind.label()
        );
        assert_eq!(transcript, "portable context from the preceding turn");
        assert!(current.starts_with(CURRENT_PROMPT));
        assert!(current.contains("source=\"https://attachments.example.test/item-000\""));
        assert!(current.contains("[Attachment references truncated]"));
        assert!(!current.contains("https://attachments.example.test/item-255"));
    }
}

#[test]
fn receiver_launch_recovery_prompt_keeps_honest_markers_when_every_section_is_oversized() {
    let transcript = format!(
        "oldest-context\n{}\nnewest-context",
        "prior-context-".repeat(RECOVERY_PROMPT_BUDGET_BYTES)
    );
    let message = format!(
        "authenticated-message-start-{}-authenticated-message-end",
        "current-message-".repeat(RECOVERY_PROMPT_BUDGET_BYTES)
    );
    let attachments: Vec<_> = (0..256)
        .map(|index| AttachmentRef {
            url: format!("https://attachments.example.test/oversized-{index:03}"),
            provider_id: Some(format!("provider-{index:03}")),
            content_type: Some("application/octet-stream".to_owned()),
            filename: Some(format!("oversized-{index:03}-{}.bin", "z".repeat(512))),
        })
        .collect();

    for kind in AgentKind::ALL {
        let controller = controller(kind, ProbeOutcome::Missing);
        let (job, conversation) = durable_fixture_with_input(
            kind,
            BindingKind::Absent,
            &transcript,
            &message,
            attachments.clone(),
        );
        let plan = plan_receiver_launch(&controller, &job, &conversation, fresh_session(), |_| {
            Ok(true)
        });
        let prompt = plan.initial_prompt();
        let (_, recovery_sections) = prompt
            .split_once("## Portable transcript\n")
            .expect("portable transcript heading");
        let (transcript_section, current_section) = recovery_sections
            .split_once("\n\n## Current authenticated message\n")
            .expect("current authenticated message heading");

        assert_eq!(
            prompt.len(),
            RECOVERY_PROMPT_BUDGET_BYTES,
            "{}",
            kind.label()
        );
        assert!(prompt.starts_with(TASK_CAPTURE_POLICY));
        assert!(transcript_section.starts_with("[Earlier portable transcript omitted]\n"));
        assert!(transcript_section.ends_with("newest-context"));
        assert!(!transcript_section.contains("oldest-context"));
        assert!(current_section.starts_with("authenticated-message-start-"));
        assert!(current_section.contains("[Current authenticated message truncated]"));
        assert!(
            current_section.contains("source=\"https://attachments.example.test/oversized-000\"")
        );
        assert!(current_section.contains("[Attachment references truncated]"));
        assert!(!current_section.contains("https://attachments.example.test/oversized-255"));
    }
}

#[test]
fn receiver_launch_uses_one_exact_task_capture_policy_for_fresh_and_resume() {
    for kind in AgentKind::ALL {
        let controller = controller(kind, ProbeOutcome::Exists);
        for binding in [BindingKind::Matching, BindingKind::Absent] {
            let (job, conversation) = durable_fixture(kind, binding, "portable transcript context");
            let plan =
                plan_receiver_launch(&controller, &job, &conversation, fresh_session(), |_| {
                    Ok(true)
                });

            assert_eq!(
                plan.initial_prompt().matches(TASK_CAPTURE_POLICY).count(),
                1,
                "{} with {binding:?}",
                kind.label(),
            );
            assert!(plan.initial_prompt().starts_with(TASK_CAPTURE_POLICY));
        }
    }
}

#[test]
fn receiver_launch_resume_prompt_bounds_utf8_message_and_attachment_metadata() {
    let message = format!(
        "authenticated-message-start-{}-authenticated-message-end",
        "current-é🙂-".repeat(RECOVERY_PROMPT_BUDGET_BYTES)
    );
    let attachments: Vec<_> = (0..256)
        .map(|index| AttachmentRef {
            url: format!("https://attachments.example.test/resume-{index:03}"),
            provider_id: Some(format!("provider-{index:03}")),
            content_type: Some("application/octet-stream".to_owned()),
            filename: Some(format!(
                "resume-{index:03}-{}.bin",
                "attachment-é🙂".repeat(128)
            )),
        })
        .collect();

    for kind in AgentKind::ALL {
        let controller = controller(kind, ProbeOutcome::Exists);
        let (job, conversation) = durable_fixture_with_input(
            kind,
            BindingKind::Matching,
            "portable transcript must not enter a resumed prompt",
            &message,
            attachments.clone(),
        );
        let plan = plan_receiver_launch(&controller, &job, &conversation, fresh_session(), |_| {
            Ok(true)
        });
        let prompt = plan.initial_prompt();

        assert_eq!(
            plan.session_plan(),
            &SessionPlan::resume(AgentSession::new("native-session").expect("native session"))
        );
        assert!(
            prompt.len() <= RECOVERY_PROMPT_BUDGET_BYTES,
            "{}",
            kind.label()
        );
        assert!(prompt.starts_with(TASK_CAPTURE_POLICY));
        assert!(prompt.contains("## Current authenticated message\n"));
        assert!(prompt.contains("authenticated-message-start-"));
        assert!(prompt.contains("[Current authenticated message truncated]"));
        assert!(prompt.contains("Attachment references:\n"));
        assert!(prompt.contains("source=\"https://attachments.example.test/resume-000\""));
        assert!(prompt.contains("[Attachment references truncated]"));
        assert!(!prompt.contains("https://attachments.example.test/resume-255"));
        assert!(!prompt.contains("portable transcript must not enter a resumed prompt"));
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }
}

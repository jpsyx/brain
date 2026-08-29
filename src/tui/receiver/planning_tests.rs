use std::path::PathBuf;

use crate::{
    actor::{ActorContext, RequestIdentity},
    agent::{
        AgentKind, AgentSession, SessionPlan, build_command,
        frontend::{SHELL_COMMAND_ARGUMENT_BUDGET_BYTES, SHELL_INLINE_VALUE_BUDGET_BYTES},
    },
    server::receiver::{AttachmentRef, Channel, InboundJob},
    state::{
        Db, ReceiverConversation, ReceiverConversationIdentity, ReceiverJob, ReceiverSessionBinding,
    },
    users::{PhoneIdentity, USERS_SCHEMA_VERSION, User, UserId, Users},
    workspace::WorkspaceId,
};

use super::planning::{
    RECOVERY_PROMPT_BUDGET_BYTES, ReceiverLaunchPlan,
    plan_receiver_launch as build_receiver_launch_plan, plan_receiver_recovery,
    receiver_job_token_marker,
};

mod localized_paths;

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const CURRENT_PROMPT: &str = "Review the attached photo and remember its subject.";
const TASK_CAPTURE_POLICY: &str = "If the message asks to add, create, capture, remember, or track a task, create it in Brain's task system; do not perform the task now unless the sender explicitly asks you to.";
const RESUME_PROMPT: &str = concat!(
    "If the message asks to add, create, capture, remember, or track a task, ",
    "create it in Brain's task system; do not perform the task now unless the ",
    "sender explicitly asks you to.",
    "\n\n## Current authenticated message\n",
    "Review the attached photo and remember its subject.",
    "\n\nLocal attachment files:\n",
    "- path=\"/workspaces/family/inbox/attachment-000.bin\"",
);

#[derive(Clone, Copy, Debug)]
enum BindingKind {
    Matching,
    OtherFrontend,
    Absent,
}

struct PlanningCase {
    binding: BindingKind,
    transcript: &'static str,
    expects_resume: bool,
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
        response_sender: "+13105550100".to_owned(),
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

fn selected_resume(binding: BindingKind) -> Option<AgentSession> {
    matches!(binding, BindingKind::Matching)
        .then(|| AgentSession::new("native-session").expect("native session"))
}

fn local_attachment_paths(job: &ReceiverJob) -> Vec<PathBuf> {
    job.inbound()
        .attachments
        .iter()
        .enumerate()
        .map(|(index, _)| {
            PathBuf::from(format!(
                "/workspaces/family/inbox/attachment-{index:03}.bin"
            ))
        })
        .collect()
}

fn long_local_attachment_paths(count: usize) -> Vec<PathBuf> {
    (0..count)
        .map(|index| {
            PathBuf::from(format!(
                "/workspaces/family/{}/attachment-{index:03}.bin",
                "long-local-path-".repeat(128)
            ))
        })
        .collect()
}

fn render_receiver_launch(
    job: &ReceiverJob,
    conversation: &ReceiverConversation,
    fresh_session: AgentSession,
    resume_session: Option<AgentSession>,
) -> ReceiverLaunchPlan {
    let paths = local_attachment_paths(job);
    render_receiver_launch_with_paths(job, conversation, &paths, fresh_session, resume_session)
}

fn render_receiver_launch_with_paths(
    job: &ReceiverJob,
    conversation: &ReceiverConversation,
    paths: &[PathBuf],
    fresh_session: AgentSession,
    resume_session: Option<AgentSession>,
) -> ReceiverLaunchPlan {
    let path_refs = paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    build_receiver_launch_plan(job, conversation, &path_refs, fresh_session, resume_session)
        .expect("matching localized attachment paths")
}

#[test]
fn accepted_recovery_plan_is_resume_only_bounded_and_contains_no_private_job_material() {
    let private_message = "delete private inbound instruction after handling";
    let private_transcript = "prior private answer and transcript material";
    let (job, conversation) = durable_fixture_with_prompt(
        AgentKind::Codex,
        BindingKind::Matching,
        private_transcript,
        private_message,
    );
    let session = AgentSession::new("exact-native-session").expect("native session");

    let plan = plan_receiver_recovery(job.id(), job.token(), session.clone());

    assert!(
        plan.session_plan() == &SessionPlan::resume(session),
        "recovery selected the wrong session plan"
    );
    assert!(plan.initial_prompt().len() <= RECOVERY_PROMPT_BUDGET_BYTES);
    assert!(plan.initial_prompt().contains(&job.id().to_string()));
    assert!(plan.initial_prompt().contains("Inspect the prior work"));
    assert!(
        plan.initial_prompt()
            .contains("avoid repeating completed side effects")
    );
    assert!(
        plan.initial_prompt()
            .contains("finish the pending response")
    );
    assert!(
        plan.initial_prompt()
            .contains("Do not replay the original inbound instruction")
    );
    assert!(
        plan.initial_prompt()
            .ends_with(&receiver_job_token_marker(job.token()))
    );
    for private_value in [
        private_message,
        private_transcript,
        job.inbound().authenticated_sender.as_str(),
        job.inbound().provider_id.as_deref().expect("provider ID"),
        job.inbound().attachments[0].url.as_str(),
        conversation.transcript_markdown(),
    ] {
        assert!(
            !plan.initial_prompt().contains(private_value),
            "recovery prompt leaked private job material"
        );
    }
}

#[test]
fn receiver_launch_planning_renders_the_authorized_session_choice_for_every_frontend() {
    let cases = [
        PlanningCase {
            binding: BindingKind::Matching,
            transcript: "old portable context",
            expects_resume: true,
        },
        PlanningCase {
            binding: BindingKind::OtherFrontend,
            transcript: "old portable context",
            expects_resume: false,
        },
        PlanningCase {
            binding: BindingKind::Absent,
            transcript: "",
            expects_resume: false,
        },
    ];

    for kind in AgentKind::ALL {
        for case in &cases {
            let (job, conversation) = durable_fixture(kind, case.binding, case.transcript);
            let plan = render_receiver_launch(
                &job,
                &conversation,
                fresh_session(),
                selected_resume(case.binding),
            );

            if case.expects_resume {
                assert!(
                    plan.session_plan()
                        == &SessionPlan::resume(
                            AgentSession::new("native-session").expect("native session")
                        ),
                    "resume planning selected the wrong session plan"
                );
                assert!(
                    plan.initial_prompt()
                        == format!(
                            "{RESUME_PROMPT}\n<!-- brain:receiver-job-token={} -->",
                            job.token()
                        ),
                    "resume prompt did not omit portable transcript context"
                );
            } else {
                assert!(
                    plan.session_plan() == &SessionPlan::fresh(fresh_session()),
                    "fresh planning selected the wrong session plan"
                );
                assert!(
                    plan.initial_prompt()
                        .contains("## Current authenticated message"),
                    "fresh planning must use recovery prompt separation",
                );
                assert!(
                    plan.initial_prompt().contains(TASK_CAPTURE_POLICY),
                    "fresh planning must retain the shared task-capture policy",
                );
                assert!(
                    plan.initial_prompt().contains(CURRENT_PROMPT),
                    "fresh planning must retain the current job",
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
        let (job, conversation) = durable_fixture(kind, BindingKind::Absent, &transcript);
        let plan = render_receiver_launch(&job, &conversation, fresh_session(), None);
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
        assert!(prompt.contains("path=\"/workspaces/family/inbox/attachment-000.bin\""));
    }
}

#[test]
fn receiver_launch_recovery_prompt_preserves_attachments_when_message_is_oversized() {
    let oversized_message = "oversized-message-🙂".repeat(RECOVERY_PROMPT_BUDGET_BYTES);

    for kind in AgentKind::ALL {
        let (job, conversation) = durable_fixture_with_prompt(
            kind,
            BindingKind::Absent,
            "portable context",
            &oversized_message,
        );
        let plan = render_receiver_launch(&job, &conversation, fresh_session(), None);
        let prompt = plan.initial_prompt();

        assert!(
            prompt.len() <= RECOVERY_PROMPT_BUDGET_BYTES,
            "{}",
            kind.label()
        );
        assert!(prompt.contains("[Current authenticated message truncated]"));
        assert!(prompt.contains("\n\nLocal attachment files:\n"));
        assert!(prompt.contains("path=\"/workspaces/family/inbox/attachment-000.bin\""));
        assert!(!prompt.contains("https://attachments.example.test/photo"));
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
        let (job, conversation) = durable_fixture_with_input(
            kind,
            BindingKind::Absent,
            "portable context from the preceding turn",
            CURRENT_PROMPT,
            attachments.clone(),
        );
        let plan = render_receiver_launch(&job, &conversation, fresh_session(), None);
        let prompt = plan.initial_prompt();
        let (_, recovery_sections) = prompt
            .split_once("## Portable transcript\n")
            .expect("portable transcript heading");
        let (transcript, current) = recovery_sections
            .split_once("\n\n## Current authenticated message\n")
            .expect("current authenticated message heading");

        assert!(prompt.len() <= RECOVERY_PROMPT_BUDGET_BYTES);
        assert!(
            transcript == "portable context from the preceding turn",
            "portable transcript context changed"
        );
        assert!(current.starts_with(CURRENT_PROMPT));
        assert!(current.contains("path=\"/workspaces/family/inbox/attachment-000.bin\""));
        assert!(current.contains("path=\"/workspaces/family/inbox/attachment-255.bin\""));
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
        let (job, conversation) = durable_fixture_with_input(
            kind,
            BindingKind::Absent,
            &transcript,
            &message,
            attachments.clone(),
        );
        let paths = long_local_attachment_paths(attachments.len());
        let plan =
            render_receiver_launch_with_paths(&job, &conversation, &paths, fresh_session(), None);
        let prompt = plan.initial_prompt();
        let (_, recovery_sections) = prompt
            .split_once("## Portable transcript\n")
            .expect("portable transcript heading");
        let (transcript_section, current_section) = recovery_sections
            .split_once("\n\n## Current authenticated message\n")
            .expect("current authenticated message heading");

        assert!(prompt.len() <= RECOVERY_PROMPT_BUDGET_BYTES);
        assert!(prompt.starts_with(TASK_CAPTURE_POLICY));
        assert!(transcript_section.starts_with("[Earlier portable transcript omitted]\n"));
        assert!(transcript_section.ends_with("newest-context"));
        assert!(!transcript_section.contains("oldest-context"));
        assert!(current_section.starts_with("authenticated-message-start-"));
        assert!(current_section.contains("[Current authenticated message truncated]"));
        assert!(current_section.contains(&paths[0].display().to_string()));
        assert!(current_section.contains("[Additional local attachment files omitted]"));
        assert!(!current_section.contains("attachment-255.bin"));
    }
}

#[test]
fn receiver_launch_uses_one_exact_task_capture_policy_for_fresh_and_resume() {
    for kind in AgentKind::ALL {
        for binding in [BindingKind::Matching, BindingKind::Absent] {
            let (job, conversation) = durable_fixture(kind, binding, "portable transcript context");
            let plan = render_receiver_launch(
                &job,
                &conversation,
                fresh_session(),
                selected_resume(binding),
            );

            assert!(
                plan.initial_prompt().matches(TASK_CAPTURE_POLICY).count() == 1,
                "{} with {binding:?} had the wrong task-capture policy count",
                kind.label()
            );
            assert!(plan.initial_prompt().starts_with(TASK_CAPTURE_POLICY));
        }
    }
}

#[test]
fn receiver_launch_appends_the_exact_job_token_marker_as_the_final_prompt_line() {
    for kind in AgentKind::ALL {
        for binding in [BindingKind::Matching, BindingKind::Absent] {
            let (job, conversation) = durable_fixture(kind, binding, "synthetic context");
            let plan = render_receiver_launch(
                &job,
                &conversation,
                fresh_session(),
                selected_resume(binding),
            );
            let expected = format!("<!-- brain:receiver-job-token={} -->", job.token());

            assert!(
                plan.initial_prompt().lines().last() == Some(expected.as_str()),
                "receiver job token marker was not last"
            );
            assert!(
                plan.initial_prompt().matches(&expected).count() == 1,
                "receiver job token marker appeared the wrong number of times"
            );
            assert!(plan.initial_prompt().len() <= RECOVERY_PROMPT_BUDGET_BYTES);
        }
    }
}

#[test]
fn receiver_launch_resume_prompt_bounds_utf8_message_and_local_attachment_paths() {
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
        let (job, conversation) = durable_fixture_with_input(
            kind,
            BindingKind::Matching,
            "portable transcript must not enter a resumed prompt",
            &message,
            attachments.clone(),
        );
        let paths = long_local_attachment_paths(attachments.len());
        let plan = render_receiver_launch_with_paths(
            &job,
            &conversation,
            &paths,
            fresh_session(),
            selected_resume(BindingKind::Matching),
        );
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
        assert!(prompt.contains("Local attachment files:\n"));
        assert!(prompt.contains(&paths[0].display().to_string()));
        assert!(prompt.contains("[Additional local attachment files omitted]"));
        assert!(!prompt.contains("attachment-255.bin"));
        assert!(!prompt.contains("portable transcript must not enter a resumed prompt"));
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }
}

#[test]
fn receiver_launch_prompts_fit_every_real_frontend_shell_command_for_fresh_and_resume() {
    let quote_mix = "'$$$".repeat(RECOVERY_PROMPT_BUDGET_BYTES);
    let transcript = format!("oldest-é🙂\n{quote_mix}\nnewest-é🙂");
    let message = format!("authenticated-message-start-é🙂-{quote_mix}-message-end");
    let attachments: Vec<_> = (0..10)
        .map(|index| AttachmentRef {
            url: format!("https://attachments.example.test/quote-{index:03}"),
            provider_id: Some(format!("provider-{index:03}")),
            content_type: Some("text/plain".to_owned()),
            filename: Some(format!("quote-{index:03}.txt")),
        })
        .collect();
    let paths = (0..attachments.len())
        .map(|index| {
            PathBuf::from(format!(
                "/workspaces/family/{}/quote-{index:03}-é🙂.txt",
                "local-'$$$-".repeat(900)
            ))
        })
        .collect::<Vec<_>>();

    for kind in AgentKind::ALL {
        for binding in [BindingKind::Matching, BindingKind::Absent] {
            let (job, conversation) = durable_fixture_with_input(
                kind,
                binding,
                &transcript,
                &message,
                attachments.clone(),
            );
            let plan = render_receiver_launch_with_paths(
                &job,
                &conversation,
                &paths,
                fresh_session(),
                selected_resume(binding),
            );
            let prompt = plan.initial_prompt();
            let command = build_command(
                kind,
                "receiver-frontend --fixed-option",
                plan.session_plan(),
                Some(prompt),
            );
            let marker = format!("<!-- brain:receiver-job-token={} -->", job.token());

            assert!(
                prompt.len() <= SHELL_INLINE_VALUE_BUDGET_BYTES,
                "{} with {binding:?} exceeded the shell inline-value budget",
                kind.label(),
            );
            assert!(prompt.starts_with(TASK_CAPTURE_POLICY));
            assert!(prompt.contains("authenticated-message-start-é🙂-"));
            assert!(prompt.contains(&paths[0].display().to_string()));
            assert!(prompt.contains("[Current authenticated message truncated]"));
            assert!(prompt.contains("[Additional local attachment files omitted]"));
            assert!(
                prompt.lines().last() == Some(marker.as_str()),
                "receiver job token marker was not last"
            );
            assert!(
                prompt.matches(&marker).count() == 1,
                "receiver prompt token marker appeared the wrong number of times"
            );
            assert!(
                command.matches(&marker).count() == 1,
                "receiver command token marker appeared the wrong number of times"
            );
            assert!(
                command.len() <= SHELL_COMMAND_ARGUMENT_BUDGET_BYTES,
                "{} with {binding:?} exceeded the shell command-argument budget",
                kind.label(),
            );
        }
    }
}

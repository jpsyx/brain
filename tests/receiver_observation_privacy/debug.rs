use std::{path::PathBuf, sync::Arc};

use brain::access::AccessMode;
use brain::agent::{
    AgentKind, AgentSession, HookMetadata, LaunchRequest, SessionPlan, SessionScope,
};
use brain::server::receiver::{
    AttachmentRef, Channel, EmailReplyContext, InboundJob, RestartPlan, StagedAttachment,
};
use brain::server::reply::ReplyEnvelope;
use brain::state::{EmailLineage, ReceiverConversationIdentity, ReceiverSessionBinding};
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

use super::{
    LOCAL_PATH_CANARY, PRIVATE_CANARIES, PRIVATE_HOST_CANARY, SENDER_CANARY, SESSION, TOKEN,
    WORKSPACE,
};

const PROVIDER_ID_CANARY: &str = "provider-job-canary-4b51";
const ATTACHMENT_PROVIDER_CANARY: &str = "attachment-provider-canary-6c62";
const ATTACHMENT_TYPE_CANARY: &str = "application/private-canary-7d73";
const ATTACHMENT_NAME_CANARY: &str = "attachment-name-canary-8e84.txt";
const EMAIL_SUBJECT_CANARY: &str = "subject-canary-9f95";
const EMAIL_ID_CANARY: &str = "provider-email-canary-a0a6";
const MESSAGE_ID_CANARY: &str = "message-lineage-canary-b1b7";
const TRANSCRIPT_CANARY: &str = "transcript-canary-c2c8";
const USER_CANARY: &str = "private-user-canary";
const HOOK_CANARY: &str = "hook-metadata-canary-d3d9";
const MODEL_CANARIES: &[&str] = &[
    PRIVATE_CANARIES[0],
    PRIVATE_CANARIES[1],
    PRIVATE_CANARIES[2],
    PRIVATE_CANARIES[3],
    PRIVATE_CANARIES[4],
    SENDER_CANARY,
    LOCAL_PATH_CANARY,
    PRIVATE_HOST_CANARY,
    PROVIDER_ID_CANARY,
    ATTACHMENT_PROVIDER_CANARY,
    ATTACHMENT_TYPE_CANARY,
    ATTACHMENT_NAME_CANARY,
    EMAIL_SUBJECT_CANARY,
    EMAIL_ID_CANARY,
    MESSAGE_ID_CANARY,
    TRANSCRIPT_CANARY,
    USER_CANARY,
    HOOK_CANARY,
    SESSION,
    WORKSPACE,
];

#[test]
fn adjacent_agent_and_reply_debug_omits_receiver_private_content() {
    let (temporary, workspace) = private_workspace();
    let actor = private_inbound_job().actor;
    let session = AgentSession::new(SESSION).expect("private native session");
    let scope = SessionScope::new(AgentKind::Claude, workspace.id(), actor.clone());
    let fresh = SessionPlan::fresh(session.clone());
    let resume = SessionPlan::resume(session.clone());
    let launch = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor,
        fresh.clone(),
        Some(PRIVATE_CANARIES[0].to_owned()),
        AccessMode::WorkspaceOnly,
    )
    .with_hook_metadata(HookMetadata::new(vec![(
        "receiver-hook".to_owned(),
        HOOK_CANARY.to_owned(),
    )]));
    let reply = ReplyEnvelope {
        channel: SENDER_CANARY,
        text: PRIVATE_CANARIES[2].to_owned(),
        long_form_available: true,
    };

    let rendered = [
        ("agent session", format!("{session:?}")),
        ("session scope", format!("{scope:?}")),
        ("fresh session plan", format!("{fresh:?}")),
        ("resume session plan", format!("{resume:?}")),
        ("launch request", format!("{launch:?}")),
        ("reply envelope", format!("{reply:?}")),
    ];
    for (label, value) in &rendered {
        assert_private_absent(label, value);
    }
    assert!(
        rendered[2].1 == "SessionPlan::Fresh(<redacted>)",
        "fresh session plan category mismatch"
    );
    assert!(
        rendered[3].1 == "SessionPlan::Resume(<redacted>)",
        "resume session plan category mismatch"
    );

    drop(temporary);
}

#[test]
fn public_receiver_model_debug_omits_private_content_and_keeps_plan_categories() {
    let inbound = private_inbound_job();
    let attachment = inbound.attachments[0].clone();
    let reply = inbound.email_reply.clone().expect("email reply context");
    let staged = StagedAttachment {
        source: PRIVATE_HOST_CANARY.to_owned(),
        path: Some(PathBuf::from(LOCAL_PATH_CANARY)),
        error: Some(PRIVATE_CANARIES[1].to_owned()),
    };
    let lineage = EmailLineage::verified(MESSAGE_ID_CANARY).expect("verified lineage");
    let identity = ReceiverConversationIdentity::email(
        inbound.workspace_id,
        inbound.actor.user_id().clone(),
        lineage.clone(),
    );
    let binding = ReceiverSessionBinding::new(brain::agent::AgentKind::Claude, SESSION)
        .expect("receiver session binding");
    let resume = binding.plan(brain::agent::AgentKind::Claude, TRANSCRIPT_CANARY);
    let fresh = binding.plan(brain::agent::AgentKind::Codex, TRANSCRIPT_CANARY);
    let restart = RestartPlan {
        command: PRIVATE_CANARIES[0].to_owned(),
        dropped: vec![PRIVATE_CANARIES[1].to_owned()],
    };

    for (label, rendered) in [
        ("attachment", format!("{attachment:?}")),
        ("email reply", format!("{reply:?}")),
        ("inbound job", format!("{inbound:?}")),
        ("staged attachment", format!("{staged:?}")),
        ("email lineage", format!("{lineage:?}")),
        ("conversation identity", format!("{identity:?}")),
        ("session binding", format!("{binding:?}")),
        ("resume plan", format!("{resume:?}")),
        ("fresh plan", format!("{fresh:?}")),
        ("restart plan", format!("{restart:?}")),
    ] {
        assert_private_absent(label, &rendered);
    }
    let uncertain_lineage = format!("{:?}", EmailLineage::Uncertain);
    assert_private_absent("uncertain email lineage", &uncertain_lineage);
    assert!(
        format!("{resume:?}") == "ReceiverSessionPlan::ResumeNative(<redacted>)",
        "resume plan Debug shape mismatch"
    );
    assert!(
        format!("{fresh:?}") == "ReceiverSessionPlan::FreshFromTranscript(<redacted>)",
        "fresh plan Debug shape mismatch"
    );
    assert!(
        format!("{lineage:?}") == "EmailLineage::Verified(<redacted>)",
        "verified lineage Debug shape mismatch"
    );
    assert!(
        uncertain_lineage == "EmailLineage::Uncertain",
        "uncertain lineage Debug shape mismatch"
    );
    assert!(
        format!("{restart:?}") == "RestartPlan(<redacted>)",
        "restart plan Debug shape mismatch"
    );
}

#[test]
fn whole_value_assertion_helper_never_formats_private_values() {
    let left = private_inbound_job();
    let mut right = left.clone();
    right.prompt = PRIVATE_CANARIES[1].to_owned();

    let failure = std::panic::catch_unwind(|| {
        assert_content_equal("inbound job", &left, &right);
    })
    .expect_err("different inbound jobs must fail");
    let message = panic_message(&failure);

    assert_private_absent("whole-value assertion", message);
    assert!(
        message == "inbound job values differ",
        "whole-value assertion shape mismatch"
    );
    assert!(
        !message.contains("left"),
        "whole-value diagnostic named a value side"
    );
    assert!(
        !message.contains("right"),
        "whole-value diagnostic named a value side"
    );
}

fn assert_private_absent(label: &str, rendered: &str) {
    for (index, canary) in MODEL_CANARIES.iter().enumerate() {
        assert!(
            !rendered.contains(canary),
            "{label} contains private canary at index {index}"
        );
    }
    assert!(
        !rendered.contains(TOKEN),
        "{label} contains a receiver token"
    );
}

fn assert_content_equal<T: PartialEq>(label: &str, left: &T, right: &T) {
    assert!(left == right, "{label} values differ");
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-text panic")
}

fn private_inbound_job() -> InboundJob {
    let actor = serde_json::from_value(serde_json::json!({
        "user_id": USER_CANARY,
        "display_name": PRIVATE_CANARIES[1],
        "channel": "email",
    }))
    .expect("private actor fixture");
    InboundJob {
        job_id: uuid::Uuid::parse_str("44444444-4444-4444-8444-444444444444")
            .expect("private job ID"),
        workspace_id: brain::workspace::WorkspaceId::parse(WORKSPACE)
            .expect("private workspace ID"),
        actor,
        channel: Channel::Email,
        authenticated_sender: SENDER_CANARY.to_owned(),
        response_sender: PRIVATE_CANARIES[3].to_owned(),
        prompt: PRIVATE_CANARIES[0].to_owned(),
        attachments: vec![AttachmentRef {
            url: PRIVATE_HOST_CANARY.to_owned(),
            provider_id: Some(ATTACHMENT_PROVIDER_CANARY.to_owned()),
            content_type: Some(ATTACHMENT_TYPE_CANARY.to_owned()),
            filename: Some(ATTACHMENT_NAME_CANARY.to_owned()),
        }],
        received_at_unix_ms: 1_000,
        provider_id: Some(PROVIDER_ID_CANARY.to_owned()),
        thread_participants: vec![PRIVATE_CANARIES[3].to_owned()],
        response_email: Some(PRIVATE_CANARIES[3].to_owned()),
        allowed_response_recipients: vec![PRIVATE_CANARIES[3].to_owned()],
        email_reply: Some(EmailReplyContext {
            provider_email_id: EMAIL_ID_CANARY.to_owned(),
            subject: EMAIL_SUBJECT_CANARY.to_owned(),
            message_id: Some(MESSAGE_ID_CANARY.to_owned()),
        }),
    }
}

fn private_workspace() -> (tempfile::TempDir, Arc<WorkspaceContext>) {
    let temporary = tempfile::tempdir().expect("temporary private workspace");
    let root = temporary.path().join(PRIVATE_CANARIES[1]);
    std::fs::create_dir_all(root.join(".config")).expect("private workspace config");
    let workspace = WorkspaceContext::new(
        temporary.path(),
        WorkspaceId::parse(WORKSPACE).expect("private workspace ID"),
        WorkspaceName::parse("privacy-workspace").expect("private workspace name"),
        &root,
        USER_CANARY,
        temporary.path(),
    )
    .expect("private workspace context");
    (temporary, Arc::new(workspace))
}

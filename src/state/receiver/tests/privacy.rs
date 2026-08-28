const PRIVATE_PROMPT: &str = "private-prompt-canary-a101";
const PRIVATE_SENDER: &str = "private-sender-canary-b202@example.test";
const PRIVATE_RECIPIENT: &str = "private-recipient-canary-c303@example.test";
const PRIVATE_ATTACHMENT_URL: &str = "https://private.example.test/attachment-d404";
const PRIVATE_ATTACHMENT_ID: &str = "private-attachment-id-e505";
const PRIVATE_ATTACHMENT_TYPE: &str = "application/private-f606";
const PRIVATE_ATTACHMENT_NAME: &str = "private-attachment-g707.txt";
const PRIVATE_SUBJECT: &str = "private-subject-h808";
const PRIVATE_EMAIL_ID: &str = "private-email-id-i909";
const PRIVATE_MESSAGE_ID: &str = "private-message-id-j010";
const PRIVATE_PROVIDER_ID: &str = "private-provider-id-k111";
const PRIVATE_LINEAGE: &str = "private-lineage-l212";
const PRIVATE_TRANSCRIPT: &str = "private-transcript-m313";
const PRIVATE_NATIVE_SESSION: &str = "private-native-session-n414";
const PRIVATE_OWNER: &str = "private-owner-o515";
const PRIVATE_INSTANCE: &str = "private-instance-p616";
const PRIVATE_OBSERVATION_SESSION: &str = "private-observation-session-q717";
const PRIVATE_STORED_ERROR: &str = "private-stored-error-r818";
const PRIVATE_ANSWER: &str = "private-answer-s919";

#[test]
fn full_receiver_model_graph_debug_is_content_free() {
    let db = Db::open_in_memory().expect("receiver state");
    let workspace_id = receiver_workspace_id();
    let user_id = receiver_user_id();
    let mut inbound = receiver_job(Some(PRIVATE_PROVIDER_ID), 100);
    inbound.channel = crate::server::receiver::Channel::Email;
    inbound.authenticated_sender = PRIVATE_SENDER.to_owned();
    inbound.prompt = PRIVATE_PROMPT.to_owned();
    inbound.attachments = vec![crate::server::receiver::AttachmentRef {
        url: PRIVATE_ATTACHMENT_URL.to_owned(),
        provider_id: Some(PRIVATE_ATTACHMENT_ID.to_owned()),
        content_type: Some(PRIVATE_ATTACHMENT_TYPE.to_owned()),
        filename: Some(PRIVATE_ATTACHMENT_NAME.to_owned()),
    }];
    inbound.thread_participants = vec![PRIVATE_RECIPIENT.to_owned()];
    inbound.response_email = Some(PRIVATE_RECIPIENT.to_owned());
    inbound.allowed_response_recipients = vec![PRIVATE_RECIPIENT.to_owned()];
    inbound.email_reply = Some(crate::server::receiver::EmailReplyContext {
        provider_email_id: PRIVATE_EMAIL_ID.to_owned(),
        subject: PRIVATE_SUBJECT.to_owned(),
        message_id: Some(PRIVATE_MESSAGE_ID.to_owned()),
    });
    let lineage = EmailLineage::verified(PRIVATE_LINEAGE).expect("private email lineage");
    let identity = ReceiverConversationIdentity::email(workspace_id, user_id, lineage.clone());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept private receiver job");
    let binding = ReceiverSessionBinding::new(
        crate::agent::AgentKind::Claude,
        PRIVATE_NATIVE_SESSION,
    )
    .expect("private receiver binding");
    assert!(
        db.update_receiver_conversation(
            accepted.conversation_id(),
            PRIVATE_TRANSCRIPT,
            Some(&binding),
            200,
        )
        .expect("update private conversation")
    );
    db.conn
        .execute(
            "UPDATE receiver_jobs SET last_error = ?2 WHERE job_id = ?1",
            rusqlite::params![accepted.job_id().to_string(), PRIVATE_STORED_ERROR],
        )
        .expect("set private stored error");
    let run = db
        .claim_next_receiver_run(PRIVATE_OWNER, 1_000, 2_000)
        .expect("claim private receiver run")
        .expect("private receiver run");
    let notice = ReceiverUnavailableNoticeClaim::new(run.job(), PRIVATE_OWNER.to_owned(), 2_000);
    let native_session = crate::agent::AgentSession::new(PRIVATE_NATIVE_SESSION)
        .expect("private native session");
    let scope = crate::agent::SessionScope::new(
        crate::agent::AgentKind::Claude,
        workspace_id,
        inbound.actor.clone(),
    );
    let attribution = ReceiverSessionAttribution::new(
        accepted.conversation_id(),
        PRIVATE_INSTANCE.to_owned(),
        native_session.clone(),
        scope,
    );
    let launch = ReceiverLaunchObservation {
        token: run.job().token(),
        instance: PRIVATE_INSTANCE.to_owned(),
        session_id: PRIVATE_OBSERVATION_SESSION.to_owned(),
        observed_at_unix_ms: 1_100,
        authorized_at_unix_ms: 1_100,
    };
    let observation = ReceiverObservation {
        token: run.job().token(),
        instance: PRIVATE_INSTANCE.to_owned(),
        session_id: PRIVATE_OBSERVATION_SESSION.to_owned(),
        phase: ReceiverNonterminalObservationPhase::Accepted,
        revision: 1,
        observed_at_unix_ms: 1_200,
        authorized_at_unix_ms: 1_200,
    };
    let completion = ReceiverCompletionRequest {
        job_id: run.job().id(),
        token: run.job().token(),
        owner: PRIVATE_OWNER,
        registration: &attribution,
        completed_session: &native_session,
        answer: PRIVATE_ANSWER,
        observed_at_unix_ms: 1_300,
        authorized_at_unix_ms: 1_300,
    };
    let effect = ReceiverReconciliationEffect::new(
        ReceiverReconciliationAction::TerminalFailure,
        ReceiverReconciliationReason::RecoveryShutdown,
        run.job().id(),
        run.job().token(),
        Some(PRIVATE_INSTANCE.to_owned()),
        Some(PRIVATE_OBSERVATION_SESSION.to_owned()),
    );
    let cleanup = ReceiverRecoveryCleanupOutcome::Exact(effect.clone());
    let resume = run
        .conversation()
        .session_plan(crate::agent::AgentKind::Claude);
    let fresh = run
        .conversation()
        .session_plan(crate::agent::AgentKind::Codex);
    let token = run.job().token().to_string();
    let workspace = workspace_id.to_string();
    let private_values = [
        PRIVATE_PROMPT,
        PRIVATE_SENDER,
        PRIVATE_RECIPIENT,
        PRIVATE_ATTACHMENT_URL,
        PRIVATE_ATTACHMENT_ID,
        PRIVATE_ATTACHMENT_TYPE,
        PRIVATE_ATTACHMENT_NAME,
        PRIVATE_SUBJECT,
        PRIVATE_EMAIL_ID,
        PRIVATE_MESSAGE_ID,
        PRIVATE_PROVIDER_ID,
        PRIVATE_ANSWER,
        PRIVATE_LINEAGE,
        PRIVATE_TRANSCRIPT,
        PRIVATE_NATIVE_SESSION,
        PRIVATE_OWNER,
        PRIVATE_INSTANCE,
        PRIVATE_OBSERVATION_SESSION,
        PRIVATE_STORED_ERROR,
        workspace.as_str(),
        inbound.actor.user_id().as_str(),
        token.as_str(),
    ];

    for (label, rendered) in [
        ("inbound", format!("{inbound:?}")),
        ("lineage", format!("{lineage:?}")),
        ("identity", format!("{identity:?}")),
        ("binding", format!("{binding:?}")),
        ("run claim", format!("{run:?}")),
        ("claim", format!("{:?}", run.claim())),
        ("job", format!("{:?}", run.job())),
        ("conversation", format!("{:?}", run.conversation())),
        ("notice", format!("{notice:?}")),
        ("attribution", format!("{attribution:?}")),
        ("launch", format!("{launch:?}")),
        ("observation", format!("{observation:?}")),
        ("completion", format!("{completion:?}")),
        ("effect", format!("{effect:?}")),
        ("cleanup", format!("{cleanup:?}")),
        ("resume", format!("{resume:?}")),
        ("fresh", format!("{fresh:?}")),
    ] {
        assert_private_values_absent(label, &rendered, &private_values);
    }

    assert_eq!(
        format!("{resume:?}"),
        "ReceiverSessionPlan::ResumeNative(<redacted>)"
    );
    assert_eq!(
        format!("{fresh:?}"),
        "ReceiverSessionPlan::FreshFromTranscript(<redacted>)"
    );
}

fn assert_private_values_absent(label: &str, rendered: &str, values: &[&str]) {
    for (index, value) in values.iter().enumerate() {
        assert!(
            !rendered.contains(value),
            "{label} Debug contains private value at index {index}"
        );
    }
}

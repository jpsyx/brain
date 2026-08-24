#[test]
fn sms_conversation_identity_is_stable_for_workspace_and_user() {
    let first = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let second = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());

    assert_eq!(first, second);
    assert_eq!(first.channel(), crate::server::receiver::Channel::Sms);
}
#[test]
fn email_conversation_identity_uses_only_verified_lineage() {
    let verified = EmailLineage::verified("provider-thread-7").expect("verified lineage");
    let first = ReceiverConversationIdentity::email(
        receiver_workspace_id(),
        receiver_user_id(),
        verified.clone(),
    );
    let second = ReceiverConversationIdentity::email(
        receiver_workspace_id(),
        receiver_user_id(),
        verified,
    );
    let uncertain = ReceiverConversationIdentity::email(
        receiver_workspace_id(),
        receiver_user_id(),
        EmailLineage::Uncertain,
    );
    let another_uncertain = ReceiverConversationIdentity::email(
        receiver_workspace_id(),
        receiver_user_id(),
        EmailLineage::Uncertain,
    );

    assert_eq!(first, second);
    assert_ne!(first, uncertain);
    assert_ne!(uncertain, another_uncertain);
}

#[test]
fn receiver_job_states_allow_only_forward_lifecycle_transitions() {
    assert!(ReceiverJobState::Queued.can_transition_to(ReceiverJobState::Claimed));
    assert!(ReceiverJobState::Claimed.can_transition_to(ReceiverJobState::Launching));
    assert!(ReceiverJobState::Launching.can_transition_to(ReceiverJobState::Accepted));
    assert!(ReceiverJobState::Launching.can_transition_to(ReceiverJobState::Done));
    assert!(ReceiverJobState::Accepted.can_transition_to(ReceiverJobState::Processing));
    assert!(ReceiverJobState::Processing.can_transition_to(ReceiverJobState::AnswerReady));
    assert!(ReceiverJobState::AnswerReady.can_transition_to(ReceiverJobState::Delivering));
    assert!(ReceiverJobState::Delivering.can_transition_to(ReceiverJobState::Done));
    assert!(!ReceiverJobState::Done.can_transition_to(ReceiverJobState::Processing));
    assert!(!ReceiverJobState::Failed.can_transition_to(ReceiverJobState::Claimed));
}

#[test]
fn receiver_session_binding_resumes_only_its_own_frontend() {
    let binding = ReceiverSessionBinding::new(
        crate::agent::AgentKind::Claude,
        "native-session-1",
    )
    .expect("valid native session binding");

    assert_eq!(
        binding.plan(crate::agent::AgentKind::Claude, "# Transcript\nhello"),
        ReceiverSessionPlan::ResumeNative("native-session-1".to_owned())
    );
    assert_eq!(
        binding.plan(crate::agent::AgentKind::Codex, "# Transcript\nhello"),
        ReceiverSessionPlan::FreshFromTranscript("# Transcript\nhello".to_owned())
    );
}

#[test]
fn native_binding_replacement_requires_the_exact_instance_actual_session_and_preserves_transcript()
{
    use crate::agent::{AgentSession, SessionScope};

    for frontend in [
        crate::agent::AgentKind::Codex,
        crate::agent::AgentKind::OpenCode,
    ] {
        let db = Db::open_in_memory().expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let job = receiver_job(None, 100);
        let accepted = db
            .accept_receiver_job(&job, &identity)
            .expect("accept receiver job");
        db.update_receiver_conversation(
            accepted.conversation_id(),
            "# Portable transcript\n\nUser: private context",
            None,
            1_000,
        )
        .expect("seed transcript");
        let scope = SessionScope::new(frontend, receiver_workspace_id(), job.actor.clone());
        let placeholder = AgentSession::new(format!("pending-{}-launch", frontend.as_str()))
            .expect("placeholder");
        let registration = db
            .register_receiver_session(
                accepted.conversation_id(),
                &placeholder,
                "remote-instance",
                42,
                &scope,
            )
            .expect("register remote placeholder");

        assert!(!db
            .replace_receiver_binding_from_instance(&registration, 1_100)
            .expect("placeholder is not a native binding"));
        assert!(
            db.receiver_conversation(accepted.conversation_id())
                .unwrap()
                .unwrap()
                .binding()
                .is_none()
        );

        db.conn
            .execute(
                "UPDATE brain_sessions SET agent_session_id = ?1
                 WHERE brain_instance_id = 'remote-instance'",
                [format!("actual-{}-session", frontend.as_str())],
            )
            .expect("simulate lifecycle-reported rotation");

        let other_instance = ReceiverSessionAttribution::new(
            accepted.conversation_id(),
            "other-instance".to_owned(),
            placeholder.clone(),
            scope.clone(),
        );
        assert!(!db
            .replace_receiver_binding_from_instance(&other_instance, 1_200)
            .expect("reject another instance"));

        let other_job = receiver_job_for(
            receiver_workspace_id(),
            crate::server::receiver::Channel::Email,
            None,
            100,
        );
        let other_scope =
            SessionScope::new(frontend, receiver_workspace_id(), other_job.actor.clone());
        let other_channel = ReceiverSessionAttribution::new(
            accepted.conversation_id(),
            "other-channel-instance".to_owned(),
            AgentSession::new("pending-other-channel").expect("placeholder"),
            other_scope,
        );
        assert!(db
            .replace_receiver_binding_from_instance(&other_channel, 1_200)
            .is_err());

        assert!(db
            .replace_receiver_binding_from_instance(&registration, 1_200)
            .expect("persist actual native binding"));

        let conversation = db
            .receiver_conversation(accepted.conversation_id())
            .expect("load conversation")
            .expect("conversation remains durable");
        assert_eq!(
            conversation.transcript_markdown(),
            "# Portable transcript\n\nUser: private context"
        );
        assert_eq!(
            conversation.binding(),
            Some(
                &ReceiverSessionBinding::new(
                    frontend,
                    format!("actual-{}-session", frontend.as_str())
                )
                .expect("actual binding")
            )
        );
        let registered_actual = db
            .conn
            .query_row(
                "SELECT actual_session_id FROM receiver_session_registrations
                 WHERE brain_instance_id = 'remote-instance'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("load lifecycle-rotated registration");
        assert_eq!(
            registered_actual,
            format!("actual-{}-session", frontend.as_str())
        );
    }
}

#[test]
fn native_binding_replacement_rejects_a_placeholder_registered_to_another_instance() {
    use crate::agent::{AgentSession, SessionScope};

    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let job = receiver_job(None, 100);
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    let scope = SessionScope::new(
        crate::agent::AgentKind::Codex,
        receiver_workspace_id(),
        job.actor,
    );
    let first = AgentSession::new("pending-first").expect("first placeholder");
    let second = AgentSession::new("pending-second").expect("second placeholder");
    let first_registration = db
        .register_receiver_session(
            accepted.conversation_id(),
            &first,
            "first-instance",
            41,
            &scope,
        )
        .expect("register first placeholder");
    let second_registration = db
        .register_receiver_session(
            accepted.conversation_id(),
            &second,
            "second-instance",
            42,
            &scope,
        )
        .expect("register second placeholder");
    db.conn
        .execute(
            "UPDATE brain_sessions SET agent_session_id = 'actual-first'
             WHERE brain_instance_id = 'first-instance'",
            [],
        )
        .expect("simulate first lifecycle rotation");
    let crossed = ReceiverSessionAttribution::new(
        accepted.conversation_id(),
        first_registration.instance().to_owned(),
        second_registration.registered_session().clone(),
        scope,
    );

    assert!(!db
        .replace_receiver_binding_from_instance(&crossed, 1_100)
        .expect("reject crossed placeholder"));
    assert!(
        db.receiver_conversation(accepted.conversation_id())
            .expect("load conversation")
            .expect("durable conversation")
            .binding()
            .is_none()
    );
}

#[test]
fn native_binding_replacement_cannot_cross_same_actor_channel_conversations() {
    use crate::agent::{AgentSession, SessionScope};

    let db = Db::open_in_memory().expect("receiver state");
    let first_job = receiver_job_for(
        receiver_workspace_id(),
        crate::server::receiver::Channel::Email,
        None,
        100,
    );
    let second_job = receiver_job_for(
        receiver_workspace_id(),
        crate::server::receiver::Channel::Email,
        None,
        200,
    );
    let first_identity = ReceiverConversationIdentity::email(
        receiver_workspace_id(),
        receiver_user_id(),
        EmailLineage::verified("thread-first").expect("first lineage"),
    );
    let second_identity = ReceiverConversationIdentity::email(
        receiver_workspace_id(),
        receiver_user_id(),
        EmailLineage::verified("thread-second").expect("second lineage"),
    );
    let first = db
        .accept_receiver_job(&first_job, &first_identity)
        .expect("accept first conversation");
    let second = db
        .accept_receiver_job(&second_job, &second_identity)
        .expect("accept second conversation");
    let scope = SessionScope::new(
        crate::agent::AgentKind::OpenCode,
        receiver_workspace_id(),
        first_job.actor,
    );
    let placeholder = AgentSession::new("pending-first-conversation").expect("placeholder");
    let registration = db
        .register_receiver_session(
            first.conversation_id(),
            &placeholder,
            "first-instance",
            41,
            &scope,
        )
        .expect("register first conversation placeholder");
    db.conn
        .execute(
            "UPDATE brain_sessions SET agent_session_id = 'actual-first-conversation'
             WHERE brain_instance_id = 'first-instance'",
            [],
        )
        .expect("simulate first lifecycle rotation");
    let crossed = ReceiverSessionAttribution::new(
        second.conversation_id(),
        registration.instance().to_owned(),
        registration.registered_session().clone(),
        scope,
    );

    assert!(!db
        .replace_receiver_binding_from_instance(&crossed, 1_100)
        .expect("reject another logical conversation"));
    assert!(
        db.receiver_conversation(first.conversation_id())
            .expect("load first conversation")
            .expect("first conversation")
            .binding()
            .is_none()
    );
    assert!(
        db.receiver_conversation(second.conversation_id())
            .expect("load second conversation")
            .expect("second conversation")
            .binding()
            .is_none()
    );
}

struct CompletionFixture {
    db: Db,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    registration: ReceiverSessionAttribution,
    completed_session: crate::agent::AgentSession,
}

impl CompletionFixture {
    fn request(&self) -> ReceiverCompletionRequest<'_> {
        ReceiverCompletionRequest {
            job_id: self.job_id,
            token: self.token,
            owner: "owner",
            registration: &self.registration,
            completed_session: &self.completed_session,
            observed_at_unix_ms: 1_500,
            authorized_at_unix_ms: 1_500,
        }
    }
}

fn completion_fixture(state: ReceiverJobState) -> CompletionFixture {
    use crate::agent::{AgentKind, AgentSession, SessionScope};

    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let job = receiver_job(None, 100);
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");
    let scope = SessionScope::new(AgentKind::Codex, receiver_workspace_id(), job.actor);
    let placeholder = AgentSession::new("pending-completion").expect("placeholder");
    let registration = db
        .register_receiver_session(
            accepted.conversation_id(),
            &placeholder,
            "completion-instance",
            42,
            &scope,
        )
        .expect("register receiver session");
    db.claim_next_receiver_run("owner", 1_000, 2_000)
        .expect("claim receiver job")
        .expect("receiver claim");
    assert!(db
        .prepare_receiver_job_launch(accepted.job_id(), "owner", 1_100)
        .expect("prepare receiver launch"));
    let token = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job")
        .token();
    assert!(db
        .commit_receiver_job_launch(
            accepted.job_id(),
            "owner",
            &launch_observation(token, "completion-instance", "pending-completion", 1_200),
        )
        .expect("commit launch evidence"));
    if matches!(state, ReceiverJobState::Accepted | ReceiverJobState::Processing) {
        assert!(db
            .apply_receiver_observation(
                accepted.job_id(),
                "owner",
                &observation(
                    token,
                    "completion-instance",
                    "pending-completion",
                    ReceiverNonterminalObservationPhase::Accepted,
                    1,
                    1_300,
                ),
            )
            .expect("record accepted evidence"));
    }
    if state == ReceiverJobState::Processing {
        assert!(db
            .apply_receiver_observation(
                accepted.job_id(),
                "owner",
                &observation(
                    token,
                    "completion-instance",
                    "pending-completion",
                    ReceiverNonterminalObservationPhase::Progressing,
                    2,
                    1_400,
                ),
            )
            .expect("record progress evidence"));
    }
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load observed job")
            .expect("receiver job")
            .state(),
        state
    );
    let completed_session = AgentSession::new("completed-completion").expect("completed session");
    db.conn
        .execute(
            "UPDATE brain_sessions
             SET agent_session_id = ?1, completion_status = 'completed'
             WHERE brain_instance_id = 'completion-instance'",
            [completed_session.as_str()],
        )
        .expect("record exact completed native session");
    CompletionFixture {
        db,
        job_id: accepted.job_id(),
        token,
        registration,
        completed_session,
    }
}

#[test]
fn exact_completion_accepts_launched_accepted_and_processing_without_fabricating_evidence() {
    for (state, accepted_at, progressing_at) in [
        (ReceiverJobState::Launched, None, None),
        (ReceiverJobState::Accepted, Some(1_300), None),
        (ReceiverJobState::Processing, Some(1_300), Some(1_400)),
    ] {
        let fixture = completion_fixture(state);

        assert!(fixture
            .db
            .complete_receiver_job_with_binding(&fixture.request())
            .expect("complete exact receiver job"));

        let job = fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load completed job")
            .expect("receiver job");
        assert_eq!(job.state(), ReceiverJobState::Done);
        assert_eq!(job.completed_at_unix_ms(), Some(1_500));
        assert_eq!(job.accepted_at_unix_ms(), accepted_at);
        assert_eq!(job.progressing_at_unix_ms(), progressing_at);
    }
}

#[test]
fn exact_completion_rejects_a_wrong_durable_token() {
    let fixture = completion_fixture(ReceiverJobState::Launched);
    let wrong_token = ReceiverJobToken::parse("00000000-0000-4000-8000-000000000001")
        .expect("wrong token");
    let request = ReceiverCompletionRequest {
        token: wrong_token,
        ..fixture.request()
    };

    assert!(!fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("reject wrong token"));
}

#[test]
fn exact_completion_rejects_a_stale_owner() {
    let fixture = completion_fixture(ReceiverJobState::Launched);
    let request = ReceiverCompletionRequest {
        owner: "other-owner",
        ..fixture.request()
    };

    assert!(!fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("reject stale owner"));
}

#[test]
fn exact_completion_uses_fresh_authorization_time_for_lease_validation() {
    let fixture = completion_fixture(ReceiverJobState::Launched);
    let request = ReceiverCompletionRequest {
        authorized_at_unix_ms: 2_000,
        ..fixture.request()
    };

    assert!(!fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("reject expired lease despite backdated evidence"));
}

#[test]
fn exact_completion_rejects_a_wrong_instance() {
    let fixture = completion_fixture(ReceiverJobState::Launched);
    let wrong_registration = ReceiverSessionAttribution::new(
        fixture.registration.conversation_id(),
        "other-instance".to_owned(),
        fixture.registration.registered_session().clone(),
        fixture.registration.scope().clone(),
    );
    let request = ReceiverCompletionRequest {
        registration: &wrong_registration,
        ..fixture.request()
    };

    assert!(!fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("reject wrong instance"));
}

#[test]
fn exact_completion_rejects_a_wrong_native_session() {
    use crate::agent::AgentSession;

    let fixture = completion_fixture(ReceiverJobState::Launched);
    let wrong_session = AgentSession::new("other-completed-session").expect("wrong session");
    let request = ReceiverCompletionRequest {
        completed_session: &wrong_session,
        ..fixture.request()
    };

    assert!(!fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("reject wrong native session"));
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load rejected job")
            .expect("receiver job")
            .state(),
        ReceiverJobState::Launched
    );
}

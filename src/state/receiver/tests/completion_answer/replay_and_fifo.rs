#[test]
fn exact_completion_conflict_rolls_back_without_changing_the_existing_answer() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Launched);
    let request = fixture.request();
    fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record exact answer")
        .expect("exact answer owner");
    let before = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    let conflicting = ReceiverCompletionRequest {
        answer: "different assistant answer",
        ..request
    };

    let error = fixture
        .db
        .complete_receiver_job_with_binding(&conflicting)
        .expect_err("reject conflicting answer");

    assert!(
        error.to_string() == "receiver completion conflicts with durable answer",
        "conflicting completion returned the wrong typed error"
    );
    let retained_transcript = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("reload conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    assert!(
        private_text_proof(&retained_transcript) == private_text_proof(&before),
        "conflicting completion changed the durable transcript proof"
    );
    assert_eq!(
        fixture
            .db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM receiver_deliveries WHERE job_id = ?1",
                [fixture.job_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count durable answers"),
        1
    );
}

#[test]
fn exact_completion_replay_uses_immutable_evidence_after_later_turn_and_binding_change() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    let request = fixture.request();
    let first = fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record exact answer")
        .expect("exact answer owner");
    let first_transcript = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load first conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    let later_transcript = format!("{first_transcript}\n\n## Authenticated user\n\nLater turn");
    let later_binding = ReceiverSessionBinding::new(
        crate::agent::AgentKind::OpenCode,
        "later-native-session",
    )
    .expect("later binding");
    assert!(
        fixture
            .db
            .update_receiver_conversation(
                fixture.registration.conversation_id(),
                &later_transcript,
                Some(&later_binding),
                1_600,
            )
            .expect("advance conversation after first answer")
    );

    let replay = fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("match the immutable first completion")
        .expect("existing exact answer");

    assert!(!replay.newly_recorded());
    assert_eq!(replay.delivery_id(), first.delivery_id());
    let retained = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("reload later conversation")
        .expect("durable conversation");
    assert!(
        private_text_proof(retained.transcript_markdown()) == private_text_proof(&later_transcript),
        "completion replay changed the later transcript proof"
    );
    assert_eq!(retained.binding(), Some(&later_binding));
}

#[test]
fn exact_completion_replay_rejects_a_different_registered_session() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    let request = fixture.request();
    fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record exact answer")
        .expect("exact answer owner");
    let wrong_registered = crate::agent::AgentSession::new("wrong-registered-session")
        .expect("wrong registered session");
    let crossed = ReceiverSessionAttribution::new(
        fixture.registration.conversation_id(),
        fixture.registration.instance().to_owned(),
        wrong_registered,
        fixture.registration.scope().clone(),
    );
    let conflicting = ReceiverCompletionRequest {
        registration: &crossed,
        ..request
    };

    let error = fixture
        .db
        .complete_receiver_job_with_binding(&conflicting)
        .expect_err("reject a crossed registered session");

    assert_eq!(error.to_string(), "receiver completion conflicts with durable answer");
}

#[test]
fn answer_ready_releases_agent_lane_for_the_next_queued_job() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Accepted);
    let next = receiver_job(None, 1_600);
    let identity =
        ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted_next = fixture
        .db
        .accept_receiver_job(&next, &identity)
        .expect("accept next job");

    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    let next_claim = fixture
        .db
        .claim_next_receiver_run("next-owner", 1_600, 2_600)
        .expect("claim next job")
        .expect("next queued job is independent of delivery");

    assert_eq!(next_claim.job().id(), accepted_next.job_id());
    assert_eq!(next_claim.job().state(), ReceiverJobState::Claimed);
}

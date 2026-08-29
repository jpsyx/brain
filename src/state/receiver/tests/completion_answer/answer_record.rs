#[test]
fn portable_transcript_appends_exact_markdown_escaped_turn_once() {
    let prior = "# Prior\n\nExisting context without a final newline";
    let inbound = "User text\n```\n## forged heading\n<script>private</script>";
    let answer = "Assistant text\n````\n## another heading\n<answer>exact</answer>\n";

    let appended = render_receiver_transcript(prior, inbound, answer);
    let duplicate = render_receiver_transcript(&appended, inbound, answer);

    assert!(appended.starts_with(prior));
    assert!(appended.contains(inbound));
    assert!(appended.contains(answer));
    assert!(
        appended.matches("## Authenticated user").count() == 1,
        "transcript had the wrong authenticated-user heading count"
    );
    assert!(
        appended.matches("## Assistant").count() == 1,
        "transcript had the wrong assistant heading count"
    );
    assert!(
        private_text_proof(&duplicate) != private_text_proof(&appended),
        "the pure renderer exposes append semantics"
    );
    assert!(
        receiver_transcript_has_exact_turn(&appended, inbound, answer),
        "the stored transcript must recognize an exact duplicate turn"
    );
    assert!(!receiver_transcript_has_exact_turn(
        &appended,
        inbound,
        "conflicting answer"
    ));
}

#[test]
fn exact_completion_atomically_records_answer_ready_transcript_binding_and_outbox_once() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .update_receiver_conversation(
            fixture.registration.conversation_id(),
            "# Prior\n\nDurable context",
            None,
            1_450,
        )
        .expect("seed prior transcript");
    let request = fixture.request();

    let first = fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record exact answer")
        .expect("exact answer owner");
    let first_transcript = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    let second = fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("replay exact answer")
        .expect("existing exact answer");

    assert!(first.newly_recorded());
    assert!(!second.newly_recorded());
    assert_eq!(first.delivery_id(), second.delivery_id());
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load answer-ready job")
            .expect("durable job")
            .state(),
        ReceiverJobState::AnswerReady
    );
    let reloaded_transcript = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("reload conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    assert!(
        private_text_proof(&reloaded_transcript) == private_text_proof(&first_transcript),
        "exact completion replay changed the durable transcript proof"
    );
    assert!(
        first_transcript.matches("## Authenticated user").count() == 1,
        "durable transcript had the wrong authenticated-user heading count"
    );
    assert!(
        first_transcript.matches("## Assistant").count() == 1,
        "durable transcript had the wrong assistant heading count"
    );
    let (delivery_count, delivery_state, claim_owner): (i64, String, Option<String>) = fixture
        .db
        .conn
        .query_row(
            "SELECT COUNT(*), MIN(delivery.state), MIN(job.claim_owner)
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             WHERE delivery.job_id = ?1 AND delivery.response_kind = 'final-answer'",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load answer outbox");
    assert_eq!(delivery_count, 1);
    assert_eq!(delivery_state, "ready");
    assert_eq!(claim_owner, None);
    let cleanup: (i64, String, String, i64, i64) = fixture
        .db
        .conn
        .query_row(
            "SELECT COUNT(*), MIN(brain_instance_id), MIN(registered_session_id),
                    MIN(session_released), MIN(artifacts_removed)
             FROM receiver_answer_cleanups WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("load exact post-commit cleanup");
    assert_eq!(cleanup.0, 1);
    assert_eq!(cleanup.1, fixture.registration.instance());
    assert_eq!(
        cleanup.2,
        fixture.registration.registered_session().as_str()
    );
    assert_eq!((cleanup.3, cleanup.4), (0, 0));
}

#[test]
fn completion_uses_the_sender_frozen_with_the_authenticated_inbound_job() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    let request = fixture.request();

    fixture
        .db
        .complete_receiver_job_with_binding(&request)
        .expect("record answer after machine sender changed")
        .expect("exact answer owner");
    let claim = fixture
        .db
        .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
        .expect("claim frozen delivery")
        .expect("frozen delivery exists");

    assert!(
        claim
            .envelope()
            .sms()
            .is_some_and(|sms| sms.sender() == "+12125550100"),
        "delivery did not use the sender frozen at inbound acceptance"
    );
}

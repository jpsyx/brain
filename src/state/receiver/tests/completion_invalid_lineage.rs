fn replace_persisted_inbound(
    fixture: &super::binding::CompletionFixture,
    mutate: impl FnOnce(&mut crate::server::receiver::InboundJob),
) {
    let inbound_json: String = fixture
        .db
        .conn
        .query_row(
            "SELECT inbound_json FROM receiver_jobs WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| row.get(0),
        )
        .expect("load accepted inbound job");
    let mut inbound: crate::server::receiver::InboundJob =
        serde_json::from_str(&inbound_json).expect("decode accepted inbound job");
    mutate(&mut inbound);
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET inbound_json = ?2 WHERE job_id = ?1",
            rusqlite::params![
                fixture.job_id.to_string(),
                serde_json::to_string(&inbound).expect("encode accepted inbound job"),
            ],
        )
        .expect("replace accepted inbound job");
}

fn assert_invalid_lineage_terminal(fixture: &super::binding::CompletionFixture) {
    let outcome = fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("terminalize invalid accepted reply metadata")
        .expect("exact completion authority");
    assert!(outcome.newly_recorded());
    let (job_state, delivery_state, category, cleanups): (String, String, Option<String>, i64) =
        fixture
            .db
            .conn
            .query_row(
                "SELECT job.state, delivery.state, delivery.error_category,
                        (SELECT COUNT(*) FROM receiver_answer_cleanups AS cleanup
                         WHERE cleanup.job_id = job.job_id)
                 FROM receiver_jobs AS job
                 JOIN receiver_deliveries AS delivery ON delivery.job_id = job.job_id
                 WHERE job.job_id = ?1",
                [fixture.job_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load terminal invalid-lineage outcome");
    assert_eq!(job_state, "failed");
    assert_eq!(delivery_state, "failed");
    assert_eq!(category.as_deref(), Some("invalid-request"));
    assert_eq!(cleanups, 1);
    assert!(
        fixture
            .db
            .claim_next_receiver_delivery("delivery-owner", 2_000, 32_000)
            .expect("inspect invalid-lineage delivery lane")
            .is_none(),
        "invalid accepted reply metadata entered provider delivery"
    );
}

#[test]
fn completion_terminalizes_every_invalid_accepted_recipient_and_email_lineage_shape() {
    for case in 0..4 {
        let fixture = if case == 3 {
            super::binding::completion_fixture(ReceiverJobState::Processing)
        } else {
            super::binding::email_completion_fixture_in(
                Db::open_in_memory().expect("email receiver state"),
                ReceiverJobState::Processing,
            )
        };
        replace_persisted_inbound(&fixture, |inbound| match case {
            0 => inbound.response_email = Some("not-a-mailbox".to_owned()),
            1 => {
                inbound.response_email = Some("recipient@example.test".to_owned());
                inbound.email_reply = Some(crate::server::receiver::EmailReplyContext {
                    provider_email_id: " ".to_owned(),
                    subject: "accepted subject".to_owned(),
                    message_id: Some("provider-message".to_owned()),
                });
            }
            2 => {
                inbound.response_email = Some("recipient@example.test".to_owned());
                inbound.email_reply = Some(crate::server::receiver::EmailReplyContext {
                    provider_email_id: "provider-email".to_owned(),
                    subject: "accepted subject".to_owned(),
                    message_id: Some(String::new()),
                });
            }
            3 => inbound.authenticated_sender = "invalid-sms-recipient".to_owned(),
            _ => unreachable!("fixed invalid-lineage case"),
        });

        assert_invalid_lineage_terminal(&fixture);
    }
}

#[test]
fn blank_resend_message_id_is_terminal_across_restart_replay_and_fifo_advancement() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let workspace = receiver_workspace_id().to_string();
    let actor = receiver_user_id();
    let fixture = super::binding::email_completion_fixture_in(
        Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
            .expect("file-backed receiver state"),
        ReceiverJobState::Processing,
    );
    replace_persisted_inbound(&fixture, |inbound| {
        inbound.response_email = Some("recipient@example.test".to_owned());
        inbound.email_reply = Some(crate::server::receiver::EmailReplyContext {
            provider_email_id: "resend-email-id".to_owned(),
            subject: "accepted subject".to_owned(),
            message_id: Some(String::new()),
        });
    });
    assert_invalid_lineage_terminal(&fixture);
    let first_delivery: String = fixture
        .db
        .conn
        .query_row(
            "SELECT delivery_id FROM receiver_deliveries WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| row.get(0),
        )
        .expect("load first terminal delivery");
    let first_transcript = fixture
        .db
        .receiver_conversation(fixture.registration.conversation_id())
        .expect("load terminal conversation")
        .expect("terminal conversation")
        .transcript_markdown()
        .to_owned();
    let job_id = fixture.job_id;
    let token = fixture.token;
    let registration = fixture.registration.clone();
    let completed_session = fixture.completed_session.clone();
    drop(fixture);

    let reopened = Db::open_path_with_legacy_identity(&path, &workspace, actor.as_str())
        .expect("reopen invalid-lineage receiver state");
    let replay = reopened
        .complete_receiver_job_with_binding(&ReceiverCompletionRequest {
            job_id,
            token,
            owner: "owner",
            registration: &registration,
            completed_session: &completed_session,
            answer: "exact assistant answer",
            observed_at_unix_ms: 1_500,
            authorized_at_unix_ms: 1_500,
        })
        .expect("replay terminal invalid-lineage completion")
        .expect("existing terminal invalid-lineage completion");
    assert!(!replay.newly_recorded(), "completed AI work was rerun");
    assert_eq!(replay.delivery_id().to_string(), first_delivery);
    let retained_transcript = reopened
        .receiver_conversation(registration.conversation_id())
        .expect("reload terminal conversation")
        .expect("terminal conversation")
        .transcript_markdown()
        .to_owned();
    assert_eq!(
        private_text_proof(&retained_transcript),
        private_text_proof(&first_transcript),
        "duplicate replay appended completed AI work"
    );
    assert_eq!(
        reopened
            .conn
            .query_row(
                "SELECT COUNT(*) FROM receiver_deliveries WHERE job_id = ?1",
                [job_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count terminal deliveries"),
        1
    );

    let next = receiver_job_for(
        receiver_workspace_id(),
        crate::server::receiver::Channel::Email,
        Some("later-provider-email"),
        1_600,
    );
    let identity = ReceiverConversationIdentity::email(
        receiver_workspace_id(),
        receiver_user_id(),
        EmailLineage::verified("provider-thread").expect("email lineage"),
    );
    let accepted = reopened
        .accept_receiver_job(&next, &identity)
        .expect("accept later email job");
    let claimed = reopened
        .claim_next_receiver_run("later-owner", 1_700, 2_700)
        .expect("claim after terminal invalid lineage")
        .expect("later FIFO job advances");
    assert_eq!(claimed.job().id(), accepted.job_id());
}

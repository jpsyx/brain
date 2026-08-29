use super::receiver_durable_answer_cleanup::{
    answer_fixture, completion_evidence_count, delivery_count, job_state,
};
use super::receiver_durable_delivery::ScriptedDeliveryExecution;
use super::receiver_durable_delivery_no_io::{
    delivery_lifecycle_proof, stage_later_delivery, stage_sms_delivery,
};
use super::receiver_durable_support::{ReceiverClock, publish_valid_completion};
use super::*;
use crate::state::ReceiverJobState;

#[test]
fn lost_acknowledged_resend_commit_replays_safely_and_advances_the_queue() {
    post_acknowledgement_commit_failure_case(false);
}

#[test]
fn lost_acknowledged_twilio_commit_becomes_ambiguous_and_advances_the_queue() {
    post_acknowledgement_commit_failure_case(true);
}

fn post_acknowledgement_commit_failure_case(use_twilio: bool) {
    let (temporary, mut app, db, first, second, _transport) = answer_fixture();
    let clock = ReceiverClock::at_unix_ms(10_000);
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let execution = ScriptedDeliveryExecution::acknowledged();
    app.services
        .replace_receiver_delivery_execution(Box::new(execution));
    publish_valid_completion(&app, "private acknowledgement before commit failure");
    app.tick_receiver();
    if use_twilio {
        stage_sms_delivery(&app, first.job_id());
    }
    app.tick_receiver();

    let fault = rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for acknowledged commit fault");
    fault
        .execute_batch(
            "CREATE TRIGGER fail_acknowledged_result_commit
             BEFORE UPDATE OF state ON receiver_deliveries
             WHEN OLD.state = 'delivering' AND NEW.state != 'delivering'
             BEGIN SELECT RAISE(FAIL, 'injected acknowledged result commit failure'); END;",
        )
        .expect("install acknowledged result commit fault");
    app.tick_receiver();
    assert!(
        delivery_lifecycle_proof(&app, first.job_id())
            == ("delivering".to_owned(), 1, 1, None, None),
        "failed acknowledged result commit crossed its durable fault boundary"
    );
    stage_later_delivery(&app, first.job_id(), second.job_id());
    fault
        .execute_batch("DROP TRIGGER fail_acknowledged_result_commit;")
        .expect("remove acknowledged result commit fault");
    drop(fault);
    drop(app);
    clock.advance(std::time::Duration::from_secs(61));

    let cli = Cli::parse_from(["tasks"]);
    let mut reopened = test_app(&temporary, &cli, AgentKind::Claude);
    reopened.receiver.record_intent(true);
    reopened
        .services
        .replace_receiver_sync_runtime(Box::new(clock));
    let resumed_execution = ScriptedDeliveryExecution::acknowledged();
    reopened
        .services
        .replace_receiver_delivery_execution(Box::new(resumed_execution));
    reopened.tick_receiver();

    let first_lifecycle = delivery_lifecycle_proof(&reopened, first.job_id());
    if use_twilio {
        assert!(
            first_lifecycle
                == (
                    "ambiguous".to_owned(),
                    1,
                    0,
                    None,
                    Some("provider-acceptance-unknown".to_owned()),
                ),
            "expired acknowledged Twilio result was not conservatively ambiguous"
        );
    } else {
        assert!(
            first_lifecycle
                == (
                    "retrying".to_owned(),
                    1,
                    0,
                    Some("transport-unavailable".to_owned()),
                    None,
                ),
            "expired acknowledged Resend result was not safely replayable"
        );
    }
    let later_state = job_state(&db, second.job_id());
    assert!(
        later_state == ReceiverJobState::Delivering,
        "post-acknowledgement recovery did not advance the later response queue"
    );
    assert!(
        delivery_count(&reopened, first.job_id()) == 1,
        "post-acknowledgement recovery duplicated final-answer work"
    );
    assert!(
        completion_evidence_count(&reopened, first.job_id()) == 1,
        "post-acknowledgement recovery duplicated agent completion evidence"
    );
    reopened.tick_receiver();
    assert!(
        job_state(&db, second.job_id()) == ReceiverJobState::Done,
        "later response did not complete after post-acknowledgement recovery"
    );
}

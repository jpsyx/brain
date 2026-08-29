use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use super::receiver_durable_answer_cleanup::{answer_fixture, job_state};
use super::receiver_durable_support::{ReceiverClock, publish_valid_completion};
use super::*;

use crate::server::delivery::{
    ReceiverDeliveryExecution, ReceiverDeliveryExecutionPoll, ReceiverDeliveryStart,
};
use crate::state::{
    ReceiverDeliveryClaim, ReceiverJobState, ReceiverProviderReference, ReceiverProviderResultClass,
};

#[derive(Clone)]
pub(super) struct ScriptedDeliveryExecution {
    state: Arc<Mutex<ScriptedDeliveryState>>,
    result: ReceiverProviderResultClass,
    publication_fails: bool,
}

#[derive(Default)]
struct ScriptedDeliveryState {
    reserved: Vec<ReceiverDeliveryClaim>,
    results: VecDeque<(ReceiverDeliveryClaim, ReceiverProviderResultClass)>,
    cancellations: usize,
}

impl ScriptedDeliveryExecution {
    pub(super) fn acknowledged() -> Self {
        Self {
            state: Arc::new(Mutex::new(ScriptedDeliveryState::default())),
            result: ReceiverProviderResultClass::Acknowledged(
                ReceiverProviderReference::parse("10000000-0000-4000-8000-000000000001")
                    .expect("provider reference"),
            ),
            publication_fails: false,
        }
    }

    fn publication_failure() -> Self {
        let mut execution = Self::acknowledged();
        execution.publication_fails = true;
        execution
    }

    pub(super) fn reservation_count(&self) -> usize {
        self.state
            .lock()
            .expect("scripted delivery state")
            .reserved
            .len()
    }

    fn cancellation_count(&self) -> usize {
        self.state
            .lock()
            .expect("scripted delivery state")
            .cancellations
    }
}

struct ScriptedDeliveryStart {
    state: Arc<Mutex<ScriptedDeliveryState>>,
    claim: ReceiverDeliveryClaim,
    result: ReceiverProviderResultClass,
    publication_fails: bool,
}

impl ReceiverDeliveryStart for ScriptedDeliveryStart {
    fn attempt_kind(&self) -> crate::server::delivery::ReceiverDeliveryAttemptKind {
        crate::server::delivery::ReceiverDeliveryAttemptKind::ProviderIo
    }

    fn start(self: Box<Self>) -> anyhow::Result<()> {
        if self.publication_fails {
            anyhow::bail!("scripted worker disconnected before publication");
        }
        self.state
            .lock()
            .expect("scripted delivery state")
            .results
            .push_back((self.claim, self.result));
        Ok(())
    }
}

impl ReceiverDeliveryExecution for ScriptedDeliveryExecution {
    fn reserve(
        &mut self,
        _command: crate::workspace::CommandContext,
        claim: ReceiverDeliveryClaim,
    ) -> Result<Box<dyn ReceiverDeliveryStart>, Box<ReceiverDeliveryClaim>> {
        self.state
            .lock()
            .expect("scripted delivery state")
            .reserved
            .push(claim.clone());
        Ok(Box::new(ScriptedDeliveryStart {
            state: self.state.clone(),
            claim,
            result: self.result.clone(),
            publication_fails: self.publication_fails,
        }))
    }

    fn poll(&self) -> ReceiverDeliveryExecutionPoll {
        self.state
            .lock()
            .expect("scripted delivery state")
            .results
            .pop_front()
            .map_or(ReceiverDeliveryExecutionPoll::Pending, |(claim, result)| {
                ReceiverDeliveryExecutionPoll::Ready {
                    claim: Box::new(claim),
                    result,
                    attempt_kind: crate::server::delivery::ReceiverDeliveryAttemptKind::ProviderIo,
                }
            })
    }

    fn cancel(&mut self) {
        self.state
            .lock()
            .expect("scripted delivery state")
            .cancellations += 1;
    }
}

#[test]
fn orderly_app_shutdown_cancels_provider_work_for_restart_reconciliation() {
    let (_temporary, mut app, _db, _first, _second, _transport) = answer_fixture();
    let execution = ScriptedDeliveryExecution::acknowledged();
    app.services
        .replace_receiver_delivery_execution(Box::new(execution.clone()));

    app.shutdown_receiver_runtime();

    assert_eq!(execution.cancellation_count(), 1);
}

#[test]
fn app_tick_claims_starts_and_applies_final_answer_delivery_independently() {
    let (_temporary, mut app, db, first, second, _transport) = answer_fixture();
    let execution = ScriptedDeliveryExecution::acknowledged();
    app.services
        .replace_receiver_delivery_execution(Box::new(execution.clone()));
    publish_valid_completion(&app, "immutable delivered answer");

    app.tick_receiver();
    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::AnswerReady,
        "delivery startup changed the answer-ready state"
    );
    app.tick_receiver();
    assert_eq!(execution.reservation_count(), 1);
    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::Delivering,
        "delivery did not enter provider IO"
    );
    app.tick_receiver();

    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::Done,
        "acknowledged delivery did not finish"
    );
    assert!(
        job_state(&db, second.job_id()) != ReceiverJobState::Delivering,
        "delivery ownership is independent from the later agent job"
    );
}

#[test]
fn app_requeues_an_attempt_when_publication_proves_it_was_never_sent() {
    let (_temporary, mut app, db, first, _second, _transport) = answer_fixture();
    let execution = ScriptedDeliveryExecution::publication_failure();
    app.services
        .replace_receiver_delivery_execution(Box::new(execution.clone()));
    publish_valid_completion(&app, "immutable unsent answer");

    app.tick_receiver();
    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::AnswerReady,
        "unsent publication changed the answer-ready state"
    );
    app.tick_receiver();

    assert_eq!(execution.reservation_count(), 1);
    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::AnswerReady,
        "publication failure did not restore answer-ready"
    );

    app.tick_receiver();
    assert_eq!(
        execution.reservation_count(),
        2,
        "unsent answer remains due"
    );
}

#[test]
fn worker_construction_failure_becomes_one_bounded_durable_attempt() {
    let (_temporary, mut app, db, first, _second, _transport) = answer_fixture();
    publish_valid_completion(&app, "immutable answer without a delivery worker");

    app.tick_receiver();
    app.tick_receiver();
    app.tick_receiver();

    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::Retrying,
        "worker construction failure did not schedule retry"
    );
    let delivery: (String, i64, Option<i64>, Option<String>) =
        rusqlite::Connection::open(app.context.state_db_path())
            .expect("open receiver state after worker construction failure")
            .query_row(
                "SELECT state, attempt_count, retry_at_unix_ms, error_category
                 FROM receiver_deliveries WHERE job_id = ?1",
                [first.job_id().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load bounded worker construction result");
    assert!(
        delivery
            == (
                "retrying".to_owned(),
                1,
                delivery.2,
                Some("transport-unavailable".to_owned()),
            )
            && delivery.2.is_some(),
        "worker construction failure did not consume one bounded retry attempt"
    );

    app.tick_receiver();

    assert!(
        rusqlite::Connection::open(app.context.state_db_path())
            .expect("reopen receiver state after bounded retry")
            .query_row(
                "SELECT attempt_count FROM receiver_deliveries WHERE job_id = ?1",
                [first.job_id().to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("load stable bounded attempt count")
            == 1,
        "worker construction retry was reclaimed immediately"
    );
}

#[test]
fn fresh_app_reconciles_expired_resend_io_before_new_delivery_work() {
    let (temporary, mut app, db, first, _second, _transport) = answer_fixture();
    let clock = ReceiverClock::at_unix_ms(10_000);
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    publish_valid_completion(&app, "answer survives fresh app reconciliation");
    app.tick_receiver();
    let claim = db
        .claim_next_receiver_delivery("departed-owner", 10_000, 11_000)
        .expect("claim delivery before crash")
        .expect("answer delivery");
    assert!(
        db.mark_receiver_delivery_io_started(&claim, 10_100)
            .expect("record provider IO before crash")
    );
    drop(app);
    clock.advance(std::time::Duration::from_secs(2));

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(clock));
    let execution = ScriptedDeliveryExecution::acknowledged();
    restarted
        .services
        .replace_receiver_delivery_execution(Box::new(execution.clone()));

    restarted.tick_receiver();

    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::Retrying,
        "expired provider IO did not schedule retry"
    );
    assert_eq!(execution.reservation_count(), 0);
}

#[test]
fn fresh_app_terminalizes_due_resend_retry_after_idempotency_window_before_provider_io() {
    let (temporary, mut app, db, first, _second, _transport) = answer_fixture();
    publish_valid_completion(&app, "answer survives an offline retry window");
    app.tick_receiver();
    let email_envelope = serde_json::json!({
        "channel": "email",
        "value": {
            "sender": "brain@example.test",
            "recipients": ["member@example.test"],
            "subject": "Re: Frozen subject",
            "text": "frozen private answer",
            "html": "<p>frozen private answer</p>",
            "in_reply_to": "<message@example.test>",
            "references": "<message@example.test>",
            "provider_email_id": "provider-email-id"
        }
    })
    .to_string();
    let connection = rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for retry fixture");
    connection
        .execute(
            "UPDATE receiver_deliveries
             SET envelope_json = ?2, state = 'retrying', attempt_count = 1,
                 retry_at_unix_ms = 61_000, first_attempt_at_unix_ms = 1_000,
                 error_category = 'transport-unavailable'
             WHERE job_id = ?1",
            rusqlite::params![first.job_id().to_string(), email_envelope],
        )
        .expect("stage due Resend retry");
    connection
        .execute(
            "UPDATE receiver_jobs SET state = 'retrying' WHERE job_id = ?1",
            [first.job_id().to_string()],
        )
        .expect("stage retrying answer job");
    drop(connection);
    drop(app);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(ReceiverClock::at_unix_ms(86_401_001)));
    let execution = ScriptedDeliveryExecution::acknowledged();
    restarted
        .services
        .replace_receiver_delivery_execution(Box::new(execution.clone()));

    restarted.tick_receiver();

    assert_eq!(execution.reservation_count(), 0);
    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::Failed,
        "expired idempotency window did not fail delivery"
    );
    let terminal: (String, Option<String>) =
        rusqlite::Connection::open(restarted.context.state_db_path())
            .expect("open receiver state after retry reconciliation")
            .query_row(
                "SELECT state, ambiguity_reason FROM receiver_deliveries WHERE job_id = ?1",
                [first.job_id().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load terminalized retry");
    assert!(
        terminal
            == (
                "ambiguous".to_owned(),
                Some("idempotency-window-expired".to_owned())
            ),
        "expired Resend retry did not terminalize with the stable ambiguity category"
    );
}

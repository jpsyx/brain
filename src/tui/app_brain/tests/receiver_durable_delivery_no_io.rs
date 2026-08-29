use std::sync::{Arc, Mutex};

use super::receiver_durable_answer_cleanup::{answer_fixture, job_state};
use super::receiver_durable_support::{ReceiverClock, publish_valid_completion};
use super::*;
use crate::server::delivery::{
    ReceiverDeliveryExecution, ReceiverDeliveryExecutionPoll, ReceiverDeliveryStart,
};
use crate::state::{ReceiverDeliveryClaim, ReceiverJobState, ReceiverProviderResultClass};

#[derive(Clone, Default)]
struct DelayedNoProviderIoExecution {
    state: Arc<Mutex<DelayedNoProviderIoState>>,
}

#[derive(Default)]
struct DelayedNoProviderIoState {
    claim: Option<ReceiverDeliveryClaim>,
    ready: bool,
}

impl DelayedNoProviderIoExecution {
    fn make_ready(&self) {
        self.state.lock().expect("delayed no-IO state").ready = true;
    }
}

struct DelayedNoProviderIoStart {
    state: Arc<Mutex<DelayedNoProviderIoState>>,
    claim: ReceiverDeliveryClaim,
}

impl ReceiverDeliveryStart for DelayedNoProviderIoStart {
    fn attempt_kind(&self) -> crate::server::delivery::ReceiverDeliveryAttemptKind {
        crate::server::delivery::ReceiverDeliveryAttemptKind::NoProviderIo
    }

    fn start(self: Box<Self>) -> anyhow::Result<()> {
        self.state.lock().expect("delayed no-IO state").claim = Some(self.claim);
        Ok(())
    }
}

impl ReceiverDeliveryExecution for DelayedNoProviderIoExecution {
    fn reserve(
        &mut self,
        _command: crate::workspace::CommandContext,
        claim: ReceiverDeliveryClaim,
    ) -> Result<Box<dyn ReceiverDeliveryStart>, Box<ReceiverDeliveryClaim>> {
        Ok(Box::new(DelayedNoProviderIoStart {
            state: self.state.clone(),
            claim,
        }))
    }

    fn poll(&self) -> ReceiverDeliveryExecutionPoll {
        let mut state = self.state.lock().expect("delayed no-IO state");
        if !state.ready {
            return ReceiverDeliveryExecutionPoll::Pending;
        }
        state
            .claim
            .take()
            .map_or(ReceiverDeliveryExecutionPoll::Pending, |claim| {
                ReceiverDeliveryExecutionPoll::Ready {
                    claim: Box::new(claim),
                    result: ReceiverProviderResultClass::DefinitelyNotAccepted(
                        crate::state::ReceiverDeliveryErrorCategory::TransportUnavailable,
                    ),
                    attempt_kind:
                        crate::server::delivery::ReceiverDeliveryAttemptKind::NoProviderIo,
                }
            })
    }

    fn cancel(&mut self) {}
}

#[test]
fn delayed_worker_construction_failure_never_crosses_the_provider_io_boundary() {
    let (_temporary, mut app, db, first, second, _transport) = answer_fixture();
    let clock = ReceiverClock::at_unix_ms(10_000);
    app.services.replace_receiver_sync_runtime(Box::new(clock));
    let execution = DelayedNoProviderIoExecution::default();
    app.services
        .replace_receiver_delivery_execution(Box::new(execution.clone()));
    publish_valid_completion(&app, "private delayed worker failure answer");

    app.tick_receiver();
    stage_later_delivery(&app, first.job_id(), second.job_id());
    app.tick_receiver();

    let provider_io_started = rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state during delayed worker construction failure")
        .query_row(
            "SELECT provider_io_started FROM receiver_deliveries WHERE job_id = ?1",
            [first.job_id().to_string()],
            |row| row.get::<_, i64>(0),
        )
        .expect("load provider IO boundary");
    assert!(
        provider_io_started == 0,
        "worker construction failure falsely crossed the provider IO boundary"
    );

    execution.make_ready();
    app.tick_receiver();
    let durable = rusqlite::Connection::open(app.context.state_db_path())
        .expect("reopen receiver state after delayed worker construction failure")
        .query_row(
            "SELECT state, attempt_count, provider_io_started, error_category
             FROM receiver_deliveries WHERE job_id = ?1",
            [first.job_id().to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .expect("load durable no-IO result");
    assert!(
        durable
            == (
                "retrying".to_owned(),
                1,
                0,
                Some("transport-unavailable".to_owned()),
            ),
        "worker construction failure was not durably classified without provider IO"
    );
    assert!(
        job_state(&db, second.job_id()) == ReceiverJobState::Delivering,
        "bounded no-IO failure did not advance the later response queue"
    );
}

#[test]
fn expired_delayed_worker_construction_failure_remains_safe_to_retry() {
    let (temporary, mut app, db, first, _second, _transport) = answer_fixture();
    let clock = ReceiverClock::at_unix_ms(10_000);
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let execution = DelayedNoProviderIoExecution::default();
    app.services
        .replace_receiver_delivery_execution(Box::new(execution.clone()));
    publish_valid_completion(&app, "private expired local failure answer");
    app.tick_receiver();
    stage_sms_delivery(&app, first.job_id());
    app.tick_receiver();

    clock.advance(std::time::Duration::from_secs(61));
    execution.make_ready();
    app.tick_receiver();
    app.tick_receiver();

    let durable = delivery_lifecycle_proof(&app, first.job_id());
    assert!(
        durable
            == (
                "retrying".to_owned(),
                1,
                0,
                Some("transport-unavailable".to_owned()),
                None,
            ),
        "expired local failure became ambiguous instead of a typed retry"
    );
    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::Retrying,
        "expired local failure did not leave the job retrying"
    );
    drop(app);

    let cli = Cli::parse_from(["tasks"]);
    let reopened = test_app(&temporary, &cli, AgentKind::Claude);
    let reopened_db =
        crate::state::Db::open(reopened.context.workspace()).expect("reopen receiver state DB");
    assert!(
        job_state(&reopened_db, first.job_id()) == ReceiverJobState::Retrying,
        "reopen lost the no-IO retry classification"
    );
}

#[test]
fn failed_no_io_result_commit_reopens_without_twilio_ambiguity() {
    let (temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let clock = ReceiverClock::at_unix_ms(10_000);
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let execution = DelayedNoProviderIoExecution::default();
    app.services
        .replace_receiver_delivery_execution(Box::new(execution.clone()));
    publish_valid_completion(&app, "private no-IO commit failure answer");
    app.tick_receiver();
    stage_sms_delivery(&app, first.job_id());
    app.tick_receiver();

    let fault = rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for commit fault");
    fault
        .execute_batch(
            "CREATE TRIGGER fail_no_io_result_commit
             BEFORE UPDATE OF attempt_count ON receiver_deliveries
             WHEN NEW.attempt_count > OLD.attempt_count
             BEGIN SELECT RAISE(FAIL, 'injected no-IO result commit failure'); END;",
        )
        .expect("install no-IO commit fault");
    execution.make_ready();
    app.tick_receiver();
    assert!(
        delivery_lifecycle_proof(&app, first.job_id())
            == ("delivering".to_owned(), 0, 0, None, None),
        "failed no-IO result commit mutated durable delivery state"
    );
    fault
        .execute_batch("DROP TRIGGER fail_no_io_result_commit;")
        .expect("remove no-IO commit fault");
    drop(fault);
    drop(app);
    clock.advance(std::time::Duration::from_secs(61));

    let cli = Cli::parse_from(["tasks"]);
    let mut reopened = test_app(&temporary, &cli, AgentKind::Claude);
    reopened.receiver.record_intent(true);
    reopened
        .services
        .replace_receiver_sync_runtime(Box::new(clock));
    reopened.tick_receiver();
    reopened.tick_receiver();

    assert!(
        delivery_lifecycle_proof(&reopened, first.job_id())
            == (
                "retrying".to_owned(),
                1,
                0,
                Some("transport-unavailable".to_owned()),
                None,
            ),
        "reopened Twilio delivery treated a no-IO commit failure as ambiguous"
    );
}

pub(super) fn stage_later_delivery(
    app: &App,
    source_job_id: crate::state::ReceiverJobId,
    later_job_id: crate::state::ReceiverJobId,
) {
    let connection = rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state to seed a later response");
    connection
        .execute(
            "UPDATE receiver_jobs SET state = 'answer-ready' WHERE job_id = ?1",
            [later_job_id.to_string()],
        )
        .expect("stage later answer-ready job");
    connection
        .execute(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                attempt_count, created_at_unix_ms, updated_at_unix_ms)
             SELECT ?1, job.job_id, job.job_token, 'final-answer', source.envelope_json,
                    'ready', 0, 20_000, 20_000
             FROM receiver_jobs AS job
             JOIN receiver_deliveries AS source ON source.job_id = ?3
             WHERE job.job_id = ?2",
            rusqlite::params![
                crate::state::ReceiverDeliveryId::new().to_string(),
                later_job_id.to_string(),
                source_job_id.to_string(),
            ],
        )
        .expect("stage later semantic response");
}

pub(super) fn stage_sms_delivery(app: &App, job_id: crate::state::ReceiverJobId) {
    let envelope = serde_json::json!({
        "channel": "sms",
        "value": {
            "sender": "+12125550100",
            "recipient": "+12125550199",
            "body": "private durable SMS answer",
            "long_form_available": false
        }
    })
    .to_string();
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for SMS delivery")
        .execute(
            "UPDATE receiver_deliveries SET envelope_json = ?2 WHERE job_id = ?1",
            rusqlite::params![job_id.to_string(), envelope],
        )
        .expect("stage SMS delivery envelope");
}

pub(super) fn delivery_lifecycle_proof(
    app: &App,
    job_id: crate::state::ReceiverJobId,
) -> (String, i64, i64, Option<String>, Option<String>) {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver delivery lifecycle")
        .query_row(
            "SELECT state, attempt_count, provider_io_started,
                    error_category, ambiguity_reason
             FROM receiver_deliveries WHERE job_id = ?1",
            [job_id.to_string()],
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
        .expect("load receiver delivery lifecycle")
}

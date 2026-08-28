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
struct ScriptedDeliveryExecution {
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
    fn acknowledged() -> Self {
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

    fn reservation_count(&self) -> usize {
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
    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    app.tick_receiver();
    assert_eq!(execution.reservation_count(), 1);
    assert_eq!(job_state(&db, first.job_id()), ReceiverJobState::Delivering);
    app.tick_receiver();

    assert_eq!(job_state(&db, first.job_id()), ReceiverJobState::Done);
    assert_ne!(
        job_state(&db, second.job_id()),
        ReceiverJobState::Delivering,
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
    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    app.tick_receiver();

    assert_eq!(execution.reservation_count(), 1);
    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );

    app.tick_receiver();
    assert_eq!(
        execution.reservation_count(),
        2,
        "unsent answer remains due"
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

    assert_eq!(job_state(&db, first.job_id()), ReceiverJobState::Retrying);
    assert_eq!(execution.reservation_count(), 0);
}

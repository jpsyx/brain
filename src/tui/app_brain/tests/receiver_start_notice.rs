use super::receiver_durable_delivery::ScriptedDeliveryExecution;
use super::receiver_durable_observation::{rotate_active_session, write_active_snapshot};
use super::*;
use crate::state::ReceiverConversationIdentity;

#[test]
fn every_frontend_sends_one_start_sms_only_after_acceptance() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).unwrap();
        let inbound = receiver_job(&app, sms_actor(), Channel::Sms, "start notice");
        let identity = ReceiverConversationIdentity::sms(
            inbound.workspace_id,
            inbound.actor.user_id().clone(),
        );
        let accepted = db.accept_receiver_job(&inbound, &identity).unwrap();
        let execution = ScriptedDeliveryExecution::acknowledged();
        app.services
            .replace_receiver_start_execution(Box::new(execution.clone()));
        app.brain
            .replace_receiver_transport(TransportRecording::default().transport());
        app.tick_receiver();
        app.tick_receiver();
        assert_eq!(
            execution.start_count(),
            0,
            "{kind:?}: launch is not acceptance"
        );
        let session = rotate_active_session(&app, &format!("notice-{kind:?}"));
        write_active_snapshot(&app, &session, 1, "accepted", Some(1_000), None, None);
        app.tick_receiver();
        app.tick_receiver();
        assert_eq!(execution.start_count(), 1, "{kind:?}");
        app.tick_receiver();
        app.tick_receiver();
        assert_eq!(
            execution.start_count(),
            1,
            "{kind:?}: duplicate observation"
        );
        assert!(
            db.receiver_job(accepted.job_id()).unwrap().unwrap().state()
                == crate::state::ReceiverJobState::Accepted,
            "start notification must preserve the agent job state"
        );
    }
}

#[test]
fn fast_completion_still_sends_start_before_final_answer_without_submit_hook() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).unwrap();
    let inbound = receiver_job(&app, sms_actor(), Channel::Sms, "fast completion");
    let identity =
        ReceiverConversationIdentity::sms(inbound.workspace_id, inbound.actor.user_id().clone());
    db.accept_receiver_job(&inbound, &identity).unwrap();
    let starts = ScriptedDeliveryExecution::acknowledged();
    let answers = ScriptedDeliveryExecution::acknowledged();
    app.services
        .replace_receiver_start_execution(Box::new(starts.clone()));
    app.services
        .replace_receiver_delivery_execution(Box::new(answers.clone()));
    app.brain
        .replace_receiver_transport(TransportRecording::default().transport());
    app.tick_receiver();
    app.tick_receiver();
    super::receiver_durable_support::publish_valid_completion(&app, "Finished");
    app.tick_receiver();
    app.tick_receiver();
    assert_eq!(starts.start_count(), 1);
    assert_eq!(
        answers.start_count(),
        0,
        "answer must follow the start attempt"
    );
    app.tick_receiver();
    assert_eq!(answers.start_count(), 1);
}

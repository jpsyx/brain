use std::time::{Duration, Instant};

use super::{ReceiverRuntime, SyncGateObservation, SyncGatePoll};
use crate::server::receiver::{Channel, InboundJob};

fn sms_job(prompt: &str) -> InboundJob {
    let workspace_id = crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .expect("valid workspace id");
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: crate::users::UserId::parse("member").expect("valid user id"),
            name: "Member".to_owned(),
            phones: vec![crate::users::PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let actor = crate::actor::resolve_actor(
        &crate::users::UserId::parse("member").expect("valid user id"),
        crate::actor::RequestIdentity::Sms {
            from: "+12125550100",
        },
        &users,
    )
    .expect("resolved actor");

    InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id,
        actor,
        channel: Channel::Sms,
        authenticated_sender: "+12125550100".to_owned(),
        prompt: prompt.to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 1,
        provider_id: None,
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    }
}

#[test]
fn construction_has_no_receiver_work_or_session_state() {
    let runtime = ReceiverRuntime::new(false);

    assert!(!runtime.is_enabled());
    assert!(!runtime.has_pending_work());
    assert!(!runtime.remote_turn_in_flight());
    assert!(!runtime.receiver_panel_is_warm());
    assert!(runtime.interactive_response_id().is_none());
    assert!(runtime.interactive_agent_session_id().is_none());
    assert!(!runtime.sync_gate_is_armed());
}

#[test]
fn successful_dispatch_owns_delivery_timing_and_warm_lease_transition() {
    let mut runtime = ReceiverRuntime::new(true);
    let job = sms_job("what is on today?");
    let started = Instant::now();

    runtime.enqueue(job.clone()).expect("queue room");
    runtime.request_receiver_launch(job.actor.clone());
    runtime.record_receiver_session("response-1".to_owned());
    assert!(runtime.finish_dispatch(true, &job, started));

    let turn = runtime.active_remote_turn().expect("active remote turn");
    assert_eq!(turn.response_id, "response-1");
    assert_eq!(turn.channel, Channel::Sms);
    assert_eq!(turn.sender, "+12125550100");
    assert_eq!(runtime.remote_started_at(), Some(started));
    assert!(!runtime.has_pending_work());
    assert!(runtime.remote_turn_in_flight());

    runtime.finish_remote_response(started + Duration::from_secs(1));

    assert!(!runtime.remote_turn_in_flight());
    assert!(runtime.receiver_panel_is_warm());
    assert_eq!(runtime.active_channel(), Some(Channel::Sms));
    assert!(
        runtime
            .warm_lease_expired(started + Duration::from_secs(180))
            .is_none(),
        "response completion renewed the lease one second after dispatch"
    );
    assert_eq!(
        runtime.warm_lease_expired(started + Duration::from_secs(181)),
        Some(Channel::Sms)
    );
}

#[test]
fn force_fresh_is_consumed_only_when_session_selection_begins() {
    let mut runtime = ReceiverRuntime::new(true);
    runtime
        .enqueue(sms_job("/new"))
        .expect("new-session control fits");
    let command = runtime.take_new_session().expect("new-session control");
    runtime.prepare_channel_launch(command.channel);

    let _launch = runtime.begin_session_launch();
    assert!(runtime.begin_session_selection().force_fresh);
    assert!(!runtime.begin_session_selection().force_fresh);
}

#[test]
fn sync_gate_transitions_only_from_caller_supplied_observations() {
    let mut runtime = ReceiverRuntime::new(true);
    let launched_at = Instant::now();
    runtime.arm_sync_gate(launched_at, Some(4), 1);

    let waiting = runtime.poll_sync_gate(SyncGateObservation::new(launched_at, Some(4), false));
    assert!(matches!(waiting, Some(SyncGatePoll::Waiting)));

    let completed = runtime.poll_sync_gate(SyncGateObservation::new(
        launched_at + Duration::from_millis(250),
        Some(5),
        false,
    ));
    assert!(matches!(completed, Some(SyncGatePoll::Completed)));
    assert!(!runtime.sync_gate_is_armed());
}

use super::receiver_durable_producer_matrix::{
    active_completion_path, active_observation_path, rotate_active_session, run_stop_hook,
    snapshot_timestamp,
};
use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn stop_hook_future_completion_evidence_uses_fresh_local_lease_authority() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "complete across clock skew", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let session = rotate_active_session(&app, "future-skew-claude-native");
    let observation_path = active_observation_path(&app);
    let completion_path = active_completion_path(&app);

    run_stop_hook(&app, AgentKind::Claude, &session, &observation_path);

    let snapshot: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&observation_path).expect("real producer observation"),
    )
    .expect("valid producer observation");
    let producer_completed_at = snapshot_timestamp(&snapshot, "completed_at_unix_ms");
    assert_eq!(snapshot["phase"], "completed");
    assert!(
        completion_path.exists(),
        "real producer completion artifact"
    );
    assert!(
        producer_completed_at > clock.unix_ms().saturating_add(30_000),
        "the real producer clock must be beyond the renewed local lease"
    );

    app.tick_receiver();

    let completed = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(completed.state(), ReceiverJobState::AnswerReady);
    assert_eq!(
        completed.completed_at_unix_ms(),
        Some(producer_completed_at),
        "real producer evidence must persist without becoming lease authority"
    );
    assert_eq!(transport.shutdowns(), 1);
    assert!(!completion_path.exists());
    assert!(!observation_path.exists());
    assert!(app.brain.receiver_run_observations().is_empty());
}

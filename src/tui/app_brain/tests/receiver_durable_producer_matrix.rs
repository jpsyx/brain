use std::path::Path;

use super::receiver_durable_producer_support::{ProducerStage, produce_stage, snapshot};
pub(super) use super::receiver_durable_producer_support::{
    active_completion_path, active_observation_path, produce_completion, rotate_active_session,
    run_stop_hook, snapshot_timestamp,
};
use super::receiver_durable_support::accept_email_job;
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn normalized_producers_drive_one_controller_and_coordinator_lifecycle_matrix() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).expect("state DB");
        let first = accept_email_job(&app, &db, "synthetic lifecycle", 100);
        let second = accept_email_job(&app, &db, "synthetic completion", 200);
        let first_transport = TransportRecording::default();
        app.brain
            .replace_receiver_transport(first_transport.transport());
        app.tick_receiver();
        let first_session = rotate_active_session(&app, &format!("native-{}-first", kind.as_str()));
        let first_path = active_observation_path(&app);
        assert_job(
            &db,
            first.job_id(),
            ReceiverJobState::Launched,
            0,
            None,
            None,
        );

        produce_stage(
            &app,
            kind,
            &first_session,
            &first_path,
            ProducerStage::ReorderedProgress,
        );
        app.tick_receiver();
        assert_job(
            &db,
            first.job_id(),
            ReceiverJobState::Launched,
            0,
            None,
            None,
        );
        assert!(
            !first_path.exists(),
            "{kind:?} reordered progress created evidence"
        );

        produce_stage(
            &app,
            kind,
            &first_session,
            &first_path,
            ProducerStage::Accepted,
        );
        let accepted_snapshot = snapshot(&first_path);
        let accepted_at = snapshot_timestamp(&accepted_snapshot, "accepted_at_unix_ms");
        assert_eq!(accepted_snapshot["phase"], "accepted", "{kind:?}");
        assert_eq!(accepted_snapshot["revision"], 1, "{kind:?}");
        app.tick_receiver();
        assert_job(
            &db,
            first.job_id(),
            ReceiverJobState::Accepted,
            1,
            Some(accepted_at),
            None,
        );
        assert_duplicate_stage(
            &mut app,
            &db,
            first.job_id(),
            kind,
            &first_session,
            &first_path,
            ProducerStage::Accepted,
        );

        produce_stage(
            &app,
            kind,
            &first_session,
            &first_path,
            ProducerStage::Progressing,
        );
        let progressing_snapshot = snapshot(&first_path);
        let progressing_at = snapshot_timestamp(&progressing_snapshot, "progressing_at_unix_ms");
        assert_eq!(progressing_snapshot["phase"], "progressing", "{kind:?}");
        assert_eq!(progressing_snapshot["revision"], 2, "{kind:?}");
        assert_eq!(
            snapshot_timestamp(&progressing_snapshot, "accepted_at_unix_ms"),
            accepted_at,
            "{kind:?} rewrote the accepted timestamp"
        );
        app.tick_receiver();
        assert_job(
            &db,
            first.job_id(),
            ReceiverJobState::Processing,
            2,
            Some(accepted_at),
            Some(progressing_at),
        );
        assert_duplicate_stage(
            &mut app,
            &db,
            first.job_id(),
            kind,
            &first_session,
            &first_path,
            ProducerStage::Progressing,
        );

        produce_stage(
            &app,
            kind,
            &first_session,
            &first_path,
            ProducerStage::Completed,
        );
        let completed_snapshot = snapshot(&first_path);
        let completed_at = snapshot_timestamp(&completed_snapshot, "completed_at_unix_ms");
        assert_eq!(completed_snapshot["phase"], "completed", "{kind:?}");
        assert_eq!(completed_snapshot["revision"], 3, "{kind:?}");
        assert_eq!(
            snapshot_timestamp(&completed_snapshot, "accepted_at_unix_ms"),
            accepted_at,
            "{kind:?} rewrote the accepted timestamp"
        );
        assert_eq!(
            snapshot_timestamp(&completed_snapshot, "progressing_at_unix_ms"),
            progressing_at,
            "{kind:?} rewrote the progress timestamp"
        );
        assert_terminal_duplicate(&app, kind, &first_session, &first_path);
        app.tick_receiver();
        let completed = db.receiver_job(first.job_id()).unwrap().unwrap();
        assert_eq!(completed.state(), ReceiverJobState::Done, "{kind:?}");
        assert_eq!(completed.observation_revision(), 3, "{kind:?}");
        assert_eq!(
            completed.accepted_at_unix_ms(),
            Some(accepted_at),
            "{kind:?}"
        );
        assert_eq!(
            completed.progressing_at_unix_ms(),
            Some(progressing_at),
            "{kind:?}"
        );
        assert_eq!(
            completed.completed_at_unix_ms(),
            Some(completed_at),
            "{kind:?}"
        );
        assert_eq!(first_transport.shutdowns(), 1, "{kind:?}");

        let second_transport = TransportRecording::default();
        app.brain
            .replace_receiver_transport(second_transport.transport());
        app.tick_receiver();
        let second_session =
            rotate_active_session(&app, &format!("native-{}-second", kind.as_str()));
        let second_path = active_observation_path(&app);
        produce_stage(
            &app,
            kind,
            &second_session,
            &second_path,
            ProducerStage::Completed,
        );
        let completion_first = snapshot(&second_path);
        let completion_first_at = snapshot_timestamp(&completion_first, "completed_at_unix_ms");
        assert_eq!(completion_first["phase"], "completed", "{kind:?}");
        assert_eq!(completion_first["revision"], 1, "{kind:?}");
        assert!(
            completion_first["accepted_at_unix_ms"].is_null(),
            "{kind:?}"
        );
        assert!(
            completion_first["progressing_at_unix_ms"].is_null(),
            "{kind:?}"
        );
        assert_terminal_duplicate(&app, kind, &second_session, &second_path);
        app.tick_receiver();
        let completion_first_job = db.receiver_job(second.job_id()).unwrap().unwrap();
        assert_eq!(
            completion_first_job.state(),
            ReceiverJobState::Done,
            "{kind:?}"
        );
        assert_eq!(completion_first_job.observation_revision(), 1, "{kind:?}");
        assert_eq!(completion_first_job.accepted_at_unix_ms(), None, "{kind:?}");
        assert_eq!(
            completion_first_job.progressing_at_unix_ms(),
            None,
            "{kind:?}"
        );
        assert_eq!(
            completion_first_job.completed_at_unix_ms(),
            Some(completion_first_at),
            "{kind:?}"
        );
        assert_eq!(second_transport.shutdowns(), 1, "{kind:?}");
        assert!(app.brain.receiver_run_observations().is_empty(), "{kind:?}");
    }
}

fn assert_terminal_duplicate(app: &App, kind: AgentKind, session: &AgentSession, path: &Path) {
    let completion_path = active_completion_path(app);
    let completion_before = std::fs::read(&completion_path).expect("completion");
    let snapshot_before = std::fs::read(path).expect("completed snapshot");
    produce_stage(app, kind, session, path, ProducerStage::Completed);
    assert_eq!(
        std::fs::read(&completion_path).expect("duplicate completion"),
        completion_before,
        "{kind:?} duplicate terminal producer changed the artifact"
    );
    assert_eq!(
        std::fs::read(path).expect("duplicate completed snapshot"),
        snapshot_before,
        "{kind:?} duplicate terminal producer changed the snapshot"
    );
}

fn assert_duplicate_stage(
    app: &mut App,
    db: &Db,
    job_id: crate::state::ReceiverJobId,
    kind: AgentKind,
    session: &AgentSession,
    path: &Path,
    stage: ProducerStage,
) {
    let durable_before = db.receiver_job(job_id).unwrap().unwrap();
    let snapshot_before = std::fs::read(path).expect("snapshot before duplicate");
    produce_stage(app, kind, session, path, stage);
    assert_eq!(
        std::fs::read(path).expect("snapshot after duplicate"),
        snapshot_before,
        "{kind:?} duplicate producer delivery changed the snapshot"
    );
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(job_id).unwrap().unwrap(),
        durable_before,
        "{kind:?} duplicate producer delivery changed durable evidence"
    );
}

fn assert_job(
    db: &Db,
    job_id: crate::state::ReceiverJobId,
    state: ReceiverJobState,
    revision: u64,
    accepted_at: Option<u64>,
    progressing_at: Option<u64>,
) {
    let job = db.receiver_job(job_id).unwrap().unwrap();
    assert_eq!(job.state(), state);
    assert_eq!(job.observation_revision(), revision);
    assert_eq!(job.accepted_at_unix_ms(), accepted_at);
    assert_eq!(job.progressing_at_unix_ms(), progressing_at);
    assert_eq!(job.completed_at_unix_ms(), None);
}

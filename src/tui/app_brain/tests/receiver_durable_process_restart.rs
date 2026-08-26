use super::receiver_durable_support::accept_email_job;
use super::*;

use crate::state::{ReceiverNonterminalObservationPhase, ReceiverObservation};

#[test]
fn fresh_app_preserves_expired_launched_and_observed_runs_without_replay() {
    for state in ["launched", "accepted", "processing"] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut first_app = test_app(&temporary, &cli, AgentKind::Claude);
        first_app.receiver.record_intent(true);
        let db = Db::open(first_app.context.workspace()).expect("state DB");
        let accepted = accept_email_job(&first_app, &db, "synthetic restart", 100);
        first_app
            .brain
            .replace_receiver_transport(TransportRecording::default().transport());
        first_app.tick_receiver();
        let active = first_app
            .receiver
            .active_durable_run()
            .expect("active receiver");
        let token = active.claim.job().token();
        let owner = active.claim.claim().owner().to_owned();
        let instance = active.attribution.instance().to_owned();
        if state != "launched" {
            let session = AgentSession::new("native-restart").expect("native session");
            rusqlite::Connection::open(first_app.context.state_db_path())
                .expect("lifecycle fixture connection")
                .execute(
                    "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
                    rusqlite::params![session.as_str(), &instance],
                )
                .expect("simulate lifecycle native session");
            assert!(
                db.apply_receiver_observation(
                    accepted.job_id(),
                    &owner,
                    &ReceiverObservation {
                        token,
                        instance: instance.clone(),
                        session_id: session.as_str().to_owned(),
                        phase: ReceiverNonterminalObservationPhase::Accepted,
                        revision: 1,
                        observed_at_unix_ms: 1_000,
                        authorized_at_unix_ms: 1_050,
                    },
                )
                .expect("persist accepted evidence")
            );
            if state == "processing" {
                assert!(
                    db.apply_receiver_observation(
                        accepted.job_id(),
                        &owner,
                        &ReceiverObservation {
                            token,
                            instance: instance.clone(),
                            session_id: session.as_str().to_owned(),
                            phase: ReceiverNonterminalObservationPhase::Progressing,
                            revision: 2,
                            observed_at_unix_ms: 1_100,
                            authorized_at_unix_ms: 1_150,
                        },
                    )
                    .expect("persist progressing evidence")
                );
            }
        }
        rusqlite::Connection::open(first_app.context.state_db_path())
            .expect("expiry fixture connection")
            .execute(
                "UPDATE receiver_jobs SET claim_expires_at_unix_ms = 1 WHERE job_id = ?1",
                [accepted.job_id().to_string()],
            )
            .expect("expire receiver lease");
        let before = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        drop(first_app);

        let mut fresh_app = test_app(&temporary, &cli, AgentKind::Claude);
        fresh_app.receiver.record_intent(true);
        let transport = TransportRecording::default();
        fresh_app
            .brain
            .replace_receiver_transport(transport.transport());
        fresh_app.tick_receiver();

        let reopened = Db::open(fresh_app.context.workspace()).expect("reopened state DB");
        assert_eq!(
            reopened.receiver_job(accepted.job_id()).unwrap().unwrap(),
            before,
            "{state} changed after process restart"
        );
        assert!(
            fresh_app.brain.receiver_run_observations().is_empty(),
            "{state} was replayed by the fresh App"
        );
        assert_eq!(transport.shutdowns(), 0, "{state}");
    }
}

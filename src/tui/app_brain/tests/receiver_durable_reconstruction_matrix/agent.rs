use super::super::receiver_durable_answer_cleanup::job_state;
use super::super::receiver_durable_support::{
    ReceiverClock, accept_email_job, publish_valid_completion,
};
use super::*;

use crate::state::{ReceiverJobState, ReceiverNonterminalObservationPhase, ReceiverObservation};

pub(super) fn assert_reconstructs_and_advances(phase: RestartPhase, departure: Departure) {
    match phase {
        RestartPhase::Queued | RestartPhase::Claimed | RestartPhase::Launching => {
            assert_pre_spawn_reconstruction(phase, departure);
        }
        RestartPhase::Launched | RestartPhase::Accepted | RestartPhase::Processing => {
            assert_post_spawn_reconstruction(phase, departure);
        }
        RestartPhase::Failed | RestartPhase::Done => {
            assert_terminal_reconstruction(phase, departure);
        }
        _ => unreachable!("agent reconstruction received a delivery phase"),
    }
}

fn assert_pre_spawn_reconstruction(phase: RestartPhase, departure: Departure) {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut origin = test_app(&temporary, &cli, AgentKind::Claude);
    let generic = Departure::install_generic_controller(&mut origin);
    origin.receiver.record_intent(true);
    let clock = ReceiverClock::at_unix_ms(1_000);
    origin
        .services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(origin.context.workspace()).expect("origin state DB");
    let first = accept_email_job(&origin, &db, "matrix first", 100);
    let later = accept_email_job(&origin, &db, "matrix later", 200);

    if phase != RestartPhase::Queued {
        let claim = db
            .claim_next_receiver_run("departed-origin", 1_000, 2_000)
            .expect("claim phase fixture")
            .expect("claimed phase job");
        assert_eq!(claim.job().id(), first.job_id());
        if phase == RestartPhase::Launching {
            assert!(
                db.prepare_receiver_job_launch(first.job_id(), "departed-origin", 1_100)
                    .expect("prepare launching phase")
            );
        }
        clock.advance(std::time::Duration::from_secs(2));
    }

    departure.leave(&mut origin, &generic, phase);
    assert_eq!(
        job_state(&db, first.job_id()),
        match phase {
            RestartPhase::Queued => ReceiverJobState::Queued,
            RestartPhase::Claimed => ReceiverJobState::Claimed,
            RestartPhase::Launching => ReceiverJobState::Launching,
            _ => unreachable!("pre-spawn immediate outcome received another phase"),
        },
        "{phase:?} orderly shutdown changed the immediate durable phase"
    );
    drop(db);
    drop(origin);
    let (mut restarted, _transport) = reconstructed_app(&temporary, &clock);
    let reopened = Db::open(restarted.context.workspace()).expect("reconstructed state DB");

    drive_first_then_follower(
        &mut restarted,
        &reopened,
        &clock,
        first.job_id(),
        later.job_id(),
    );
    assert_resumed_or_terminalized(&restarted, &reopened, first.job_id(), phase);
    assert_ne!(
        job_state(&reopened, later.job_id()),
        ReceiverJobState::Queued,
        "{phase:?} reconstruction left FIFO waiting on the departed App"
    );
}

fn assert_post_spawn_reconstruction(phase: RestartPhase, departure: Departure) {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut origin = test_app(&temporary, &cli, AgentKind::Claude);
    let generic = Departure::install_generic_controller(&mut origin);
    origin.receiver.record_intent(true);
    let clock = ReceiverClock::at_unix_ms(10_000);
    origin
        .services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(origin.context.workspace()).expect("origin state DB");
    let private_inbound = "private post-spawn inbound must not replay";
    let first = accept_email_job(&origin, &db, private_inbound, 100);
    let later = accept_email_job(&origin, &db, "matrix later", 200);
    origin
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    origin.tick_receiver();

    let active = origin
        .receiver
        .active_durable_run()
        .expect("origin active receiver");
    let owner = active.claim.claim().owner().to_owned();
    let token = active.claim.job().token();
    let instance = active.attribution.instance().to_owned();
    let native_session =
        AgentSession::new(format!("claude-reconstruction-{}", uuid::Uuid::new_v4()))
            .expect("native session");
    let _transcript = (phase != RestartPhase::Launched).then(|| {
        ClaudeTranscript::create(origin.context.workspace().root(), native_session.as_str())
    });

    if phase != RestartPhase::Launched {
        rusqlite::Connection::open(origin.context.state_db_path())
            .expect("native-session fixture connection")
            .execute(
                "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
                rusqlite::params![native_session.as_str(), &instance],
            )
            .expect("persist native session");
        assert!(
            db.apply_receiver_observation(
                first.job_id(),
                &owner,
                &ReceiverObservation {
                    token,
                    instance: instance.clone(),
                    session_id: native_session.as_str().to_owned(),
                    phase: ReceiverNonterminalObservationPhase::Accepted,
                    revision: 1,
                    observed_at_unix_ms: 10_100,
                    authorized_at_unix_ms: 10_100,
                },
            )
            .expect("persist accepted phase")
        );
        if phase == RestartPhase::Processing {
            assert!(
                db.apply_receiver_observation(
                    first.job_id(),
                    &owner,
                    &ReceiverObservation {
                        token,
                        instance: instance.clone(),
                        session_id: native_session.as_str().to_owned(),
                        phase: ReceiverNonterminalObservationPhase::Progressing,
                        revision: 2,
                        observed_at_unix_ms: 10_200,
                        authorized_at_unix_ms: 10_200,
                    },
                )
                .expect("persist processing phase")
            );
        }
    }
    rusqlite::Connection::open(origin.context.state_db_path())
        .expect("dead-origin fixture connection")
        .execute(
            "UPDATE brain_sessions SET locked_pid = 999999 WHERE brain_instance_id = ?1",
            [&instance],
        )
        .expect("mark origin process dead");
    clock.advance(std::time::Duration::from_secs(10 * 60));

    departure.leave(&mut origin, &generic, phase);
    assert!(
        job_state(&db, first.job_id())
            == match phase {
                RestartPhase::Launched => ReceiverJobState::Launched,
                RestartPhase::Accepted => ReceiverJobState::Accepted,
                RestartPhase::Processing => ReceiverJobState::Processing,
                _ => unreachable!("post-spawn immediate outcome received another phase"),
            },
        "{phase:?} orderly shutdown changed the immediate durable phase"
    );
    drop(db);
    drop(origin);
    let (mut restarted, transport) = reconstructed_app(&temporary, &clock);
    let reopened = Db::open(restarted.context.workspace()).expect("reconstructed state DB");

    drive_first_then_follower(
        &mut restarted,
        &reopened,
        &clock,
        first.job_id(),
        later.job_id(),
    );
    assert_resumed_or_terminalized(&restarted, &reopened, first.job_id(), phase);
    assert_post_spawn_no_replay(&transport, private_inbound, token, &native_session, phase);
    assert_ne!(
        job_state(&reopened, later.job_id()),
        ReceiverJobState::Queued,
        "{phase:?} reconstruction left FIFO waiting on the departed controller"
    );
}

fn assert_terminal_reconstruction(phase: RestartPhase, departure: Departure) {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut origin = test_app(&temporary, &cli, AgentKind::Claude);
    let generic = Departure::install_generic_controller(&mut origin);
    let db = Db::open(origin.context.workspace()).expect("origin state DB");
    let private_inbound = "private terminal inbound must not replay";
    let first = accept_email_job(&origin, &db, private_inbound, 100);
    let (expected, state) = match phase {
        RestartPhase::Failed => (ReceiverJobState::Failed, "failed"),
        RestartPhase::Done => (ReceiverJobState::Done, "done"),
        _ => unreachable!("terminal fixture received nonterminal phase"),
    };
    rusqlite::Connection::open(origin.context.state_db_path())
        .expect("terminal fixture connection")
        .execute(
            "UPDATE receiver_jobs
             SET state = ?1, claim_owner = NULL, claim_expires_at_unix_ms = NULL
             WHERE job_id = ?2",
            rusqlite::params![state, first.job_id().to_string()],
        )
        .expect("seed terminal phase");
    departure.leave(&mut origin, &generic, phase);
    assert!(
        job_state(&db, first.job_id()) == expected,
        "{phase:?} orderly shutdown changed terminal authority"
    );
    drop(db);
    drop(origin);

    let clock = ReceiverClock::at_unix_ms(20_000);
    let (mut restarted, transport) = reconstructed_app(&temporary, &clock);
    let reopened = Db::open(restarted.context.workspace()).expect("reconstructed state DB");
    restarted.tick_receiver();

    assert!(
        job_state(&reopened, first.job_id()) == expected,
        "{phase:?} fresh App changed the terminal state"
    );
    assert!(
        transport.launch_specs().is_empty(),
        "{phase:?} fresh App launched terminal receiver work"
    );

    let later = accept_email_job(&restarted, &reopened, "matrix later", 200);
    restarted.tick_receiver();
    assert_ne!(
        job_state(&reopened, later.job_id()),
        ReceiverJobState::Queued,
        "{phase:?} replayed or blocked the FIFO follower"
    );
    assert!(
        restarted
            .receiver
            .active_durable_run()
            .is_some_and(|run| run.claim.job().id() == later.job_id()),
        "{phase:?} reconstructed the wrong run"
    );
    let specifications = transport.launch_specs();
    assert!(
        specifications.len() == 1,
        "{phase:?} fresh App launched more than the FIFO follower"
    );
    assert!(
        !specifications[0].command.contains(private_inbound),
        "{phase:?} fresh App replayed terminal inbound content"
    );
}

fn reconstructed_app(
    temporary: &tempfile::TempDir,
    clock: &ReceiverClock,
) -> (App, TransportRecording) {
    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let transport = TransportRecording::default();
    restarted
        .brain
        .replace_receiver_transport(transport.transport());
    (restarted, transport)
}

fn drive_first_then_follower(
    restarted: &mut App,
    db: &Db,
    clock: &ReceiverClock,
    first: crate::state::ReceiverJobId,
    later: crate::state::ReceiverJobId,
) {
    let mut completion_published = false;
    for _ in 0..12 {
        restarted.tick_receiver();
        if !completion_published
            && restarted
                .receiver
                .active_durable_run()
                .is_some_and(|run| run.claim.job().id() == first)
        {
            publish_valid_completion(restarted, "matrix reconstructed completion");
            completion_published = true;
        }
        if job_state(db, later) != ReceiverJobState::Queued {
            return;
        }
        clock.advance(std::time::Duration::from_secs(30));
    }
}

fn assert_resumed_or_terminalized(
    restarted: &App,
    db: &Db,
    first: crate::state::ReceiverJobId,
    phase: RestartPhase,
) {
    let state = job_state(db, first);
    if matches!(
        state,
        ReceiverJobState::Launched | ReceiverJobState::Accepted | ReceiverJobState::Processing
    ) {
        assert!(
            restarted
                .receiver
                .active_durable_run()
                .is_some_and(|run| run.claim.job().id() == first),
            "{phase:?} reconstruction left durable work without a fresh-App controller"
        );
        return;
    }
    assert!(
        matches!(
            state,
            ReceiverJobState::AnswerReady
                | ReceiverJobState::Delivering
                | ReceiverJobState::Retrying
                | ReceiverJobState::Failed
                | ReceiverJobState::Done
        ),
        "{phase:?} reconstruction neither resumed safely nor terminalized explicitly: {state:?}"
    );
}

fn assert_post_spawn_no_replay(
    transport: &TransportRecording,
    private_inbound: &str,
    token: crate::state::ReceiverJobToken,
    native_session: &AgentSession,
    phase: RestartPhase,
) {
    let specifications = transport.launch_specs();
    assert!(
        specifications
            .iter()
            .all(|specification| !specification.command.contains(private_inbound)),
        "{phase:?} fresh App replayed the durable inbound turn"
    );
    let token = token.to_string();
    let recovery_commands: Vec<_> = specifications
        .iter()
        .filter(|specification| specification.command.contains(&token))
        .collect();
    match phase {
        RestartPhase::Launched => assert!(
            recovery_commands.is_empty(),
            "launched reconstruction did not terminalize before the FIFO follower"
        ),
        RestartPhase::Accepted | RestartPhase::Processing => {
            assert!(
                recovery_commands.len() == 1,
                "{phase:?} reconstruction did not launch exactly one durable recovery"
            );
            let command = &recovery_commands[0].command;
            assert!(
                command.contains("--resume") && command.contains(native_session.as_str()),
                "{phase:?} reconstruction did not resume the exact durable native session"
            );
        }
        _ => unreachable!("post-spawn replay assertion received another phase"),
    }
}

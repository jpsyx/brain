use super::receiver_durable_support::{ReceiverClock, accept_email_job, accept_email_job_with_id};
use super::*;

use crate::main_view::MainView;
use crate::state::{ReceiverConversationIdentity, ReceiverJobState};
use crate::tui::model::{BrainTab, Panel};

#[test]
fn durable_receiver_launches_in_background_while_main_turn_is_busy() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    app.shell.show_main_view(MainView::BrainSearch);
    let (main, main_recording) = recording_controller(&app, true, "busy main");
    app.brain.install_main(main);
    app.brain.mark_turn_started();
    let before = (
        app.shell.main_view(),
        app.brain.any_panel_visible(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );
    assert_eq!(
        before,
        (MainView::BrainSearch, true, BrainTab::Main, Panel::Tasks)
    );

    let inbound = receiver_job(&app, sms_actor(), Channel::Sms, "Handle this remotely");
    let identity = ReceiverConversationIdentity::sms(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job");
    let receiver_recording = TransportRecording::default();
    app.brain
        .replace_receiver_transport(receiver_recording.transport());

    app.tick_receiver();

    let runs = app.brain.receiver_run_observations();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].job_id, accepted.job_id());
    assert_eq!(receiver_recording.launch_specs().len(), 1);
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load receiver job")
            .expect("receiver job")
            .state(),
        ReceiverJobState::Launched
    );
    assert_eq!(
        (
            app.shell.main_view(),
            app.brain.any_panel_visible(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before
    );
    assert!(
        !main_recording
            .events()
            .contains(&ControllerEvent::QueueAfterActiveTurn)
    );
}

#[test]
fn active_receiver_renews_its_exact_durable_claim() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let inbound = receiver_job(&app, sms_actor(), Channel::Sms, "Keep this claim alive");
    let identity = ReceiverConversationIdentity::sms(
        app.context.workspace().id(),
        inbound.actor.user_id().clone(),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    db.accept_receiver_job(&inbound, &identity)
        .expect("accept durable receiver job");
    let receiver_recording = TransportRecording::default();
    app.brain
        .replace_receiver_transport(receiver_recording.transport());
    app.tick_receiver();

    clock.advance(std::time::Duration::from_secs(20));
    app.tick_receiver();
    clock.advance(std::time::Duration::from_secs(15));
    let now = clock.unix_ms();

    assert!(
        db.claim_next_receiver_run("competing-owner", now, now + 30_000)
            .expect("competing claim")
            .is_none(),
        "the active run must renew before its original claim expires"
    );
}

#[test]
fn durable_receiver_fifo_breaks_equal_timestamps_by_job_id() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let high = accept_email_job_with_id(
        &app,
        &db,
        "lexically later",
        100,
        uuid::Uuid::parse_str("ffffffff-ffff-4fff-8fff-ffffffffffff").unwrap(),
    );
    let low = accept_email_job_with_id(
        &app,
        &db,
        "lexically earlier",
        100,
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
    );
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());

    app.tick_receiver();

    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        low.job_id()
    );
    assert_eq!(
        db.receiver_job(high.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued
    );
}

#[test]
fn spawn_failure_cleans_registration_and_schedules_preacceptance_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "spawn failure", 100);
    app.brain
        .replace_receiver_transport(Box::new(FailingSpawnTransport));

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Retrying);
    assert_eq!(job.retry_count(), 1);
    assert_eq!(job.last_error(), Some("launch-spawn"));
}

#[test]
fn every_frontend_gets_an_isolated_controller_and_remote_instance() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let mut config = app.context.config().clone();
        config.access_mode = crate::access::AccessMode::WorkspaceOnly;
        app.context = app.context.replacing_config(config);
        let before = (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        );
        let db = Db::open(app.context.workspace()).expect("state DB");
        let accepted = accept_email_job(&app, &db, "frontend-neutral run", 100);
        let transport = TransportRecording::default();
        app.brain.replace_receiver_transport(transport.transport());

        app.tick_receiver();

        let observations = app.brain.receiver_run_observations();
        assert_eq!(observations.len(), 1, "missing {kind:?} receiver tab");
        assert_eq!(observations[0].job_id, accepted.job_id());
        let specs = transport.launch_specs();
        assert_eq!(specs.len(), 1, "missing {kind:?} receiver launch");
        let instance = specs[0]
            .environment
            .iter()
            .find(|(name, _)| name == "BRAIN_INSTANCE_ID")
            .map(|(_, value)| value.as_str())
            .expect("remote instance environment");
        let response = specs[0]
            .environment
            .iter()
            .find(|(name, _)| name == "BRAIN_RESPONSE_ID")
            .map(|(_, value)| value.as_str())
            .expect("remote response environment");
        let token = specs[0]
            .environment
            .iter()
            .find(|(name, _)| name == "BRAIN_RECEIVER_JOB_TOKEN")
            .map(|(_, value)| value.as_str())
            .expect("receiver job-token environment");
        let observation_path = specs[0]
            .environment
            .iter()
            .find(|(name, _)| name == "BRAIN_RECEIVER_OBSERVATION_PATH")
            .map(|(_, value)| value.as_str())
            .expect("receiver observation-path environment");
        assert_eq!(instance, response);
        assert_eq!(
            token,
            db.receiver_job(accepted.job_id())
                .unwrap()
                .unwrap()
                .token()
                .to_string()
        );
        assert_eq!(
            observation_path,
            app.context
                .workspace()
                .paths()
                .cache_dir()
                .join("receiver-observations")
                .join(format!("{instance}.json"))
                .display()
                .to_string()
        );
        assert!(
            specs[0]
                .command
                .contains(&format!("<!-- brain:receiver-job-token={token} -->"))
        );
        assert_ne!(instance, "shell-under-test");
        assert_eq!(specs[0].cwd, app.context.workspace().root());
        assert!(specs[0].command.contains("frontend-neutral run"));
        assert!(
            specs[0]
                .environment
                .contains(&("BRAIN_ACTOR_ID".to_owned(), "remote-member".to_owned(),))
        );
        assert!(
            specs[0]
                .environment
                .contains(&("BRAIN_CHANNEL".to_owned(), "email".to_owned()))
        );
        if kind == AgentKind::OpenCode {
            let policy = specs[0]
                .environment
                .iter()
                .find(|(name, _)| name == "OPENCODE_CONFIG_CONTENT")
                .map(|(_, value)| value.as_str())
                .expect("OpenCode inline policy");
            assert!(policy.contains("advisory prompt enforcement"));
        } else {
            assert!(specs[0].command.contains("advisory prompt enforcement"));
        }
        assert_eq!(
            (
                app.shell.main_view(),
                app.effective_brain_tab(),
                app.shell.focus(),
            ),
            before,
            "{kind:?} receiver launch changed the active shell"
        );
    }
}

#[test]
fn progressed_stale_job_is_not_rerun_before_recovery_policy_exists() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "already progressed", 100);
    let previous_session = AgentSession::new("previous-session").expect("previous session");
    let previous_scope = crate::agent::SessionScope::new(
        AgentKind::Claude,
        app.context.workspace().id(),
        email_actor(),
    );
    db.register_receiver_session(
        accepted.conversation_id(),
        &previous_session,
        "previous-instance",
        42,
        &previous_scope,
    )
    .expect("register previous lifecycle session");
    let now = clock.unix_ms();
    let claim = db
        .claim_next_receiver_run("previous-owner", now, now + 1_000)
        .expect("initial claim")
        .expect("claim available");
    assert_eq!(claim.job().id(), accepted.job_id());
    assert!(
        db.prepare_receiver_job_launch(accepted.job_id(), "previous-owner", now)
            .expect("prepare previous launch")
    );
    let token = db
        .receiver_job(accepted.job_id())
        .expect("load previous launch")
        .expect("previous launch")
        .token();
    assert!(
        db.commit_receiver_job_launch(
            accepted.job_id(),
            "previous-owner",
            &crate::state::ReceiverLaunchObservation {
                token,
                instance: "previous-instance".to_owned(),
                session_id: "previous-session".to_owned(),
                observed_at_unix_ms: now,
                authorized_at_unix_ms: now,
            },
        )
        .expect("commit previous launch")
    );
    assert!(
        db.apply_receiver_observation(
            accepted.job_id(),
            "previous-owner",
            &crate::state::ReceiverObservation {
                token,
                instance: "previous-instance".to_owned(),
                session_id: "previous-session".to_owned(),
                phase: crate::state::ReceiverNonterminalObservationPhase::Accepted,
                revision: 1,
                observed_at_unix_ms: now,
                authorized_at_unix_ms: now,
            },
        )
        .expect("record accepted evidence")
    );
    assert!(
        db.apply_receiver_observation(
            accepted.job_id(),
            "previous-owner",
            &crate::state::ReceiverObservation {
                token,
                instance: "previous-instance".to_owned(),
                session_id: "previous-session".to_owned(),
                phase: crate::state::ReceiverNonterminalObservationPhase::Progressing,
                revision: 2,
                observed_at_unix_ms: now,
                authorized_at_unix_ms: now,
            },
        )
        .expect("record progressing evidence")
    );
    clock.advance(std::time::Duration::from_secs(2));
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(transport.launch_specs().is_empty());
    assert_eq!(transport.shutdowns(), 0);
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Processing
    );
}

use super::receiver_durable_support::{
    ReceiverClock, accept_email_job, accept_email_job_in_thread, publish_valid_completion,
};
use super::*;

use crate::main_view::MainView;
use crate::state::ReceiverJobState;

#[test]
fn completion_closes_only_the_exact_receiver_then_next_tick_launches_oldest_waiter() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    app.shell.show_main_view(MainView::BrainSearch);
    let (main, main_recording) = recording_controller(&app, true, "main");
    app.brain.install_main(main);
    let before = (
        app.shell.main_view(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );
    let db = Db::open(app.context.workspace()).expect("state DB");
    let newer = accept_email_job(&app, &db, "newer", 200);
    let older = accept_email_job(&app, &db, "older", 100);
    let first_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(first_transport.transport());

    app.tick_receiver();
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        older.job_id()
    );
    let later = accept_email_job(&app, &db, "later arrival", 150);
    app.tick_receiver();
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(
        db.receiver_job(later.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued
    );
    let active = app.receiver.active_durable_run().expect("active receiver");
    let wrong_path = app
        .context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{}.json", active.attribution.instance()));
    std::fs::create_dir_all(wrong_path.parent().unwrap()).unwrap();
    std::fs::write(
        &wrong_path,
        serde_json::json!({
            "session_id": active.attribution.registered_session().as_str(),
            "response_id": "another-remote-instance",
            "frontend": "claude",
            "workspace_id": app.context.workspace().id().to_string(),
            "actor_id": active.attribution.scope().actor().user_id().as_str(),
            "channel": "email",
            "completion_status": "completed",
            "message": "wrong run",
        })
        .to_string(),
    )
    .unwrap();
    app.tick_receiver();
    assert_eq!(
        app.brain.receiver_run_observations().len(),
        1,
        "another run's artifact is not completion"
    );
    std::fs::write(
        app.context.tasks_csv_path(),
        "task_uuid,task_id,task_name,task_type,status,priority,due_date,hard_deadline,start_date,assigned_to,system_key,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue\n\
         8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,T1,Created remotely,,not_started,p2,,false,,pablo,,,,,,,,,0,2026-08-24,,,\n",
    )
    .expect("remote task mutation");
    assert_eq!(
        crate::tasks::task::load_tasks(app.context.tasks_csv_path())
            .expect("valid remote task fixture")
            .len(),
        1
    );
    let completion_path = publish_valid_completion(&app, "Finished remotely");

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(first_transport.shutdowns(), 1);
    assert_eq!(
        db.receiver_job(older.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );
    assert!(!completion_path.exists());
    assert!(app.tasks.contains_task_named("Created remotely"));
    assert_eq!(
        (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before
    );
    assert_eq!(main_recording.events(), Vec::<ControllerEvent>::new());

    let second_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(second_transport.transport());
    app.tick_receiver();

    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        later.job_id()
    );
    assert_eq!(
        db.receiver_job(newer.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued
    );
    assert_eq!(
        (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before,
        "launching the next durable run must also preserve the active shell"
    );
}

#[test]
fn child_exit_after_launch_cleans_locally_without_replaying_or_changing_correlation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "exit before completion", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();
    let durable_before = db
        .receiver_job(accepted.job_id())
        .expect("load launched job")
        .expect("launched job");
    let correlation_before = app
        .services
        .locked_session_for_instance(attribution.instance(), attribution.scope());
    transport.set_alive(false);

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap(),
        durable_before,
        "an ambiguous post-spawn exit must not change durable job state"
    );
    assert_eq!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope()),
        correlation_before,
        "an ambiguous post-spawn exit must retain durable session correlation"
    );
}

#[test]
fn expired_launched_lease_stops_local_child_without_mutating_durable_correlation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "ownership changes", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();
    let durable_before = db
        .receiver_job(accepted.job_id())
        .expect("load launched job")
        .expect("launched job");
    let correlation_before = app
        .services
        .locked_session_for_instance(attribution.instance(), attribution.scope());
    let artifact = app
        .context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{}.json", attribution.instance()));
    std::fs::create_dir_all(artifact.parent().expect("response directory"))
        .expect("create response directory");
    std::fs::write(&artifact, "partial private response").expect("partial response artifact");
    clock.advance(std::time::Duration::from_secs(31));
    let now = clock.unix_ms();
    assert!(
        db.claim_next_receiver_run("replacement-owner", now, now + 30_000)
            .expect("poll expired launched job")
            .is_none()
    );

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap(),
        durable_before
    );
    assert_eq!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope()),
        correlation_before
    );
    assert!(
        !artifact.exists(),
        "lease-expiry cleanup left a local artifact"
    );
}

#[test]
fn active_receiver_remains_owned_and_completes_across_disable_and_reenable() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "finish despite disable", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();

    clock.advance(std::time::Duration::from_secs(20));
    app.receiver.record_intent(false);
    app.tick_receiver();
    clock.advance(std::time::Duration::from_secs(15));
    let now = clock.unix_ms();

    assert!(
        db.claim_next_receiver_run("competing-owner", now, now + 30_000)
            .expect("competing claim")
            .is_none(),
        "disabling intent must not abandon an already active claim"
    );
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(transport.shutdowns(), 0);

    app.receiver.record_intent(true);
    publish_valid_completion(&app, "Finished after re-enable");
    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::AnswerReady
    );
}

#[test]
fn fresh_claude_completion_persists_its_native_id_and_the_next_message_resumes_it() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_email_job_in_thread(&app, &db, "claude-thread", "first message", 100);
    assert!(
        db.update_receiver_conversation(
            first.conversation_id(),
            "# Portable transcript\n\nPrior durable context",
            None,
            50,
        )
        .expect("seed portable transcript")
    );
    let first_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(first_transport.transport());
    app.tick_receiver();
    let active = app
        .receiver
        .active_durable_run()
        .expect("fresh Claude receiver");
    let native_id = active.attribution.registered_session().clone();
    let _transcript = ClaudeTranscript::create(app.context.workspace().root(), native_id.as_str());
    publish_valid_completion(&app, "first answer");

    app.tick_receiver();

    let conversation = db
        .receiver_conversation(first.conversation_id())
        .unwrap()
        .expect("durable conversation");
    assert_eq!(
        conversation
            .binding()
            .map(|binding| (binding.frontend(), binding.native_session_id().to_owned(),)),
        Some((AgentKind::Claude, native_id.as_str().to_owned()))
    );
    assert_eq!(
        conversation.transcript_markdown(),
        "# Portable transcript\n\nPrior durable context\n\n## Authenticated user\n\n```text\nfirst message\n```\n\n## Assistant\n\n```text\nfirst answer\n```"
    );
    let second = accept_email_job_in_thread(&app, &db, "claude-thread", "second message", 200);
    let second_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(second_transport.transport());

    app.tick_receiver();

    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        second.job_id()
    );
    let specs = second_transport.launch_specs();
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.contains("--resume"));
    assert!(specs[0].command.contains(native_id.as_str()));
    assert!(specs[0].command.contains("second message"));
    assert!(!specs[0].command.contains("Prior durable context"));
}

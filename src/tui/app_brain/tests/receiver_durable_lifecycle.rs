use super::receiver_durable_support::{ReceiverClock, accept_email_job, publish_valid_completion};
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
        ReceiverJobState::Done
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
fn child_exit_without_valid_completion_releases_registration_and_retries_durably() {
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
    transport.set_alive(false);

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Retrying
    );
    assert!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope())
            .is_none()
    );
}

#[test]
fn lost_claim_stops_local_child_without_mutating_session_or_job_lifecycle() {
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
    clock.advance(std::time::Duration::from_secs(31));
    let now = clock.unix_ms();
    db.claim_next_receiver_run("replacement-owner", now, now + 30_000)
        .expect("replacement claim")
        .expect("expired active job is recoverable");

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launching
    );
    assert!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope())
            .is_some(),
        "lost ownership must not release exact lifecycle state"
    );
}

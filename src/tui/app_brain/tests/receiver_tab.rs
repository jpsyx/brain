use super::*;

use crate::main_view::MainView;
use crate::skill_session::SkillSessionKey;
use crate::state::ReceiverJobId;

fn receiver_job_id(value: &str) -> ReceiverJobId {
    ReceiverJobId::from(uuid::Uuid::parse_str(value).expect("receiver job ID"))
}

fn receiver_controller(app: &App, recording: &TransportRecording) -> AgentController {
    AgentController::configured(
        app.context.command(),
        app.context.agent_kind(),
        crate::actor::test_actor("receiver"),
        recording.transport(),
    )
}

fn visible_state(app: &App) -> (MainView, bool, BrainTab, Panel) {
    (
        app.shell.main_view(),
        app.brain.any_panel_visible(),
        app.effective_brain_tab(),
        app.shell.focus(),
    )
}

#[test]
fn background_receiver_lifecycle_never_changes_view_visibility_tab_or_focus() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.shell.show_main_view(MainView::BrainSearch);
    let before = visible_state(&app);
    assert_eq!(
        before,
        (MainView::BrainSearch, false, BrainTab::Main, Panel::Tasks)
    );

    let running = TransportRecording::default();
    running.set_alive(true);
    let controller = receiver_controller(&app, &running);
    let receiver = app
        .brain
        .add_receiver_run(
            receiver_job_id("416432be-1f80-4c14-a1cd-a67990cba013"),
            "Receiver · SMS".to_owned(),
            "receiver-instance".to_owned(),
            controller,
        )
        .expect("background receiver tab");

    assert_eq!(visible_state(&app), before, "inserting a background run");
    assert_eq!(app.brain.receiver_run_observations()[0].id, receiver);
    assert!(!app.brain.receiver_run_observations()[0].exited);

    running.set_alive(false);
    assert!(app.brain.receiver_run_observations()[0].exited);
    assert_eq!(visible_state(&app), before, "observing a failed run");

    let removed = app
        .brain
        .remove_receiver_run(receiver)
        .expect("remove background receiver run");
    assert_eq!(
        removed.job_id,
        receiver_job_id("416432be-1f80-4c14-a1cd-a67990cba013")
    );
    assert_eq!(removed.instance, "receiver-instance");
    assert_eq!(visible_state(&app), before, "removing a background run");

    crate::tui::state::exhaust_session_tab_ids(&mut app.brain);
    let rejected = TransportRecording::default();
    rejected.set_alive(true);
    let controller = receiver_controller(&app, &rejected);
    app.brain
        .add_receiver_run(
            receiver_job_id("1fe4e060-c513-48f7-a4a0-eb925ff884fc"),
            "Receiver · Email".to_owned(),
            "rejected-instance".to_owned(),
            controller,
        )
        .expect_err("exhausted receiver allocation");
    assert_eq!(rejected.shutdowns(), 1);
    assert_eq!(visible_state(&app), before, "rejecting a background run");
}

#[test]
fn receiver_controller_closes_independently_and_shell_shutdown_stops_the_rest() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    let (main, main_recording) = recording_controller(&app, true, "main");
    app.brain.install_main(main);
    let (skill, skill_recording) = recording_controller(&app, true, "skill");
    app.insert_test_skill_session(
        SkillSessionKey::DailyTriage,
        "Daily triage",
        "triage-token",
        skill,
    );
    let receiver_recording = TransportRecording::default();
    receiver_recording.set_alive(true);
    let controller = receiver_controller(&app, &receiver_recording);
    let receiver = app
        .brain
        .add_receiver_run(
            receiver_job_id("416432be-1f80-4c14-a1cd-a67990cba013"),
            "Receiver · SMS".to_owned(),
            "receiver-instance".to_owned(),
            controller,
        )
        .expect("receiver tab");

    app.brain
        .remove_receiver_run(receiver)
        .expect("receiver run");

    assert_eq!(receiver_recording.shutdowns(), 1);
    assert!(!main_recording.events().contains(&ControllerEvent::Shutdown));
    assert!(
        !skill_recording
            .events()
            .contains(&ControllerEvent::Shutdown)
    );

    let live_receiver_recording = TransportRecording::default();
    live_receiver_recording.set_alive(true);
    let controller = receiver_controller(&app, &live_receiver_recording);
    app.brain
        .add_receiver_run(
            receiver_job_id("1fe4e060-c513-48f7-a4a0-eb925ff884fc"),
            "Receiver · Email".to_owned(),
            "live-receiver-instance".to_owned(),
            controller,
        )
        .expect("live receiver tab");

    assert!(app.shutdown_agent_controllers().is_empty());

    assert!(main_recording.events().contains(&ControllerEvent::Shutdown));
    assert!(
        skill_recording
            .events()
            .contains(&ControllerEvent::Shutdown)
    );
    assert_eq!(receiver_recording.shutdowns(), 1);
    assert_eq!(live_receiver_recording.shutdowns(), 1);
}

#[test]
fn interleaved_receiver_tabs_use_the_same_order_for_strip_slots_and_cycles() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let (main, _) = recording_controller(&app, true, "main");
    app.brain.install_main(main);
    let (first_skill_controller, _) = recording_controller(&app, true, "first skill");
    let first_skill = app.insert_test_skill_session(
        SkillSessionKey::Custom(0),
        "First skill",
        "first-token",
        first_skill_controller,
    );
    let receiver_recording = TransportRecording::default();
    receiver_recording.set_alive(true);
    let controller = receiver_controller(&app, &receiver_recording);
    let receiver = app
        .brain
        .add_receiver_run(
            receiver_job_id("416432be-1f80-4c14-a1cd-a67990cba013"),
            "Receiver · SMS".to_owned(),
            "receiver-instance".to_owned(),
            controller,
        )
        .expect("receiver tab");
    let (second_skill_controller, _) = recording_controller(&app, true, "second skill");
    let second_skill = app.insert_test_skill_session(
        SkillSessionKey::Custom(1),
        "Second skill",
        "second-token",
        second_skill_controller,
    );

    assert_eq!(
        app.brain.tab_titles(),
        ["Brain", "First skill", "Receiver · SMS", "Second skill"]
    );
    assert!(app.select_brain_tab(BrainTab::Session(first_skill)));
    app.cycle_brain_tab(true);
    assert_eq!(app.effective_brain_tab(), BrainTab::Session(receiver));
    assert!(app.select_brain_tab_slot(3));
    assert_eq!(app.effective_brain_tab(), BrainTab::Session(second_skill));
}

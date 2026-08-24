use super::*;

#[test]
fn panel_activity_is_detected_the_same_way_for_every_frontend() {
    for agent_kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, agent_kind);
        let live = live_panel(app.context.workspace().root());
        let controller = panel_controller(&app, live);
        app.brain.install_main(controller);
        let start = std::time::Instant::now();
        let started = start
            .checked_sub(std::time::Duration::from_secs(3600))
            .expect("a turn opened an hour ago");
        let actor = app.brain.interactive_actor().clone();
        let job = receiver_job(&app, actor, Channel::Sms, "slow request");
        begin_receiver_turn(&mut app, &job, "slow-response", started);

        app.sample_panel_activity(start);
        let baseline = app
            .last_panel_change()
            .unwrap_or_else(|| panic!("{agent_kind:?} recorded no baseline"));

        app.sample_panel_activity(start + std::time::Duration::from_secs(4));
        assert_eq!(
            app.last_panel_change(),
            Some(baseline),
            "{agent_kind:?} treated a static panel as activity"
        );

        app.brain
            .main_controller_mut()
            .expect("panel")
            .type_text("working")
            .expect("render into the panel");
        assert!(
            wait_for_panel_contents(app.brain.main_controller().expect("panel"), "working"),
            "{agent_kind:?} panel never echoed"
        );
        let later = start + std::time::Duration::from_secs(8);
        app.sample_panel_activity(later);
        assert_eq!(
            app.last_panel_change(),
            Some(later),
            "{agent_kind:?} missed visible work"
        );

        assert!(
            !crate::tui::receiver::policy::abandons_stalled_turn(
                app.receiver.remote_started_at(),
                app.last_panel_change(),
                later + std::time::Duration::from_secs(10),
            ),
            "{agent_kind:?} abandoned a turn that was still working"
        );
        assert!(
            crate::tui::receiver::policy::abandons_stalled_turn(
                app.receiver.remote_started_at(),
                app.last_panel_change(),
                later + crate::tui::receiver::policy::ACTIVE_WORK_IDLE,
            ),
            "{agent_kind:?} never gave up on a panel that went quiet"
        );
    }
}

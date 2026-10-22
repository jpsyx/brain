use super::*;

#[test]
fn fresh_receiver_launch_uses_a_native_uuid_while_registration_stays_pending() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).expect("state DB");
        let inbound = receiver_job(&app, sms_actor(), Channel::Sms, "A remote request");
        let identity = crate::state::ReceiverConversationIdentity::sms(
            app.context.workspace().id(),
            inbound.actor.user_id().clone(),
        );
        db.accept_receiver_job(&inbound, &identity)
            .expect("accept request");
        let transport = TransportRecording::default();
        app.brain.replace_receiver_transport(transport.transport());

        app.tick_receiver();

        let active = app.receiver.active_durable_run().expect("receiver launch");
        assert!(
            active
                .attribution
                .registered_session()
                .as_str()
                .starts_with("pending-receiver-")
        );
        let specs = transport.launch_specs();
        let command = &specs[0].command;
        assert!(
            !command.contains("pending-receiver-"),
            "{kind:?} launch leaked its internal placeholder into frontend arguments"
        );
        if matches!(kind, AgentKind::Claude | AgentKind::Pi) {
            let session = command
                .split("--session-id ")
                .nth(1)
                .expect("native session argument")
                .split_whitespace()
                .next()
                .expect("native session value")
                .trim_matches('\'');
            assert!(
                uuid::Uuid::parse_str(session).is_ok(),
                "{kind:?} requires a UUID session ID"
            );
        }
    }
}

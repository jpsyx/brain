use super::*;

#[test]
fn wrong_workspace_actor_and_channel_cannot_rotate_a_registered_lineage() {
    for (name, value) in [
        ("BRAIN_WORKSPACE_ID", "22222222-2222-4222-8222-222222222222"),
        ("BRAIN_ACTOR_ID", "other-actor"),
        ("BRAIN_CHANNEL", "email"),
    ] {
        let (_temporary, db) = fresh_db();
        register_session(&db, "opencode", "pablo", "pending", "instance", 4242);
        let mut command = scoped_hook_command(&db, "opencode", "pablo", "instance");
        command.env(name, value);

        let output = run_hook_command(command, &start_input("forged-real"));

        assert!(output.status.success(), "{name} failed: {output:?}");
        assert!(read_session(&db, "forged-real").is_none(), "{name} rotated");
        assert_eq!(read_session(&db, "pending").unwrap().1, Some(4242));
    }
}

#[test]
fn child_session_start_payload_is_a_noop_for_every_frontend() {
    for agent_kind in ["claude", "codex", "opencode"] {
        let (_temporary, db) = fresh_db();
        register_session(&db, agent_kind, "pablo", "pending", "instance", 4242);
        let payload = serde_json::json!({
            "session_id": "child-session",
            "parent_session_id": "root-session",
            "source": "startup"
        })
        .to_string();

        let output = run_scoped_hook(&db, agent_kind, "pablo", "instance", &payload);

        assert!(output.status.success(), "{agent_kind} failed: {output:?}");
        assert!(read_session(&db, "child-session").is_none());
        assert_eq!(read_session(&db, "pending").unwrap().1, Some(4242));
    }
}

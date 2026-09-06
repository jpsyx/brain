use super::*;

#[test]
fn wrong_workspace_actor_and_channel_cannot_rotate_a_registered_lineage() {
    for agent_kind in ["claude", "codex", "opencode"] {
        for (name, value) in [
            ("BRAIN_WORKSPACE_ID", "22222222-2222-4222-8222-222222222222"),
            ("BRAIN_ACTOR_ID", "other-actor"),
            ("BRAIN_CHANNEL", "email"),
            ("BRAIN_AGENT_KIND", "unregistered-frontend"),
            ("BRAIN_INSTANCE_ID", "unregistered-instance"),
        ] {
            let (_temporary, db) = fresh_db();
            register_manual_session(&db, agent_kind, "instance", "pending", "Atlas", 1);
            let mut command = scoped_hook_command(&db, agent_kind, "pablo", "instance");
            command.env(name, value);

            let output = run_hook_command(command, &start_input("forged-real"));

            assert!(output.status.success(), "{name} failed: {output:?}");
            assert!(read_session(&db, "forged-real").is_none(), "{name} rotated");
            assert_eq!(read_session(&db, "pending").unwrap().1, Some(4242));
            assert_eq!(
                read_manual_native_id(&db, agent_kind, "instance"),
                Some("pending".to_owned())
            );
        }
    }
}

#[test]
fn child_session_start_payload_is_a_noop_for_every_frontend() {
    for agent_kind in ["claude", "codex", "opencode"] {
        let (_temporary, db) = fresh_db();
        register_manual_session(&db, agent_kind, "instance", "pending", "Atlas", 1);
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
        assert_eq!(
            read_manual_native_id(&db, agent_kind, "instance"),
            Some("pending".to_owned())
        );
    }
}

#[test]
fn a_fork_cannot_displace_a_manual_mapping_for_any_frontend() {
    for agent_kind in ["claude", "codex", "opencode"] {
        let (_temporary, db) = fresh_db();
        register_manual_session(&db, agent_kind, "instance", "pending", "Atlas", 1);

        let output = run_scoped_hook(&db, agent_kind, "pablo", "instance", &fork_input("forked"));

        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        assert!(read_session(&db, "forked").is_none());
        assert_eq!(read_session(&db, "pending").unwrap().1, Some(4242));
        assert_eq!(
            read_manual_native_id(&db, agent_kind, "instance"),
            Some("pending".to_owned())
        );
    }
}

#[test]
fn an_authorized_rotation_does_not_update_a_manual_mapping_in_another_scope() {
    for (column, value) in [
        ("agent_kind", "codex"),
        ("workspace_id", "22222222-2222-4222-8222-222222222222"),
        ("actor_id", "other-actor"),
    ] {
        let (_temporary, db) = fresh_db();
        register_manual_session(&db, "claude", "manual-id", "foreign-pending", "Atlas", 1);
        let connection = Connection::open(&db).unwrap();
        connection
            .execute_batch("BEGIN; PRAGMA defer_foreign_keys = ON;")
            .unwrap();
        for table in ["brain_sessions", "manual_sessions"] {
            connection
                .execute(&format!("UPDATE {table} SET {column} = ?1"), [value])
                .unwrap();
        }
        connection.execute_batch("COMMIT;").unwrap();
        register_session(&db, "claude", "pablo", "pending", "manual-id", 4242);

        let output = run_scoped_hook(&db, "claude", "pablo", "manual-id", &start_input("rotated"));

        assert!(output.status.success());
        assert_eq!(read_session(&db, "rotated").unwrap().1, Some(4242));
        let mapped: String = connection
            .query_row("SELECT agent_session_id FROM manual_sessions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(mapped, "foreign-pending", "{column} mapping was displaced");
    }
}

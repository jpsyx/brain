use super::*;

#[test]
fn hook_without_instance_env_is_noop() {
    let (_tmp, db) = fresh_db();
    let out = run_hook(&db, None, &start_input("claude-xyz"));
    assert!(out.status.success(), "hook exited non-zero: {out:?}");
    assert!(
        read_session(&db, "claude-xyz").is_none(),
        "ambient claude run must not record a session"
    );
}

#[test]
fn hook_rejects_an_unregistered_workspace_session_tuple() {
    let (_tmp, db) = fresh_db();

    let out = run_hook(
        &db,
        Some(("unregistered-shell", 4242)),
        &start_input("unregistered-session"),
    );

    assert!(out.status.success(), "hook exited non-zero: {out:?}");
    assert!(
        read_session(&db, "unregistered-session").is_none(),
        "hook events cannot create an unregistered workspace/session tuple"
    );
}

#[test]
fn normalized_session_id_records_exact_identity_for_every_frontend() {
    for agent_kind in ["claude", "codex", "opencode"] {
        let (_tmp, db) = fresh_db();
        let pending = format!("pending-{agent_kind}");
        let real = format!("real-{agent_kind}");
        let instance = format!("instance-{agent_kind}");
        register_session(&db, agent_kind, "pablo", &pending, &instance, 4242);

        let out = run_scoped_hook(&db, agent_kind, "pablo", &instance, &start_input(&real));

        assert!(
            out.status.success(),
            "{agent_kind} hook failed: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(read_session(&db, &pending).unwrap().1, None);
        assert_eq!(
            read_session(&db, &real),
            Some((
                instance,
                Some(4242),
                agent_kind.to_owned(),
                "11111111-1111-4111-8111-111111111111".to_owned(),
                "pablo".to_owned(),
                "interactive".to_owned(),
            ))
        );
    }
}

#[test]
fn hook_without_complete_workspace_identity_is_noop() {
    let (_tmp, db) = fresh_db();
    let mut cmd = Command::new("python3");
    cmd.arg(hook_script());
    cmd.env("BRAIN_WORKSPACE_ID", "11111111-1111-4111-8111-111111111111");
    cmd.env("BRAIN_WORKSPACE", "family");
    cmd.env("BRAIN_ROOT", "/tmp/family");
    cmd.env_remove("BRAIN_ACTOR_ID");
    cmd.env("BRAIN_CHANNEL", "interactive");
    cmd.env("BRAIN_AGENT_KIND", "claude");
    cmd.env("BRAIN_INSTANCE_ID", "inst-1");
    cmd.env("BRAIN_PID", "4242");
    cmd.env("BRAIN_STATE_DB", &db);

    let out = run_hook_command(cmd, &start_input("claude-incomplete"));

    assert!(out.status.success(), "hook exited non-zero: {out:?}");
    assert!(
        read_session(&db, "claude-incomplete").is_none(),
        "a hook without the complete selected-workspace identity must not write"
    );
}

#[test]
fn new_rotation_frees_the_prior_session_for_the_same_instance() {
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "sess-A", "inst-1", 4242);
    // First session for the instance.
    run_hook(&db, Some(("inst-1", 4242)), &start_input("sess-A"));
    // `/new` rotates to a fresh session id; the hook fires again.
    run_hook(&db, Some(("inst-1", 4242)), &start_input("sess-B"));

    let a = read_session(&db, "sess-A").expect("A still present");
    let b = read_session(&db, "sess-B").expect("B recorded");
    assert_eq!(a.1, None, "the prior session is unlocked (resumable later)");
    assert_eq!(b.1, Some(4242), "the current session stays locked");
}

#[test]
fn re_firing_the_same_session_keeps_it_locked() {
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "sess-A", "inst-1", 4242);
    run_hook(&db, Some(("inst-1", 4242)), &start_input("sess-A"));
    // Resume / compact fires SessionStart again for the same id.
    run_hook(&db, Some(("inst-1", 4242)), &start_input("sess-A"));
    let a = read_session(&db, "sess-A").expect("A present");
    assert_eq!(a.1, Some(4242), "still locked to this instance");
}

#[test]
fn hook_with_malformed_stdin_is_noop_not_error() {
    let (_tmp, db) = fresh_db();
    let out = run_hook(&db, Some(("inst-1", 4242)), "not even json{");
    assert!(out.status.success(), "hook must not error on bad stdin");
}

#[test]
fn distinct_instances_get_distinct_locked_sessions() {
    // Two tasks shells each record their own session; neither frees the
    // other's (the /new free pass is scoped to the firing instance).
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "sess-1", "inst-1", 10);
    register_session(&db, "claude", "pablo", "sess-2", "inst-2", 20);
    run_hook(&db, Some(("inst-1", 10)), &start_input("sess-1"));
    run_hook(&db, Some(("inst-2", 20)), &start_input("sess-2"));
    assert_eq!(read_session(&db, "sess-1").unwrap().1, Some(10));
    assert_eq!(read_session(&db, "sess-2").unwrap().1, Some(20));
}

#[test]
fn rotation_cannot_steal_a_session_registered_to_another_live_lineage() {
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "sess-1", "inst-1", 10);
    register_session(&db, "claude", "pablo", "sess-2", "inst-2", 20);

    let out = run_hook(&db, Some(("inst-1", 10)), &start_input("sess-2"));

    assert!(out.status.success(), "hook exited non-zero: {out:?}");
    let first = read_session(&db, "sess-1").expect("first lineage preserved");
    let second = read_session(&db, "sess-2").expect("second lineage preserved");
    assert_eq!((first.0.as_str(), first.1), ("inst-1", Some(10)));
    assert_eq!((second.0.as_str(), second.1), ("inst-2", Some(20)));
}

#[test]
fn hook_preserves_equal_opaque_ids_with_conflicting_immutable_attribution() {
    let (_tmp, db) = fresh_db();
    register_session(
        &db,
        "claude",
        "pablo",
        "same-opaque-id",
        "claude-instance",
        4242,
    );
    register_session(
        &db,
        "codex",
        "partner",
        "same-opaque-id",
        "codex-instance",
        4242,
    );
    let first = run_scoped_hook(
        &db,
        "claude",
        "pablo",
        "claude-instance",
        &start_input("same-opaque-id"),
    );
    let second = run_scoped_hook(
        &db,
        "codex",
        "partner",
        "codex-instance",
        &serde_json::json!({
            "thread_id": "same-opaque-id",
            "source": "startup",
            "hook_event_name": "SessionStart"
        })
        .to_string(),
    );
    assert!(first.status.success());
    assert!(second.status.success());

    let conn = Connection::open(db).unwrap();
    let mut statement = conn
        .prepare(
            "SELECT agent_kind, actor_id FROM brain_sessions
             WHERE agent_session_id = 'same-opaque-id'
             ORDER BY agent_kind",
        )
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("claude".to_owned(), "pablo".to_owned()),
            ("codex".to_owned(), "partner".to_owned()),
        ]
    );
}

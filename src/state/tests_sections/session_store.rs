use crate::agent::{AgentSession, CompletionStatus, SessionStore};

fn scope() -> SessionScope {
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: crate::users::UserId::parse("test-user").unwrap(),
            name: "Test user".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let actor = crate::actor::resolve_actor(
        &crate::users::UserId::parse("test-user").unwrap(),
        crate::actor::RequestIdentity::Local,
        &users,
    )
    .unwrap();
    SessionScope::new(
        crate::session::AgentKind::Claude,
        crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        actor,
    )
}

#[test]
fn session_store_tracks_attribution_and_completion_for_each_frontend() {
    let db = Db::open_in_memory().unwrap();

    for kind in [
        crate::session::AgentKind::Claude,
        crate::session::AgentKind::Codex,
    ] {
        let base_scope = scope();
        let scope = SessionScope::new(kind, base_scope.workspace_id(), base_scope.actor().clone());
        let session = AgentSession::new(format!("{}-session", kind.as_str())).unwrap();

        SessionStore::register(&db, &session, "shell", 42, &scope).unwrap();

        assert_eq!(
            SessionStore::completion_status(&db, &session, &scope),
            Some(CompletionStatus::Active)
        );
        assert!(SessionStore::mark_completed(&db, &session, &scope).unwrap());
        assert_eq!(
            SessionStore::completion_status(&db, &session, &scope),
            Some(CompletionStatus::Completed)
        );
        assert!(SessionStore::mark_active(&db, "shell", &scope).unwrap());
        assert_eq!(
            SessionStore::completion_status(&db, &session, &scope),
            Some(CompletionStatus::Active)
        );
    }
}

/// Insert a session row directly with an explicit `last_active`,
/// bypassing the clock, for ordering tests.
fn seed(db: &Db, id: &str, instance: &str, locked: Option<i32>, last_active: i64) {
    db.conn
        .execute(
            "INSERT INTO brain_sessions
                   (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                    workspace_id, actor_id, channel, created_at, last_active_at)
                 VALUES ('claude', ?1, ?2, ?3, 'seed', '8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b',
                         'test-user', 'interactive', ?4, ?4)",
            rusqlite::params![id, instance, locked, last_active],
        )
        .unwrap();
}

#[test]
fn free_sessions_is_empty_on_an_empty_db() {
    let db = Db::open_in_memory().unwrap();
    assert!(db.sessions_by_recency(&scope()).is_empty());
}

#[test]
fn free_sessions_are_ordered_newest_first_and_skip_locked() {
    let db = Db::open_in_memory().unwrap();
    seed(&db, "old", "i1", None, 100);
    seed(&db, "new", "i1", None, 200);
    seed(&db, "locked-newer", "i2", Some(4242), 300);
    // The locked one is newer but held by a live shell, so it's excluded;
    // the rest come newest-first.
    assert_eq!(db.sessions_by_recency(&scope()), vec!["new", "old"]);
}

#[test]
fn register_fresh_then_release_makes_it_resumable() {
    let db = Db::open_in_memory().unwrap();
    db.register_scoped_fresh("s1", "i1", 999, &scope()).unwrap();
    // While locked, nothing is free to resume.
    assert!(db.sessions_by_recency(&scope()).is_empty());
    db.release("i1").unwrap();
    assert_eq!(db.sessions_by_recency(&scope()), vec!["s1"]);
}

#[test]
fn locked_session_for_instance_is_scoped_and_tracks_frontend_rotation() {
    let db = Db::open_in_memory().unwrap();
    let selected = scope();
    db.register_scoped_fresh("placeholder", "shell", 999, &selected)
        .unwrap();
    db.conn
        .execute(
            "UPDATE brain_sessions SET agent_session_id = 'frontend-real-id'
             WHERE brain_instance_id = 'shell'",
            [],
        )
        .unwrap();

    assert_eq!(
        db.locked_session_for_instance("shell", &selected)
            .as_deref(),
        Some("frontend-real-id")
    );
    let other = SessionScope::new(
        crate::session::AgentKind::OpenCode,
        selected.workspace_id(),
        selected.actor().clone(),
    );
    assert!(db.locked_session_for_instance("shell", &other).is_none());
}

#[test]
fn claim_wins_once_then_loses_on_a_held_session() {
    let db = Db::open_in_memory().unwrap();
    seed(&db, "s1", "i0", None, 100);
    assert!(
        db.claim("s1", "i1", 111, &scope()).unwrap(),
        "first claim wins"
    );
    assert!(
        !db.claim("s1", "i2", 222, &scope()).unwrap(),
        "a held session can't be claimed again"
    );
}

#[test]
fn reap_dead_locks_frees_sessions_held_by_dead_pids() {
    // pid 1 is "dead", everything else alive.
    let db = Db::open_in_memory().unwrap().with_pid_alive(|pid| pid != 1);
    seed(&db, "dead", "i1", Some(1), 100);
    seed(&db, "alive", "i2", Some(2), 200);
    db.reap_dead_locks().unwrap();
    // The dead-held session is now resumable; the live-held one is not.
    assert_eq!(db.sessions_by_recency(&scope()), vec!["dead"]);
}

#[test]
fn two_shells_take_distinct_sessions() {
    // Shell A claims the only free session; shell B must find nothing
    // free and would start fresh, never sharing A's thread.
    let db = Db::open_in_memory().unwrap();
    seed(&db, "s1", "i0", None, 100);
    let a = db.sessions_by_recency(&scope()).into_iter().next().unwrap();
    assert!(db.claim(&a, "A", 10, &scope()).unwrap());
    assert!(
        db.sessions_by_recency(&scope()).is_empty(),
        "B sees nothing free"
    );
}

#[test]
fn panel_side_defaults_to_right_and_round_trips() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.get_panel_side(), PanelSide::Right);
    db.set_panel_side(PanelSide::Left).unwrap();
    assert_eq!(db.get_panel_side(), PanelSide::Left);
    db.set_panel_side(PanelSide::Right).unwrap();
    assert_eq!(db.get_panel_side(), PanelSide::Right);
}

#[test]
fn panel_side_flip_is_symmetric() {
    assert_eq!(PanelSide::Left.flipped(), PanelSide::Right);
    assert_eq!(PanelSide::Right.flipped(), PanelSide::Left);
}

#[test]
fn skills_synced_version_is_absent_then_round_trips() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.skills_synced_version(), None);
    db.set_skills_synced_version("0.18.0").unwrap();
    assert_eq!(db.skills_synced_version().as_deref(), Some("0.18.0"));
    // A later render overwrites it in place.
    db.set_skills_synced_version("0.19.0").unwrap();
    assert_eq!(db.skills_synced_version().as_deref(), Some("0.19.0"));
}

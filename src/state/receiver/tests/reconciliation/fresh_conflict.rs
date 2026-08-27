struct FreshConflictFixture {
    db: Db,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    conversation_id: ReceiverConversationId,
    scope: crate::agent::SessionScope,
    prior: crate::agent::AgentSession,
    placeholder: crate::agent::AgentSession,
    native: crate::agent::AgentSession,
}

impl FreshConflictFixture {
    fn new(db: Db, provider_id: &str, actual_conflict: bool) -> Self {
        let inbound = receiver_job(Some(provider_id), 100);
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let accepted = db
            .accept_receiver_job(&inbound, &identity)
            .expect("accept receiver job");
        let scope = crate::agent::SessionScope::new(
            crate::agent::AgentKind::Codex,
            inbound.workspace_id,
            inbound.actor,
        );
        let prior = crate::agent::AgentSession::new("prior-native-session")
            .expect("prior native session");
        crate::agent::SessionStore::register(&db, &prior, "prior-instance", 41, &scope)
            .expect("register prior native session");
        crate::agent::SessionStore::release(&db, "prior-instance")
            .expect("leave prior native session resumable");
        let binding = ReceiverSessionBinding::new(crate::agent::AgentKind::Codex, prior.as_str())
            .expect("prior receiver binding");
        db.update_receiver_conversation(
            accepted.conversation_id(),
            "portable transcript",
            Some(&binding),
            1_000,
        )
        .expect("retain prior receiver binding");

        db.claim_next_receiver_run("ordinary-owner", 1_000, 2_000)
            .expect("claim ordinary run")
            .expect("ordinary run");
        assert!(
            db.prepare_receiver_job_launch(accepted.job_id(), "ordinary-owner", 1_100)
                .expect("prepare ordinary launch")
        );
        let placeholder = crate::agent::AgentSession::new("pending-receiver-fresh-conflict")
            .expect("fresh placeholder");
        let instance = "fresh-conflict-instance";
        db.register_receiver_session(
            accepted.conversation_id(),
            &placeholder,
            instance,
            42,
            &scope,
        )
        .expect("register exact fresh fallback");
        let token = db
            .receiver_job(accepted.job_id())
            .expect("load launching job")
            .expect("launching job")
            .token();
        assert!(
            db.commit_receiver_job_launch(
                accepted.job_id(),
                "ordinary-owner",
                &launch_observation(token, instance, placeholder.as_str(), 1_200),
            )
            .expect("commit fresh launch")
        );
        let native = crate::agent::AgentSession::new("observed-native-session")
            .expect("observed native session");
        crate::agent::SessionStore::register(&db, &native, instance, 42, &scope)
            .expect("register lifecycle-native session");
        db.conn
            .execute(
                "UPDATE brain_sessions SET locked_pid = NULL
                 WHERE workspace_id = ?1 AND brain_instance_id = ?2
                   AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
                   AND agent_session_id = ?6",
                rusqlite::params![
                    receiver_workspace_id().to_string(),
                    instance,
                    scope.agent_kind().as_str(),
                    scope.actor().user_id().as_str(),
                    scope.actor().channel().as_str(),
                    placeholder.as_str(),
                ],
            )
            .expect("rotate placeholder to lifecycle-native session");
        if actual_conflict {
            db.conn
                .execute(
                    "UPDATE receiver_session_registrations SET actual_session_id = ?1
                     WHERE workspace_id = ?2 AND conversation_id = ?3
                       AND brain_instance_id = ?4 AND registered_session_id = ?5",
                    rusqlite::params![
                        prior.as_str(),
                        receiver_workspace_id().to_string(),
                        accepted.conversation_id().to_string(),
                        instance,
                        placeholder.as_str(),
                    ],
                )
                .expect("establish conflicting registration actual session");
        }
        assert!(
            db.apply_receiver_observation(
                accepted.job_id(),
                "ordinary-owner",
                &observation(
                    token,
                    instance,
                    native.as_str(),
                    ReceiverNonterminalObservationPhase::Accepted,
                    1,
                    1_300,
                ),
            )
            .expect("persist accepted lifecycle observation")
        );
        Self {
            db,
            job_id: accepted.job_id(),
            token,
            conversation_id: accepted.conversation_id(),
            scope,
            prior,
            placeholder,
            native,
        }
    }

    fn reconcile(&self, now_unix_ms: u64) -> ReceiverReconciliationEffect {
        self.db
            .reconcile_next_receiver_job(now_unix_ms)
            .expect("reconcile fresh conflict")
            .expect("fresh conflict effect")
    }
}

#[test]
fn prior_bound_fresh_rotation_stall_has_one_exact_releasable_cleanup() {
    let fixture = FreshConflictFixture::new(
        Db::open_in_memory().expect("receiver state"),
        "prior-bound-fresh-stall",
        false,
    );
    let later = receiver_job(Some("later-after-fresh-conflict"), 200);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let later = fixture
        .db
        .accept_receiver_job(&later, &identity)
        .expect("accept later FIFO work");

    let effect = fixture.reconcile(301_300);

    assert_fresh_conflict_terminal_effect(&fixture, &effect);
    assert_eq!(fixture.registration_actual(), None);
    assert!(!fixture.acknowledge(
        fixture.token,
        "wrong-instance",
        fixture.native.as_str()
    ));
    assert!(!fixture.acknowledge(
        fixture.token,
        "fresh-conflict-instance",
        "wrong-session"
    ));
    let wrong_token = ReceiverJobToken::new();
    assert!(!fixture.acknowledge(
        wrong_token,
        "fresh-conflict-instance",
        fixture.native.as_str()
    ));
    assert!(!fixture
        .db
        .acknowledge_receiver_recovery_cleanup(
            ReceiverJobId::from(uuid::Uuid::new_v4()),
            fixture.token,
            "fresh-conflict-instance",
            fixture.native.as_str(),
            301_401,
        )
        .expect("wrong job cannot acknowledge fresh-conflict cleanup"));
    let unrelated = crate::agent::AgentSession::new("unrelated-native-session")
        .expect("unrelated native session");
    fixture
        .db
        .register_receiver_session(
            fixture.conversation_id,
            &unrelated,
            "unrelated-instance",
            43,
            &fixture.scope,
        )
        .expect("register unrelated receiver session");
    assert!(fixture.acknowledge(
        fixture.token,
        "fresh-conflict-instance",
        fixture.native.as_str()
    ));
    assert_eq!(fixture.session_lock(&fixture.native), None);
    assert_eq!(fixture.session_lock(&fixture.prior), None);
    assert_eq!(fixture.session_lock(&unrelated), Some(43));
    assert!(fixture.registration_exists("unrelated-instance", unrelated.as_str()));
    assert_eq!(
        fixture
            .db
            .claim_next_receiver_run("later-owner", 301_402, 331_402)
            .expect("claim later FIFO work")
            .expect("fresh conflict does not fail stuck")
            .job()
            .id(),
        later.job_id()
    );
}

#[test]
fn prior_bound_fresh_rotation_absolute_expiry_has_one_exact_releasable_cleanup() {
    let fixture = FreshConflictFixture::new(
        Db::open_in_memory().expect("receiver state"),
        "prior-bound-fresh-expiry",
        false,
    );
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_jobs SET absolute_work_expires_at_unix_ms = 1_300
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3",
            rusqlite::params![
                receiver_workspace_id().to_string(),
                fixture.job_id.to_string(),
                fixture.token.to_string(),
            ],
        )
        .expect("expire exact accepted fresh fallback");

    let effect = fixture.reconcile(1_300);

    assert_fresh_conflict_terminal_effect(&fixture, &effect);
    assert!(fixture.acknowledge(
        fixture.token,
        "fresh-conflict-instance",
        fixture.native.as_str()
    ));
    assert_eq!(fixture.session_lock(&fixture.native), None);
    assert_eq!(fixture.session_lock(&fixture.prior), None);
}

#[test]
fn conflicting_registration_actual_is_preserved_until_exact_fresh_cleanup_release() {
    let fixture = FreshConflictFixture::new(
        Db::open_in_memory().expect("receiver state"),
        "fresh-conflicting-actual",
        true,
    );

    let effect = fixture.reconcile(301_300);

    assert_fresh_conflict_terminal_effect(&fixture, &effect);
    assert_eq!(
        fixture.registration_actual().as_deref(),
        Some(fixture.prior.as_str())
    );
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_session_registrations SET actual_session_id = 'changed-actual'
             WHERE workspace_id = ?1 AND conversation_id = ?2
               AND brain_instance_id = 'fresh-conflict-instance'
               AND registered_session_id = ?3",
            rusqlite::params![
                receiver_workspace_id().to_string(),
                fixture.conversation_id.to_string(),
                fixture.placeholder.as_str(),
            ],
        )
        .expect("change actual session after reconciliation");
    assert!(!fixture.acknowledge(
        fixture.token,
        "fresh-conflict-instance",
        fixture.native.as_str()
    ));
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_session_registrations SET actual_session_id = ?1
             WHERE workspace_id = ?2 AND conversation_id = ?3
               AND brain_instance_id = 'fresh-conflict-instance'
               AND registered_session_id = ?4",
            rusqlite::params![
                fixture.prior.as_str(),
                receiver_workspace_id().to_string(),
                fixture.conversation_id.to_string(),
                fixture.placeholder.as_str(),
            ],
        )
        .expect("restore reconciled actual-session fence");
    assert!(fixture.acknowledge(
        fixture.token,
        "fresh-conflict-instance",
        fixture.native.as_str()
    ));
    assert_eq!(fixture.session_lock(&fixture.native), None);
    assert_eq!(fixture.session_lock(&fixture.prior), None);
}

#[test]
fn fresh_conflict_restart_proof_requires_the_exact_dead_lifecycle_row() {
    for actual_conflict in [false, true] {
        let temporary = tempfile::tempdir().expect("temporary receiver state");
        let path = temporary.path().join("state.db");
        let db = Db::open_path_with_legacy_identity(
                &path,
                &receiver_workspace_id().to_string(),
                receiver_user_id().as_str(),
            )
            .expect("open receiver state")
            .with_pid_alive(|pid| pid == 42);
        let fixture = FreshConflictFixture::new(
            db,
            "fresh-conflict-restart",
            actual_conflict,
        );
        let effect = fixture.reconcile(301_300);
        assert_fresh_conflict_terminal_effect(&fixture, &effect);
        assert!(!fixture
            .db
            .receiver_cleanup_registration_is_stale(&effect)
            .expect("live PID rejects restart cleanup"));
        fixture
            .db
            .conn
            .execute(
                "UPDATE brain_sessions SET locked_pid = 999999
                 WHERE workspace_id = ?1 AND brain_instance_id = ?2
                   AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
                   AND agent_session_id = ?6",
                rusqlite::params![
                    receiver_workspace_id().to_string(),
                    "fresh-conflict-instance",
                    fixture.scope.agent_kind().as_str(),
                    fixture.scope.actor().user_id().as_str(),
                    fixture.scope.actor().channel().as_str(),
                    fixture.native.as_str(),
                ],
            )
            .expect("mark exact lifecycle row stale");
        drop(fixture);

        let reopened = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("reopen receiver state")
        .with_pid_alive(|_| false);
        assert!(reopened
            .receiver_cleanup_registration_is_stale(&effect)
            .expect("exact dead lifecycle row permits restart cleanup"));
        assert!(reopened
            .acknowledge_receiver_recovery_cleanup(
                effect.job_id(),
                effect.token(),
                effect.cleanup_instance().expect("cleanup instance"),
                effect.cleanup_session_id().expect("cleanup session"),
                301_402,
            )
            .expect("release restarted fresh-conflict cleanup"));
        let cleaned = reopened
            .receiver_job(effect.job_id())
            .expect("load restarted cleanup job")
            .expect("restarted cleanup job");
        assert_eq!(cleaned.recovery_cleanup_instance(), None);
        assert_eq!(cleaned.recovery_cleanup_session_id(), None);
    }
}

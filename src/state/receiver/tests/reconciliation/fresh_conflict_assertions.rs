impl FreshConflictFixture {
    fn assert_prior_binding_preserved(&self) {
        let conversation = self
            .db
            .receiver_conversation(self.conversation_id)
            .expect("load retained conversation")
            .expect("retained conversation");
        assert_eq!(
            conversation.binding(),
            Some(
                &ReceiverSessionBinding::new(
                    crate::agent::AgentKind::Codex,
                    self.prior.as_str(),
                )
                .expect("expected prior binding")
            )
        );
    }

    fn registration_actual(&self) -> Option<String> {
        self.db
            .conn
            .query_row(
                "SELECT actual_session_id FROM receiver_session_registrations
                 WHERE workspace_id = ?1 AND conversation_id = ?2
                   AND brain_instance_id = 'fresh-conflict-instance'
                   AND registered_session_id = ?3",
                rusqlite::params![
                    receiver_workspace_id().to_string(),
                    self.conversation_id.to_string(),
                    self.placeholder.as_str(),
                ],
                |row| row.get(0),
            )
            .expect("load fresh-conflict registration")
    }

    fn registration_exists(&self, instance: &str, registered_session: &str) -> bool {
        self.db
            .conn
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM receiver_session_registrations
                     WHERE workspace_id = ?1 AND conversation_id = ?2
                       AND brain_instance_id = ?3 AND registered_session_id = ?4
                 )",
                rusqlite::params![
                    receiver_workspace_id().to_string(),
                    self.conversation_id.to_string(),
                    instance,
                    registered_session,
                ],
                |row| row.get(0),
            )
            .expect("check exact receiver registration")
    }

    fn session_lock(&self, session: &crate::agent::AgentSession) -> Option<i64> {
        self.db
            .conn
            .query_row(
                "SELECT locked_pid FROM brain_sessions
                 WHERE workspace_id = ?1 AND agent_kind = ?2
                   AND actor_id = ?3 AND channel = ?4
                   AND agent_session_id = ?5",
                rusqlite::params![
                    receiver_workspace_id().to_string(),
                    self.scope.agent_kind().as_str(),
                    self.scope.actor().user_id().as_str(),
                    self.scope.actor().channel().as_str(),
                    session.as_str(),
                ],
                |row| row.get(0),
            )
            .expect("load exact session lock")
    }

    fn acknowledge(&self, token: ReceiverJobToken, instance: &str, session: &str) -> bool {
        self.db
            .acknowledge_receiver_recovery_cleanup(
                self.job_id,
                token,
                instance,
                session,
                301_401,
            )
            .expect("acknowledge fresh-conflict cleanup")
    }
}

fn assert_fresh_conflict_terminal_effect(
    fixture: &FreshConflictFixture,
    effect: &ReceiverReconciliationEffect,
) {
    assert_eq!(effect.action(), ReceiverReconciliationAction::TerminalFailure);
    assert_eq!(
        effect.reason(),
        ReceiverReconciliationReason::NativeSessionUnavailable
    );
    assert_eq!(effect.cleanup_instance(), Some("fresh-conflict-instance"));
    assert_eq!(effect.cleanup_session_id(), Some(fixture.native.as_str()));
    let terminal = fixture
        .db
        .receiver_job(fixture.job_id)
        .expect("load terminal fresh conflict")
        .expect("terminal fresh conflict");
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert!(terminal.pending_unavailable_notice());
    assert_eq!(
        terminal.recovery_cleanup_instance(),
        Some("fresh-conflict-instance")
    );
    assert_eq!(
        terminal.recovery_cleanup_session_id(),
        Some(fixture.native.as_str())
    );
    fixture.assert_prior_binding_preserved();
    assert_eq!(fixture.session_lock(&fixture.prior), None);
    assert_eq!(fixture.session_lock(&fixture.native), Some(42));
}

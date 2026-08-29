impl Db {
    /// Open or create a state DB at `path`. Runs migrations idempotently and
    /// enables WAL mode so concurrent shells + the hook don't block.
    pub fn open(workspace: &crate::workspace::WorkspaceContext) -> Result<Self> {
        Self::open_path_with_legacy_identity(
            &workspace.paths().state_db(),
            &workspace.id().to_string(),
            workspace.local_user_id(),
        )
    }

    pub(crate) fn open_for_receiver_ingress(
        workspace: &crate::workspace::WorkspaceContext,
        busy_timeout: std::time::Duration,
    ) -> Result<Self> {
        Self::open_path_with_legacy_identity_and_busy_timeout(
            &workspace.paths().state_db(),
            &workspace.id().to_string(),
            workspace.local_user_id(),
            busy_timeout,
        )
    }

    pub(crate) fn rebind_receiver_ingress_busy_timeout(
        &self,
        busy_timeout: std::time::Duration,
    ) -> Result<()> {
        self.conn.busy_timeout(busy_timeout)?;
        Ok(())
    }

    /// Open or create a state DB at an explicit path.
    pub fn open_path(path: &Path) -> Result<Self> {
        Self::open_path_with_legacy_identity(path, "legacy-workspace", "legacy-user")
    }

    /// Open a DB while supplying the attribution applied to pre-actor rows.
    pub fn open_path_with_legacy_identity(
        path: &Path,
        workspace_id: &str,
        local_actor_id: &str,
    ) -> Result<Self> {
        Self::open_path_with_legacy_identity_and_busy_timeout(
            path,
            workspace_id,
            local_actor_id,
            std::time::Duration::from_secs(5),
        )
    }

    fn open_path_with_legacy_identity_and_busy_timeout(
        path: &Path,
        workspace_id: &str,
        local_actor_id: &str,
        busy_timeout: std::time::Duration,
    ) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening sqlite at {}", path.display()))?;
        Self::configure_with_busy_timeout(&conn, busy_timeout)?;
        let db = Self {
            conn,
            workspace_id: workspace_id.to_owned(),
            clock: system_clock,
            pid_alive: system_pid_alive,
        };
        db.migrate(workspace_id, local_actor_id)?;
        Ok(db)
    }

    /// In-memory DB for tests: fresh, migrated, deterministic clock, all
    /// pids considered alive (tests opt into deadness via `with_pid_alive`).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        let db = Self {
            conn,
            workspace_id: "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b".to_owned(),
            clock: || 1_000_000,
            pid_alive: |_| true,
        };
        db.migrate("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b", "test-user")?;
        Ok(db)
    }

    #[cfg(test)]
    #[must_use]
    pub fn with_pid_alive(mut self, pid_alive: PidAlive) -> Self {
        self.pid_alive = pid_alive;
        self
    }

    pub(in crate::state) fn configure(conn: &Connection) -> Result<()> {
        Self::configure_with_busy_timeout(conn, std::time::Duration::from_secs(5))
    }

    fn configure_with_busy_timeout(
        conn: &Connection,
        busy_timeout: std::time::Duration,
    ) -> Result<()> {
        conn.busy_timeout(busy_timeout)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        Ok(())
    }

    fn migrate(&self, workspace_id: &str, local_actor_id: &str) -> Result<()> {
        let version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "BEGIN;
                CREATE TABLE IF NOT EXISTS brain_sessions (
                  claude_session_id  TEXT PRIMARY KEY,
                  brain_instance_id  TEXT NOT NULL,
                  locked_pid         INTEGER,
                  source             TEXT,
                  created_at         INTEGER NOT NULL,
                  last_active_at     INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS brain_sessions_by_active
                  ON brain_sessions(locked_pid, last_active_at);
                CREATE TABLE IF NOT EXISTS meta (
                  key    TEXT PRIMARY KEY,
                  value  TEXT NOT NULL
                );
                PRAGMA user_version = 1;
                COMMIT;",
            )?;
        }
        if version < 2 {
            self.conn.execute_batch(
                "ALTER TABLE brain_sessions ADD COLUMN channel TEXT NOT NULL DEFAULT 'interactive';
                 PRAGMA user_version = 2;",
            )?;
        }
        if version < 3 {
            let transaction = self.conn.unchecked_transaction()?;
            transaction.execute_batch(
                "ALTER TABLE brain_sessions RENAME TO brain_sessions_legacy;
                 DROP INDEX IF EXISTS brain_sessions_by_active;
                 CREATE TABLE brain_sessions (
                   agent_kind       TEXT NOT NULL,
                   agent_session_id TEXT PRIMARY KEY,
                   brain_instance_id TEXT NOT NULL,
                   locked_pid       INTEGER,
                   source           TEXT,
                   workspace_id     TEXT NOT NULL,
                   actor_id         TEXT NOT NULL,
                   channel          TEXT NOT NULL,
                   created_at       INTEGER NOT NULL,
                   last_active_at   INTEGER NOT NULL
                 );
                 CREATE INDEX brain_sessions_by_active
                   ON brain_sessions(agent_kind, workspace_id, actor_id, channel,
                                     locked_pid, last_active_at);",
            )?;
            transaction.execute(
                "INSERT INTO brain_sessions
                   (agent_kind, agent_session_id, brain_instance_id, locked_pid,
                    source, workspace_id, actor_id, channel, created_at, last_active_at)
                 SELECT 'claude', claude_session_id, brain_instance_id, locked_pid,
                        source, ?1, ?2, 'interactive', created_at, last_active_at
                 FROM brain_sessions_legacy",
                rusqlite::params![workspace_id, local_actor_id],
            )?;
            transaction.execute_batch(
                "DROP TABLE brain_sessions_legacy;
                 PRAGMA user_version = 3;",
            )?;
            transaction.commit()?;
        }
        if version < 4 {
            let transaction = self.conn.unchecked_transaction()?;
            transaction.execute_batch(
                "ALTER TABLE brain_sessions RENAME TO brain_sessions_v3;
                 DROP INDEX IF EXISTS brain_sessions_by_active;
                 CREATE TABLE brain_sessions (
                   agent_kind        TEXT NOT NULL,
                   agent_session_id  TEXT NOT NULL,
                   brain_instance_id TEXT NOT NULL,
                   locked_pid        INTEGER,
                   source            TEXT,
                   workspace_id      TEXT NOT NULL,
                   actor_id          TEXT NOT NULL,
                   channel           TEXT NOT NULL,
                   created_at        INTEGER NOT NULL,
                   last_active_at    INTEGER NOT NULL,
                   PRIMARY KEY
                     (agent_kind, agent_session_id, workspace_id, actor_id, channel)
                 );
                 INSERT INTO brain_sessions
                   (agent_kind, agent_session_id, brain_instance_id, locked_pid,
                    source, workspace_id, actor_id, channel, created_at, last_active_at)
                 SELECT agent_kind, agent_session_id, brain_instance_id, locked_pid,
                        source, workspace_id, actor_id, channel, created_at, last_active_at
                 FROM brain_sessions_v3;
                 DROP TABLE brain_sessions_v3;
                 CREATE INDEX brain_sessions_by_active
                   ON brain_sessions(agent_kind, workspace_id, actor_id, channel,
                                     locked_pid, last_active_at);
                 PRAGMA user_version = 4;",
            )?;
            transaction.commit()?;
        }
        if version < 5 {
            self.conn.execute_batch(
                "ALTER TABLE brain_sessions
                   ADD COLUMN completion_status TEXT NOT NULL DEFAULT 'active';
                 PRAGMA user_version = 5;",
            )?;
        }
        super::receiver::schema::up(&self.conn, version)?;
        Ok(())
    }

    pub(super) fn now(&self) -> i64 {
        (self.clock)()
    }

    // -- session locking -------------------------------------------------

    /// Release locks held by dead brain shells so their sessions become
    /// resumable again. Best-effort; called on startup before `pick_resume`.
    pub fn reap_dead_locks(&self) -> Result<()> {
        self.reap_dead_locks_after_sample(|| {})
    }

    #[cfg(test)]
    pub(in crate::state) fn reap_dead_locks_after_sample_for_test(
        &self,
        after_sample: impl FnOnce(),
    ) -> Result<()> {
        self.reap_dead_locks_after_sample(after_sample)
    }

    fn reap_dead_locks_after_sample(&self, after_sample: impl FnOnce()) -> Result<()> {
        let locked: Vec<(String, String, String, String, String, String, i64)> = {
            let mut stmt = self.conn.prepare(
                "SELECT agent_kind, agent_session_id, brain_instance_id,
                        workspace_id, actor_id, channel, locked_pid
                 FROM brain_sessions
                 WHERE locked_pid IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let dead = locked
            .into_iter()
            .filter(|(_, _, _, _, _, _, pid)| !(self.pid_alive)(i32::try_from(*pid).unwrap_or(0)))
            .collect::<Vec<_>>();
        if dead.is_empty() {
            return Ok(());
        }
        after_sample();
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        for (agent_kind, id, instance, workspace_id, actor_id, channel, pid) in dead {
            let unlocked = transaction.execute(
                "UPDATE brain_sessions SET locked_pid = NULL
                 WHERE agent_kind = ?1 AND agent_session_id = ?2
                   AND brain_instance_id = ?3 AND workspace_id = ?4
                   AND actor_id = ?5 AND channel = ?6 AND locked_pid = ?7",
                rusqlite::params![
                    agent_kind,
                    id,
                    instance,
                    workspace_id,
                    actor_id,
                    channel,
                    pid
                ],
            )?;
            if unlocked != 1 {
                continue;
            }
            let expected_acknowledgements: usize = transaction.query_row(
                "SELECT COUNT(*)
                 FROM receiver_answer_cleanups AS cleanup
                 WHERE cleanup.workspace_id = ?4
                   AND cleanup.brain_instance_id = ?3
                   AND cleanup.agent_kind = ?1 AND cleanup.actor_id = ?5
                   AND cleanup.channel = ?6 AND cleanup.actual_session_id = ?2
                   AND cleanup.controller_shutdown_acknowledged = 0
                   AND EXISTS (
                     SELECT 1 FROM receiver_session_registrations AS registration
                     WHERE registration.workspace_id = cleanup.workspace_id
                       AND registration.conversation_id = cleanup.conversation_id
                       AND registration.agent_kind = cleanup.agent_kind
                       AND registration.actor_id = cleanup.actor_id
                       AND registration.channel = cleanup.channel
                       AND registration.brain_instance_id = cleanup.brain_instance_id
                       AND registration.registered_session_id = cleanup.registered_session_id
                       AND registration.actual_session_id = cleanup.actual_session_id
                   )",
                rusqlite::params![agent_kind, id, instance, workspace_id, actor_id, channel],
                |row| row.get(0),
            )?;
            let acknowledged = transaction.execute(
                "UPDATE receiver_answer_cleanups AS cleanup
                 SET controller_shutdown_acknowledged = 1
                 WHERE cleanup.workspace_id = ?4
                   AND cleanup.brain_instance_id = ?3
                   AND cleanup.agent_kind = ?1 AND cleanup.actor_id = ?5
                   AND cleanup.channel = ?6 AND cleanup.actual_session_id = ?2
                   AND cleanup.controller_shutdown_acknowledged = 0
                   AND EXISTS (
                     SELECT 1 FROM receiver_session_registrations AS registration
                     WHERE registration.workspace_id = cleanup.workspace_id
                       AND registration.conversation_id = cleanup.conversation_id
                       AND registration.agent_kind = cleanup.agent_kind
                       AND registration.actor_id = cleanup.actor_id
                       AND registration.channel = cleanup.channel
                       AND registration.brain_instance_id = cleanup.brain_instance_id
                       AND registration.registered_session_id = cleanup.registered_session_id
                       AND registration.actual_session_id = cleanup.actual_session_id
                   )",
                rusqlite::params![agent_kind, id, instance, workspace_id, actor_id, channel],
            )?;
            anyhow::ensure!(
                acknowledged == expected_acknowledgements,
                "receiver controller cleanup ownership changed during dead-lock reap"
            );
        }
        transaction.commit()?;
        Ok(())
    }

    /// Free sessions restricted to one immutable workspace/actor/frontend/channel
    /// scope. Forked rows are excluded: a fork is a branch into some other
    /// agent's conversation (a background agent forking the panel's session),
    /// never this lineage's own, and older DBs still carry rows recorded before
    /// the session-start bridge learned to ignore them.
    #[must_use]
    pub fn sessions_by_recency(&self, scope: &SessionScope) -> Vec<String> {
        let Ok(mut statement) = self.conn.prepare(
            "SELECT agent_session_id FROM brain_sessions
             WHERE agent_kind = ?1 AND workspace_id = ?2 AND actor_id = ?3
               AND channel = ?4 AND locked_pid IS NULL
               AND (source IS NULL OR source <> 'fork')
             ORDER BY last_active_at DESC, rowid DESC",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = statement.query_map(
            rusqlite::params![
                scope.agent_kind().as_str(),
                scope.workspace_id().to_string(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
            |row| row.get::<_, String>(0),
        ) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }

    /// Return the exact frontend session currently locked by one live shell lineage.
    #[must_use]
    pub fn locked_session_for_instance(
        &self,
        instance: &str,
        scope: &SessionScope,
    ) -> Option<String> {
        self.conn
            .query_row(
                "SELECT agent_session_id FROM brain_sessions
                 WHERE brain_instance_id = ?1 AND locked_pid IS NOT NULL
                   AND agent_kind = ?2 AND workspace_id = ?3
                   AND actor_id = ?4 AND channel = ?5",
                rusqlite::params![
                    instance,
                    scope.agent_kind().as_str(),
                    scope.workspace_id().to_string(),
                    scope.actor().user_id().as_str(),
                    scope.actor().channel().as_str(),
                ],
                |row| row.get(0),
            )
            .ok()
    }

    /// Try to lock an existing free session to this shell. Returns `true`
    /// when the claim won; `false` if another shell grabbed it first (the
    /// caller should re-`pick_resume` and try again, or start fresh).
    pub fn claim(
        &self,
        agent_session_id: &str,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<bool> {
        let now = self.now();
        let n = self.conn.execute(
            "UPDATE brain_sessions
             SET locked_pid = ?2, brain_instance_id = ?3, last_active_at = ?4
             WHERE agent_session_id = ?1 AND locked_pid IS NULL
               AND agent_kind = ?5 AND workspace_id = ?6
               AND actor_id = ?7 AND channel = ?8",
            rusqlite::params![
                agent_session_id,
                pid,
                instance,
                now,
                scope.agent_kind().as_str(),
                scope.workspace_id().to_string(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
        )?;
        Ok(n == 1)
    }

    /// Register a fresh session with its complete immutable attribution.
    pub fn register_scoped_fresh(
        &self,
        agent_session_id: &str,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<()> {
        let now = self.now();
        self.conn.execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES (?1, ?2, ?3, ?4, 'fresh', ?5, ?6, ?7, ?8, ?8)",
            rusqlite::params![
                scope.agent_kind().as_str(),
                agent_session_id,
                instance,
                pid,
                scope.workspace_id().to_string(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
                now,
            ],
        )?;
        Ok(())
    }

    /// Release every lock held by `instance` and stamp `last_active`, so the
    /// session this shell was driving floats to the top of the resume queue
    /// next time. Called on clean exit.
    pub fn release(&self, instance: &str) -> Result<()> {
        let now = self.now();
        self.conn.execute(
            "UPDATE brain_sessions SET locked_pid = NULL, last_active_at = ?2
             WHERE brain_instance_id = ?1 AND locked_pid IS NOT NULL",
            rusqlite::params![instance, now],
        )?;
        Ok(())
    }

    // -- layout preference ----------------------------------------------

    #[must_use]
    pub fn get_panel_side(&self) -> PanelSide {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'panel_side'", [], |r| {
                r.get(0)
            })
            .ok();
        raw.map_or(PanelSide::DEFAULT, |s| PanelSide::parse(&s))
    }

    pub fn set_panel_side(&self, side: PanelSide) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('panel_side', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [side.as_str()],
        )?;
        Ok(())
    }

    // -- skills-render version stamp ------------------------------------

    /// The brain version that last rendered this workspace's skills into the
    /// registry, or `None` if a version-aware binary has never rendered them.
    /// Used by the startup auto-resync (see `crate::skills`).
    #[must_use]
    pub fn skills_synced_version(&self) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'skills_synced_version'",
                [],
                |r| r.get(0),
            )
            .ok()
    }

    /// Record the brain version that just rendered this workspace's skills.
    pub fn set_skills_synced_version(&self, version: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('skills_synced_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [version],
        )?;
        Ok(())
    }
}

#[cfg(test)]
use super::PidAlive;
use super::{
    Connection, Context, Db, PanelSide, Path, Result, SessionScope, system_clock, system_pid_alive,
};

#[cfg(test)]
mod configuration_tests {
    use super::Db;

    #[test]
    fn configure_explicitly_enables_foreign_keys() {
        let connection = rusqlite::Connection::open_in_memory().expect("in-memory database");
        connection
            .pragma_update(None, "foreign_keys", false)
            .expect("disable foreign keys for characterization");
        let before: i64 = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .expect("foreign key setting before configuration");

        Db::configure(&connection).expect("configure state database");

        let after: i64 = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .expect("foreign key setting after configuration");
        assert_eq!(before, 0);
        assert_eq!(after, 1);
    }

    #[test]
    fn receiver_open_bounds_lock_wait_before_schema_reconciliation() {
        let temporary = tempfile::tempdir().expect("temporary state database");
        let path = temporary.path().join("state.db");
        let db = Db::open_path_with_legacy_identity(&path, "workspace", "member")
            .expect("seed current state database");
        db.conn
            .pragma_update(None, "user_version", 5)
            .expect("stage receiver schema reconciliation");
        drop(db);
        let writer = rusqlite::Connection::open(&path).expect("open competing writer");
        writer
            .execute_batch("BEGIN IMMEDIATE")
            .expect("hold state write lock");

        let started = std::time::Instant::now();
        let result = Db::open_path_with_legacy_identity_and_busy_timeout(
            &path,
            "workspace",
            "member",
            std::time::Duration::from_millis(20),
        );

        assert!(result.is_err(), "locked reconciliation unexpectedly opened");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "receiver open exceeded its lock-wait budget"
        );
    }
}

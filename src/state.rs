//! Persistent state shared across `brain` shells and the Claude
//! SessionStart hook.
//!
//! Backed by SQLite (WAL mode) so multiple `brain` shells and the hook
//! script (a separate process) can read/write the same DB without
//! clobbering or busy-waiting. Mirrors the `tasks` sibling project's state
//! layer, scoped to what brain needs.
//!
//! Two tables:
//! - `brain_sessions` stores frontend sessions with immutable workspace,
//!   actor, and channel attribution. `locked_pid` is the PID of the live brain shell currently
//!   driving that session (NULL when free). The session-resume model is
//!   "lock + recency": on startup we resume the most-recently-active free
//!   session and lock it; on exit we release the lock; stale locks (dead
//!   PIDs) are reaped on the next startup. This keeps two terminals off the
//!   same conversation thread while still resuming your latest work.
//! - `meta` — small key/value store; today just the `panel_side` layout
//!   preference (which side the brain panel sits on).
//!
//! The SessionStart hook requires the selected workspace/actor variables
//! plus `BRAIN_INSTANCE_ID` and the selected UUID-scoped `BRAIN_STATE_DB`.
//! `BRAIN_PID` supplies optional lock ownership. Incomplete attribution means
//! ambient Claude usage, so the hook no-ops.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Immutable lookup scope for one actor's sessions in one workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionScope {
    agent_kind: crate::session::AgentKind,
    workspace_id: crate::workspace::WorkspaceId,
    actor: crate::actor::ActorContext,
}

impl SessionScope {
    #[must_use]
    pub const fn new(
        agent_kind: crate::session::AgentKind,
        workspace_id: crate::workspace::WorkspaceId,
        actor: crate::actor::ActorContext,
    ) -> Self {
        Self {
            agent_kind,
            workspace_id,
            actor,
        }
    }

    #[must_use]
    pub const fn actor(&self) -> &crate::actor::ActorContext {
        &self.actor
    }
}

/// Wall-clock provider (unix seconds). Production uses [`system_clock`];
/// tests inject a deterministic value.
pub type Clock = fn() -> i64;

/// Process-liveness probe. Production uses [`system_pid_alive`]; tests
/// inject a deterministic predicate.
pub type PidAlive = fn(i32) -> bool;

#[must_use]
pub fn system_clock() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// True if a process with `pid` currently exists. Implemented via `kill -0`,
/// which sends no signal — it only checks existence — so it stays
/// dependency-free and `unsafe`-free.
#[must_use]
pub fn system_pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Which side of the screen the brain (Claude) panel sits on. The fuzzy
/// search panel takes the other side. Persisted in `meta('panel_side')`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    /// Default: brain panel on the right, search on the left.
    pub const DEFAULT: Self = Self::Right;

    #[must_use]
    pub const fn flipped(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    #[must_use]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    #[must_use]
    fn parse(s: &str) -> Self {
        match s {
            "left" => Self::Left,
            _ => Self::Right,
        }
    }
}

pub struct Db {
    conn: Connection,
    clock: Clock,
    pid_alive: PidAlive,
}

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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening sqlite at {}", path.display()))?;
        Self::configure(&conn)?;
        let db = Self {
            conn,
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

    fn configure(conn: &Connection) -> Result<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
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
        Ok(())
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    // -- session locking -------------------------------------------------

    /// Release locks held by dead brain shells so their sessions become
    /// resumable again. Best-effort; called on startup before `pick_resume`.
    pub fn reap_dead_locks(&self) -> Result<()> {
        let locked: Vec<(String, i64)> = {
            let mut stmt = self.conn.prepare(
                "SELECT agent_session_id, locked_pid FROM brain_sessions
                 WHERE locked_pid IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (id, pid) in locked {
            if !(self.pid_alive)(i32::try_from(pid).unwrap_or(0)) {
                self.conn.execute(
                    "UPDATE brain_sessions SET locked_pid = NULL
                     WHERE agent_session_id = ?1",
                    [&id],
                )?;
            }
        }
        Ok(())
    }

    /// Free sessions restricted to one immutable workspace/actor/frontend/channel scope.
    #[must_use]
    pub fn sessions_by_recency(&self, scope: &SessionScope) -> Vec<String> {
        let Ok(mut statement) = self.conn.prepare(
            "SELECT agent_session_id FROM brain_sessions
             WHERE agent_kind = ?1 AND workspace_id = ?2 AND actor_id = ?3
               AND channel = ?4 AND locked_pid IS NULL
             ORDER BY last_active_at DESC, rowid DESC",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = statement.query_map(
            rusqlite::params![
                scope.agent_kind.as_str(),
                scope.workspace_id.to_string(),
                scope.actor.user_id().as_str(),
                scope.actor.channel().as_str(),
            ],
            |row| row.get::<_, String>(0),
        ) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }

    /// Try to lock an existing free session to this shell. Returns `true`
    /// when the claim won; `false` if another shell grabbed it first (the
    /// caller should re-`pick_resume` and try again, or start fresh).
    pub fn claim(&self, claude_session_id: &str, instance: &str, pid: i32) -> Result<bool> {
        let now = self.now();
        let n = self.conn.execute(
            "UPDATE brain_sessions
             SET locked_pid = ?2, brain_instance_id = ?3, last_active_at = ?4
             WHERE agent_session_id = ?1 AND locked_pid IS NULL",
            rusqlite::params![claude_session_id, pid, instance, now],
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
                scope.agent_kind.as_str(),
                agent_session_id,
                instance,
                pid,
                scope.workspace_id.to_string(),
                scope.actor.user_id().as_str(),
                scope.actor.channel().as_str(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn claim_wins_once_then_loses_on_a_held_session() {
        let db = Db::open_in_memory().unwrap();
        seed(&db, "s1", "i0", None, 100);
        assert!(db.claim("s1", "i1", 111).unwrap(), "first claim wins");
        assert!(
            !db.claim("s1", "i2", 222).unwrap(),
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
        // free and would start fresh — never sharing A's thread.
        let db = Db::open_in_memory().unwrap();
        seed(&db, "s1", "i0", None, 100);
        let a = db.sessions_by_recency(&scope()).into_iter().next().unwrap();
        assert!(db.claim(&a, "A", 10).unwrap());
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
}

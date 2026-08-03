//! Persistent state shared across `brain` shells and the Claude
//! SessionStart hook.
//!
//! Backed by SQLite (WAL mode) so multiple `brain` shells and the hook
//! script (a separate process) can read/write the same DB without
//! clobbering or busy-waiting. Mirrors the `tasks` sibling project's state
//! layer, scoped to what brain needs.
//!
//! Two tables:
//! - `brain_sessions` — one row per Claude session brain has launched or
//!   adopted. `locked_pid` is the PID of the live brain shell currently
//!   driving that session (NULL when free). The session-resume model is
//!   "lock + recency": on startup we resume the most-recently-active free
//!   session and lock it; on exit we release the lock; stale locks (dead
//!   PIDs) are reaped on the next startup. This keeps two terminals off the
//!   same conversation thread while still resuming your latest work.
//! - `meta` — small key/value store; today just the `panel_side` layout
//!   preference (which side the brain panel sits on).
//!
//! The SessionStart hook requires the four selected workspace/actor variables
//! plus `BRAIN_INSTANCE_ID` and the selected UUID-scoped `BRAIN_STATE_DB`.
//! `BRAIN_PID` supplies optional lock ownership. Incomplete attribution means
//! ambient Claude usage, so the hook no-ops.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

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

/// Conversation role persisted for a brain session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionChannel {
    Interactive,
    Sms,
    Email,
}

impl SessionChannel {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Sms => "sms",
            Self::Email => "email",
        }
    }
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
        Self::open_path(&workspace.paths().state_db())
    }

    /// Open or create a state DB at an explicit path.
    pub fn open_path(path: &Path) -> Result<Self> {
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
        db.migrate()?;
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
        db.migrate()?;
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

    fn migrate(&self) -> Result<()> {
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
                "SELECT claude_session_id, locked_pid FROM brain_sessions
                 WHERE locked_pid IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (id, pid) in locked {
            if !(self.pid_alive)(i32::try_from(pid).unwrap_or(0)) {
                self.conn.execute(
                    "UPDATE brain_sessions SET locked_pid = NULL
                     WHERE claude_session_id = ?1",
                    [&id],
                )?;
            }
        }
        Ok(())
    }

    /// Sessions no live brain holds, most-recently-active first. The caller
    /// walks this list and resumes the first whose transcript actually
    /// exists on disk (a session opened but never chatted in leaves a DB row
    /// with no `.jsonl`, which `claude --resume` can't find).
    #[must_use]
    pub fn free_sessions_by_recency(&self) -> Vec<String> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT claude_session_id FROM brain_sessions
             WHERE locked_pid IS NULL
             ORDER BY last_active_at DESC, rowid DESC",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
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
             WHERE claude_session_id = ?1 AND locked_pid IS NULL",
            rusqlite::params![claude_session_id, pid, instance, now],
        )?;
        Ok(n == 1)
    }

    /// Register a brand-new session id, locked to this shell.
    pub fn register_fresh(&self, claude_session_id: &str, instance: &str, pid: i32) -> Result<()> {
        let now = self.now();
        self.conn.execute(
            "INSERT INTO brain_sessions
               (claude_session_id, brain_instance_id, locked_pid, source,
                channel, created_at, last_active_at)
             VALUES (?1, ?2, ?3, 'fresh', ?4, ?5, ?5)",
            rusqlite::params![
                claude_session_id,
                instance,
                pid,
                SessionChannel::Interactive.as_str(),
                now
            ],
        )?;
        Ok(())
    }

    pub fn register_channel_fresh(
        &self,
        claude_session_id: &str,
        instance: &str,
        pid: i32,
        channel: SessionChannel,
    ) -> Result<()> {
        let now = self.now();
        self.conn.execute(
            "INSERT INTO brain_sessions
               (claude_session_id, brain_instance_id, locked_pid, source,
                channel, created_at, last_active_at)
             VALUES (?1, ?2, ?3, 'fresh', ?4, ?5, ?5)",
            rusqlite::params![claude_session_id, instance, pid, channel.as_str(), now],
        )?;
        Ok(())
    }

    #[must_use]
    pub fn session_for_channel(&self, channel: SessionChannel) -> Option<String> {
        self.conn
            .query_row(
                "SELECT claude_session_id FROM brain_sessions
                 WHERE channel = ?1 ORDER BY last_active_at DESC LIMIT 1",
                [channel.as_str()],
                |row| row.get(0),
            )
            .ok()
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
mod tests {
    use super::*;

    /// Insert a session row directly with an explicit `last_active`,
    /// bypassing the clock, for ordering tests.
    fn seed(db: &Db, id: &str, instance: &str, locked: Option<i32>, last_active: i64) {
        db.conn
            .execute(
                "INSERT INTO brain_sessions
                   (claude_session_id, brain_instance_id, locked_pid, source,
                    created_at, last_active_at)
                 VALUES (?1, ?2, ?3, 'seed', ?4, ?4)",
                rusqlite::params![id, instance, locked, last_active],
            )
            .unwrap();
    }

    #[test]
    fn free_sessions_is_empty_on_an_empty_db() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.free_sessions_by_recency().is_empty());
    }

    #[test]
    fn free_sessions_are_ordered_newest_first_and_skip_locked() {
        let db = Db::open_in_memory().unwrap();
        seed(&db, "old", "i1", None, 100);
        seed(&db, "new", "i1", None, 200);
        seed(&db, "locked-newer", "i2", Some(4242), 300);
        // The locked one is newer but held by a live shell, so it's excluded;
        // the rest come newest-first.
        assert_eq!(db.free_sessions_by_recency(), vec!["new", "old"]);
    }

    #[test]
    fn register_fresh_then_release_makes_it_resumable() {
        let db = Db::open_in_memory().unwrap();
        db.register_fresh("s1", "i1", 999).unwrap();
        // While locked, nothing is free to resume.
        assert!(db.free_sessions_by_recency().is_empty());
        db.release("i1").unwrap();
        assert_eq!(db.free_sessions_by_recency(), vec!["s1"]);
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
        assert_eq!(db.free_sessions_by_recency(), vec!["dead"]);
    }

    #[test]
    fn two_shells_take_distinct_sessions() {
        // Shell A claims the only free session; shell B must find nothing
        // free and would start fresh — never sharing A's thread.
        let db = Db::open_in_memory().unwrap();
        seed(&db, "s1", "i0", None, 100);
        let a = db.free_sessions_by_recency().into_iter().next().unwrap();
        assert!(db.claim(&a, "A", 10).unwrap());
        assert!(
            db.free_sessions_by_recency().is_empty(),
            "B sees nothing free"
        );
    }

    #[test]
    fn channel_sessions_are_kept_separate_from_interactive_sessions() {
        let db = Db::open_in_memory().unwrap();
        db.register_channel_fresh("sms-1", "i1", 10, SessionChannel::Sms)
            .unwrap();
        db.register_channel_fresh("email-1", "i1", 10, SessionChannel::Email)
            .unwrap();
        assert_eq!(
            db.session_for_channel(SessionChannel::Sms).as_deref(),
            Some("sms-1")
        );
        assert_eq!(
            db.session_for_channel(SessionChannel::Email).as_deref(),
            Some("email-1")
        );
        assert_eq!(db.session_for_channel(SessionChannel::Interactive), None);
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
}

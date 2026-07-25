//! The sync journal: a small SQLite DB at `~/.cache/brain/sync/journal.db`.
//!
//! Machine-local cache, never synced. Records each run; mirrors the WAL setup
//! of `crate::state`. The CSV-merge baselines (C3) will live beside it.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::Connection;

/// One recorded sync run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRun {
    pub started_at: String,
    pub finished_at: String,
    pub direction: String,
    pub outcome: String,
    pub transferred: u64,
    pub deleted: u64,
    pub conflicts: u64,
    pub errors: u64,
    pub note: String,
}

pub struct Journal {
    conn: Connection,
}

impl Journal {
    /// `~/.cache/brain/sync/journal.db`.
    #[must_use]
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("HOME").map_or_else(
            || PathBuf::from("."),
            |h| PathBuf::from(h).join(".cache").join("brain").join("sync"),
        );
        base.join("journal.db")
    }

    /// Open (creating parent dirs + schema). WAL like the state DB.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::configure(&conn)?;
        Self::from_conn(conn)
    }

    fn configure(conn: &Connection) -> Result<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(())
    }

    fn from_conn(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sync_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL,
                finished_at TEXT NOT NULL,
                direction TEXT NOT NULL,
                outcome TEXT NOT NULL,
                transferred INTEGER NOT NULL,
                deleted INTEGER NOT NULL,
                conflicts INTEGER NOT NULL,
                errors INTEGER NOT NULL,
                note TEXT NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    pub fn record(&self, r: &SyncRun) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_runs
               (started_at, finished_at, direction, outcome, transferred, deleted, conflicts, errors, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                r.started_at, r.finished_at, r.direction, r.outcome,
                r.transferred, r.deleted, r.conflicts, r.errors, r.note
            ],
        )?;
        Ok(())
    }

    /// Most-recent runs, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<SyncRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT started_at, finished_at, direction, outcome, transferred, deleted, conflicts, errors, note
             FROM sync_runs ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(SyncRun {
                started_at: row.get(0)?,
                finished_at: row.get(1)?,
                direction: row.get(2)?,
                outcome: row.get(3)?,
                transferred: row.get(4)?,
                deleted: row.get(5)?,
                conflicts: row.get(6)?,
                errors: row.get(7)?,
                note: row.get(8)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &str) -> SyncRun {
        SyncRun {
            started_at: "2026-07-25T00:00:00Z".into(),
            finished_at: "2026-07-25T00:00:05Z".into(),
            direction: dir.into(),
            outcome: "clean".into(),
            transferred: 3,
            deleted: 1,
            conflicts: 0,
            errors: 0,
            note: String::new(),
        }
    }

    fn mem() -> Journal {
        Journal::from_conn(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn records_and_reads_back_newest_first() {
        let j = mem();
        j.record(&run("push")).unwrap();
        j.record(&run("pull")).unwrap();
        let got = j.recent(10).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].direction, "pull");
        assert_eq!(got[1].direction, "push");
    }

    #[test]
    fn default_path_is_under_cache_brain_sync() {
        assert!(Journal::default_path().ends_with(".cache/brain/sync/journal.db"));
    }
}

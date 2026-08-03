//! The sync journal: a small SQLite DB in one workspace's UUID-scoped cache.
//!
//! Machine-local cache, never synced. Records each run; mirrors the WAL setup
//! of `crate::state`. The CSV-merge baselines (C3) will live beside it.

use std::path::Path;

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

    /// The highest row id recorded, or `None` when the journal is empty.
    ///
    /// A monotonic "have any new runs completed since I last looked?" cursor:
    /// each `record` inserts one autoincrement row, so a later `latest_id`
    /// strictly greater than an earlier one means at least one sync finished in
    /// between. Used by the startup triage gate to know when a background sync
    /// has landed.
    pub fn latest_id(&self) -> Result<Option<i64>> {
        let id: Option<i64> = self
            .conn
            .query_row("SELECT MAX(id) FROM sync_runs", [], |row| row.get(0))?;
        Ok(id)
    }

    /// Completion time of the newest successful run that could have brought
    /// remote changes downstream. Push-only and aborted runs do not refresh
    /// the receiver's remote-freshness clock.
    pub fn latest_downstream_completion(&self) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT finished_at
             FROM sync_runs
             WHERE direction IN ('both', 'pull', 'resync')
               AND outcome != 'aborted'
             ORDER BY id DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
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
    fn latest_id_is_none_when_empty_then_grows_with_each_record() {
        let j = mem();
        assert_eq!(j.latest_id().unwrap(), None, "no runs yet");
        j.record(&run("both")).unwrap();
        let first = j.latest_id().unwrap().expect("one run recorded");
        j.record(&run("pull")).unwrap();
        let second = j.latest_id().unwrap().expect("two runs recorded");
        assert!(second > first, "a completed run advances the cursor");
    }

    #[test]
    fn latest_downstream_completion_ignores_push_only_and_aborted_runs() {
        let j = mem();
        let mut pulled = run("pull");
        pulled.finished_at = "2026-07-30T10:00:00Z".into();
        j.record(&pulled).unwrap();

        let mut pushed = run("push");
        pushed.finished_at = "2026-07-30T11:00:00Z".into();
        j.record(&pushed).unwrap();

        let mut aborted = run("both");
        aborted.finished_at = "2026-07-30T12:00:00Z".into();
        aborted.outcome = "aborted".into();
        j.record(&aborted).unwrap();

        assert_eq!(
            j.latest_downstream_completion().unwrap().as_deref(),
            Some("2026-07-30T10:00:00Z")
        );
    }
}

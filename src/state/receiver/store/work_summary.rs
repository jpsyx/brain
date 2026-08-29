use std::path::Path;

use anyhow::{Context as _, Result};
use rusqlite::{Connection, OpenFlags};

use crate::state::{
    Db, MAX_RECEIVER_RECOVERY_ATTEMPTS, ReceiverDeliveryCounts, ReceiverWorkPhase,
    ReceiverWorkSummary,
};
use crate::workspace::WorkspaceId;

type SummaryRow = (
    i64,
    Option<String>,
    Option<i64>,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
);

impl Db {
    /// Return one content-free durable receiver-work snapshot.
    pub fn receiver_work_summary(&self) -> Result<ReceiverWorkSummary> {
        receiver_work_summary(&self.conn, &self.workspace_id)?.context("receiver state unavailable")
    }

    /// Read one durable work snapshot without creating or migrating state.
    pub(crate) fn receiver_work_summary_read_only(
        path: &Path,
        workspace_id: WorkspaceId,
    ) -> Result<Option<ReceiverWorkSummary>> {
        if !path.is_file() {
            return Ok(None);
        }
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        receiver_work_summary(&connection, &workspace_id.to_string())
    }

    pub(super) fn log_receiver_summary(
        &self,
        event: impl FnOnce(ReceiverWorkSummary) -> crate::logging::ReceiverLifecycleEvent,
    ) {
        if let Ok(summary) = self.receiver_work_summary() {
            crate::logging::log_receiver_lifecycle(event(summary));
        }
    }
}

fn receiver_work_summary(
    connection: &Connection,
    workspace_id: &str,
) -> Result<Option<ReceiverWorkSummary>> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Deferred)?;
    let required_tables: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN ('receiver_jobs', 'receiver_deliveries')",
        [],
        |row| row.get(0),
    )?;
    if required_tables != 2 {
        transaction.commit()?;
        return Ok(None);
    }
    let row: SummaryRow = transaction.query_row(SUMMARY_SQL, [workspace_id], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
            row.get(13)?,
            row.get(14)?,
            row.get(15)?,
            row.get(16)?,
        ))
    })?;
    transaction.commit()?;
    decode_summary(row).map(Some)
}

fn decode_summary(row: SummaryRow) -> Result<ReceiverWorkSummary> {
    let (
        queue_depth,
        oldest_phase,
        recovery_attempt,
        cleanup_gated,
        ready,
        delivering,
        retrying,
        ambiguous,
        failed,
        done,
        retry_exhausted,
        permanent_rejection,
        ambiguous_acknowledgement,
        idempotency_window_expired,
        no_safe_fallback,
        malformed_job_states,
        malformed_delivery_states,
    ) = row;
    anyhow::ensure!(malformed_job_states == 0, "receiver job state is malformed");
    anyhow::ensure!(
        malformed_delivery_states == 0,
        "receiver delivery state is malformed"
    );
    let oldest_active_phase = oldest_phase
        .as_deref()
        .map(|phase| {
            ReceiverWorkPhase::parse(phase)
                .ok_or_else(|| anyhow::anyhow!("receiver job state is malformed"))
        })
        .transpose()?;
    let recovery_attempt = recovery_attempt
        .map(|value| u32::try_from(value).context("receiver recovery attempt is malformed"))
        .transpose()?;
    Ok(ReceiverWorkSummary::new(
        count(queue_depth, "agent queue depth")?,
        oldest_active_phase,
        recovery_attempt,
        MAX_RECEIVER_RECOVERY_ATTEMPTS,
        count(cleanup_gated, "cleanup-gated response count")?,
        ReceiverDeliveryCounts::new(
            count(ready, "ready delivery count")?,
            count(delivering, "delivering count")?,
            count(retrying, "retrying count")?,
            count(ambiguous, "ambiguous count")?,
            count(failed, "failed count")?,
            count(done, "acknowledged count")?,
        )
        .with_terminal_reasons(
            count(retry_exhausted, "retry-exhausted count")?,
            count(permanent_rejection, "permanent-rejection count")?,
            count(ambiguous_acknowledgement, "ambiguous-acknowledgement count")?,
            count(
                idempotency_window_expired,
                "idempotency-window-expired count",
            )?,
            count(no_safe_fallback, "no-safe-fallback count")?,
        ),
    ))
}

fn count(value: i64, label: &str) -> Result<usize> {
    usize::try_from(value).with_context(|| format!("receiver {label} is malformed"))
}

const SUMMARY_SQL: &str = "WITH
  agent_work AS (
    SELECT state, recovery_count, received_at_unix_ms, job_id
    FROM receiver_jobs
    WHERE workspace_id = ?1 AND (
      state IN ('queued', 'claimed', 'launching', 'launched', 'accepted', 'processing')
      OR (state = 'retrying' AND retry_from_state IN (
        'claimed', 'launching', 'accepted', 'processing'
      ))
    )
  ),
  oldest AS (
    SELECT state, recovery_count FROM agent_work
    ORDER BY received_at_unix_ms, job_id LIMIT 1
  ),
  delivery AS (
    SELECT
      COUNT(*) FILTER (WHERE state = 'cleanup-gated') AS cleanup_gated,
      COUNT(*) FILTER (WHERE state = 'ready') AS ready,
      COUNT(*) FILTER (WHERE state = 'delivering') AS delivering,
      COUNT(*) FILTER (WHERE state = 'retrying') AS retrying,
      COUNT(*) FILTER (WHERE state = 'ambiguous') AS ambiguous,
      COUNT(*) FILTER (WHERE state = 'failed') AS failed,
      COUNT(*) FILTER (WHERE state = 'acknowledged') AS acknowledged,
      COUNT(*) FILTER (
        WHERE state = 'failed' AND error_category = 'retry-exhausted'
      ) AS retry_exhausted,
      COUNT(*) FILTER (
        WHERE state = 'failed' AND error_category IN (
          'authorization', 'credentials', 'invalid-request', 'provider-rejected'
        )
      ) AS permanent_rejection,
      COUNT(*) FILTER (
        WHERE state = 'ambiguous' AND ambiguity_reason IN (
          'provider-acceptance-unknown', 'provider-acknowledgement-malformed',
          'result-commit-unknown'
        )
      ) AS ambiguous_acknowledgement,
      COUNT(*) FILTER (
        WHERE (state = 'ambiguous' AND ambiguity_reason = 'idempotency-window-expired')
           OR (state = 'failed' AND error_category = 'idempotency-window-expired')
      ) AS idempotency_window_expired,
      COUNT(*) FILTER (WHERE fallback_decision = 'no-safe-fallback') AS no_safe_fallback
    FROM receiver_deliveries
  )
SELECT
  (SELECT COUNT(*) FROM agent_work),
  (SELECT state FROM oldest),
  (SELECT recovery_count FROM oldest),
  delivery.cleanup_gated,
  delivery.ready,
  delivery.delivering,
  delivery.retrying,
  delivery.ambiguous,
  delivery.failed,
  delivery.acknowledged,
  delivery.retry_exhausted,
  delivery.permanent_rejection,
  delivery.ambiguous_acknowledgement,
  delivery.idempotency_window_expired,
  delivery.no_safe_fallback,
  (SELECT COUNT(*) FROM receiver_jobs WHERE workspace_id = ?1 AND state NOT IN (
    'queued', 'claimed', 'launching', 'launched', 'accepted', 'processing',
    'answer-ready', 'delivering', 'retrying', 'failed', 'done'
  )),
  (SELECT COUNT(*) FROM receiver_deliveries WHERE state NOT IN (
    'cleanup-gated', 'ready', 'delivering', 'retrying', 'acknowledged', 'failed', 'ambiguous'
  ))
FROM delivery";

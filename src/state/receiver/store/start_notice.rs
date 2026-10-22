//! Best-effort SMS progress receipts, separate from terminal answer delivery.
use anyhow::Result;
use rusqlite::OptionalExtension as _;

use crate::state::{
    Db, ReceiverDeliveryAttemptId, ReceiverDeliveryClaim, ReceiverDeliveryId, ReceiverJobId,
    ReceiverJobToken, ReceiverResponseKind, render_receiver_delivery,
};

impl Db {
    pub(crate) fn receiver_waiting_for_agent_read_only(
        path: &std::path::Path,
        workspace: crate::workspace::WorkspaceId,
        job: uuid::Uuid,
    ) -> Result<bool> {
        let connection = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        connection.busy_timeout(std::time::Duration::ZERO)?;
        Ok(connection.query_row(
            "SELECT EXISTS (
                SELECT 1 FROM receiver_jobs AS earlier JOIN receiver_jobs AS current
                  ON current.workspace_id = ?1 AND current.job_id = ?2
                WHERE earlier.workspace_id = current.workspace_id
                  AND (earlier.received_at_unix_ms < current.received_at_unix_ms
                    OR (earlier.received_at_unix_ms = current.received_at_unix_ms
                        AND earlier.job_id < current.job_id))
                  AND earlier.state IN ('queued', 'claimed', 'launching', 'launched', 'accepted', 'processing', 'retrying')
                  AND (earlier.state != 'retrying' OR earlier.retry_from_state != 'delivering'))",
            rusqlite::params![workspace.to_string(), job.to_string()], |row| row.get(0),
        )?)
    }

    pub(crate) fn pending_receiver_start_notice(&self) -> Result<Option<ReceiverDeliveryClaim>> {
        let row = self
            .conn
            .query_row(
                "SELECT job.job_id, job.job_token, job.inbound_json, job.response_sender
             FROM receiver_jobs AS job JOIN meta ON meta.key = 'receiver-start:' || job.job_id
             AND meta.value = job.job_token
             WHERE job.workspace_id = ?1 AND job.channel = 'sms'
               AND (job.accepted_at_unix_ms IS NOT NULL OR job.completed_at_unix_ms IS NOT NULL)
               AND job.state NOT IN ('failed', 'done')
             ORDER BY job.received_at_unix_ms, job.job_id LIMIT 1",
                [&self.workspace_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, token, json, sender)) = row else {
            return Ok(None);
        };
        let inbound = super::decode_inbound(&json, sender)?;
        let envelope = render_receiver_delivery(
            &inbound,
            ReceiverResponseKind::ControlAcknowledgement,
            &inbound.response_sender,
            "Started working on your request.",
        )?;
        Ok(Some(ReceiverDeliveryClaim::new(
            ReceiverDeliveryId::new(),
            ReceiverDeliveryAttemptId::new(),
            ReceiverJobId::parse(&id)?,
            ReceiverJobToken::parse(&token)?,
            "start-notice".to_owned(),
            u64::MAX,
            1,
            None,
            envelope,
        )))
    }

    pub(crate) fn consume_receiver_start_notice(
        &self,
        notice: &ReceiverDeliveryClaim,
    ) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM meta WHERE key = ?1 AND value = ?2 AND EXISTS (
                SELECT 1 FROM receiver_jobs WHERE workspace_id = ?3 AND job_id = ?4
                AND job_token = ?2
                AND (accepted_at_unix_ms IS NOT NULL OR completed_at_unix_ms IS NOT NULL)
                AND state NOT IN ('failed', 'done'))",
            rusqlite::params![
                format!("receiver-start:{}", notice.job_id()),
                notice.token().to_string(),
                self.workspace_id,
                notice.job_id().to_string()
            ],
        )? == 1)
    }
}

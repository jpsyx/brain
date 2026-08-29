use anyhow::Result;
use rusqlite::Connection;

pub(super) fn migrate_pending(connection: &Connection) -> Result<()> {
    let pending = {
        let mut statement = connection.prepare(
            "SELECT job_id, job_token, inbound_json, response_sender, updated_at_unix_ms
             FROM receiver_jobs
             WHERE state = 'failed' AND pending_unavailable_notice = 1
               AND recovery_cleanup_instance IS NULL
               AND recovery_cleanup_session_id IS NULL
             ORDER BY received_at_unix_ms, job_id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (job_id, token, inbound_json, response_sender, observed) in pending {
        let job_id = crate::state::ReceiverJobId::parse(&job_id)?;
        let token = crate::state::ReceiverJobToken::parse(&token)?;
        let inbound = super::super::super::store::decode_inbound(&inbound_json, response_sender)?;
        let notice = crate::server::reply::unanswered_notice(
            super::super::super::store::response_intent::channel_label(inbound.channel),
        );
        let inserted = super::super::super::store::response_intent::insert(
            connection,
            job_id,
            token,
            &inbound,
            crate::state::ReceiverResponseKind::UnavailableNotice,
            &notice.text,
            observed,
        );
        match inserted {
            Ok(_) => {
                connection.execute(
                    "UPDATE receiver_jobs
                     SET state = 'answer-ready', pending_unavailable_notice = 0,
                         claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                         retry_at_unix_ms = NULL, retry_from_state = NULL
                     WHERE job_id = ?1 AND job_token = ?2 AND state = 'failed'
                       AND pending_unavailable_notice = 1
                       AND EXISTS (SELECT 1 FROM receiver_deliveries
                         WHERE job_id = ?1 AND job_token = ?2
                           AND response_kind = 'unavailable-notice')",
                    rusqlite::params![job_id.to_string(), token.to_string()],
                )?;
            }
            Err(error)
                if error
                    .downcast_ref::<crate::state::ReceiverDeliveryRenderError>()
                    .is_some() =>
            {
                connection.execute(
                    "UPDATE receiver_jobs
                     SET pending_unavailable_notice = 0,
                         last_error = 'notice-no-authorized-destination'
                     WHERE job_id = ?1 AND job_token = ?2 AND state = 'failed'
                       AND pending_unavailable_notice = 1",
                    rusqlite::params![job_id.to_string(), token.to_string()],
                )?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

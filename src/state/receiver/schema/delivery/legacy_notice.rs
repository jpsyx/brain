use anyhow::Result;
use rusqlite::{Connection, OptionalExtension as _};

pub(in crate::state::receiver::schema) fn finish_v13_cutover(
    connection: &Connection,
) -> Result<()> {
    if !super::super::has_column(connection, "pending_unavailable_notice")? {
        drop_obsolete_columns(connection)?;
        return Ok(());
    }
    let pending = {
        let mut statement = connection.prepare(
            "SELECT job_id, job_token, inbound_json, response_sender, updated_at_unix_ms,
                    recovery_cleanup_instance, recovery_cleanup_session_id
             FROM receiver_jobs
             WHERE pending_unavailable_notice = 1
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
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (
        job_id,
        token,
        inbound_json,
        response_sender,
        observed,
        cleanup_instance,
        cleanup_session,
    ) in pending
    {
        let job_id = crate::state::ReceiverJobId::parse(&job_id)?;
        let token = crate::state::ReceiverJobToken::parse(&token)?;
        let inbound = super::super::super::store::decode_inbound(&inbound_json, response_sender)?;
        let notice = crate::server::reply::unanswered_notice(
            super::super::super::store::response_intent::channel_label(inbound.channel),
        );
        let cleanup_gated = cleanup_instance.is_some() && cleanup_session.is_some();
        let delivery_state = if cleanup_gated {
            crate::state::ReceiverDeliveryState::CleanupGated
        } else {
            crate::state::ReceiverDeliveryState::Ready
        };
        let inserted = super::super::super::store::response_intent::insert_with_state(
            connection,
            job_id,
            token,
            &inbound,
            crate::state::ReceiverResponseKind::UnavailableNotice,
            &notice.text,
            delivery_state,
            observed,
        );
        match inserted {
            Ok(inserted) => {
                if !inserted {
                    ensure_expected_delivery(connection, &job_id, &token, delivery_state)?;
                }
                let changed = connection.execute(
                    "UPDATE receiver_jobs
                     SET state = CASE WHEN ?3 THEN 'failed' ELSE 'answer-ready' END,
                         pending_unavailable_notice = 0,
                         claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                         retry_at_unix_ms = NULL, retry_from_state = NULL
                     WHERE job_id = ?1 AND job_token = ?2
                       AND pending_unavailable_notice = 1
                       AND EXISTS (SELECT 1 FROM receiver_deliveries
                         WHERE job_id = ?1 AND job_token = ?2
                           AND response_kind = 'unavailable-notice'
                           AND state = ?4)",
                    rusqlite::params![
                        job_id.to_string(),
                        token.to_string(),
                        cleanup_gated,
                        delivery_state.as_str(),
                    ],
                )?;
                anyhow::ensure!(
                    changed == 1,
                    "receiver v13 unavailable-notice cutover lost source authority"
                );
            }
            Err(error)
                if error
                    .downcast_ref::<crate::state::ReceiverDeliveryRenderError>()
                    .is_some() =>
            {
                let changed = connection.execute(
                    "UPDATE receiver_jobs
                     SET state = 'failed', pending_unavailable_notice = 0,
                         claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                         retry_at_unix_ms = NULL, retry_from_state = NULL,
                         last_error = 'notice-no-authorized-destination'
                     WHERE job_id = ?1 AND job_token = ?2
                       AND pending_unavailable_notice = 1",
                    rusqlite::params![job_id.to_string(), token.to_string()],
                )?;
                anyhow::ensure!(
                    changed == 1,
                    "receiver v13 unavailable-notice terminalization lost source authority"
                );
            }
            Err(error) => return Err(error),
        }
    }
    drop_obsolete_columns(connection)?;
    Ok(())
}

fn ensure_expected_delivery(
    connection: &Connection,
    job_id: &crate::state::ReceiverJobId,
    token: &crate::state::ReceiverJobToken,
    expected: crate::state::ReceiverDeliveryState,
) -> Result<()> {
    let state = connection
        .query_row(
            "SELECT state FROM receiver_deliveries
             WHERE job_id = ?1 AND job_token = ?2
               AND response_kind = 'unavailable-notice'",
            rusqlite::params![job_id.to_string(), token.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    anyhow::ensure!(
        state.as_deref() == Some(expected.as_str()),
        "receiver v13 cutover found conflicting unavailable-notice delivery state"
    );
    Ok(())
}

fn drop_obsolete_columns(connection: &Connection) -> Result<()> {
    for column in [
        "pending_unavailable_notice",
        "unavailable_notice_owner",
        "unavailable_notice_expires_at_unix_ms",
    ] {
        if super::super::has_column(connection, column)? {
            connection
                .execute_batch(&format!("ALTER TABLE receiver_jobs DROP COLUMN {column};"))?;
        }
    }
    Ok(())
}

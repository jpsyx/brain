use anyhow::Result;

pub(super) fn terminalize_invalid_semantic_responses(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    observed_at_unix_ms: i64,
) -> Result<usize> {
    let invalid = {
        let mut statement = transaction.prepare(
            "SELECT delivery.delivery_id, delivery.job_id, delivery.job_token,
                    delivery.state, delivery.envelope_json, job.state
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
              AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
             WHERE (delivery.state = 'ready' AND job.state = 'answer-ready')
                OR (delivery.state = 'delivering' AND job.state = 'delivering')
                OR (delivery.state = 'retrying' AND job.state = 'retrying')
             ORDER BY delivery.created_at_unix_ms, delivery.delivery_id",
        )?;
        statement
            .query_map([workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(_, _, _, _, envelope, _)| {
                serde_json::from_str::<crate::state::ReceiverDeliveryEnvelope>(envelope).is_err()
            })
            .collect::<Vec<_>>()
    };
    for (delivery_id, job_id, token, delivery_state, _, job_state) in &invalid {
        let delivery_changed = transaction.execute(
            "UPDATE receiver_deliveries
             SET state = 'failed', retry_at_unix_ms = NULL,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 provider_io_started = 0, provider_reference = NULL,
                 error_category = 'invalid-request', ambiguity_reason = NULL,
                 fallback_decision = 'no-safe-fallback',
                 updated_at_unix_ms = ?5
             WHERE delivery_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = ?4",
            rusqlite::params![
                delivery_id,
                job_id,
                token,
                delivery_state,
                observed_at_unix_ms,
            ],
        )?;
        let job_changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 last_error = 'delivery-invalid-envelope', updated_at_unix_ms = ?5
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = ?4",
            rusqlite::params![workspace_id, job_id, token, job_state, observed_at_unix_ms,],
        )?;
        anyhow::ensure!(
            delivery_changed == 1 && job_changed == 1,
            "receiver invalid semantic response compare-and-swap lost authority"
        );
    }
    Ok(invalid.len())
}

pub(super) fn migrate_legacy_unavailable_notices(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    observed_at_unix_ms: i64,
) -> Result<usize> {
    let pending = {
        let mut statement = transaction.prepare(
            "SELECT job_id, job_token, inbound_json, response_sender
             FROM receiver_jobs
             WHERE workspace_id = ?1 AND state = 'failed'
               AND pending_unavailable_notice = 1
               AND recovery_cleanup_instance IS NULL
               AND recovery_cleanup_session_id IS NULL
             ORDER BY received_at_unix_ms, job_id",
        )?;
        statement
            .query_map([workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut migrated = 0usize;
    for (job_id, token, inbound_json, response_sender) in pending {
        let job_id = crate::state::ReceiverJobId::parse(&job_id)?;
        let token = crate::state::ReceiverJobToken::parse(&token)?;
        let inbound = super::super::super::decode_inbound(&inbound_json, response_sender)?;
        let message = crate::server::reply::unanswered_notice(
            super::super::super::response_intent::channel_label(inbound.channel),
        );
        let inserted = super::super::super::response_intent::insert(
            transaction,
            job_id,
            token,
            &inbound,
            crate::state::ReceiverResponseKind::UnavailableNotice,
            &message.text,
            observed_at_unix_ms,
        );
        match inserted {
            Ok(_) => {}
            Err(error)
                if error
                    .downcast_ref::<crate::state::ReceiverDeliveryRenderError>()
                    .is_some() =>
            {
                transaction.execute(
                    "UPDATE receiver_jobs
                     SET pending_unavailable_notice = 0,
                         last_error = 'notice-no-authorized-destination',
                         updated_at_unix_ms = ?4
                     WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
                       AND state = 'failed' AND pending_unavailable_notice = 1",
                    rusqlite::params![
                        workspace_id,
                        job_id.to_string(),
                        token.to_string(),
                        observed_at_unix_ms
                    ],
                )?;
                migrated = migrated.saturating_add(1);
                continue;
            }
            Err(error) => return Err(error),
        }
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'answer-ready', pending_unavailable_notice = 0,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = 'failed' AND pending_unavailable_notice = 1
               AND EXISTS (SELECT 1 FROM receiver_deliveries
                 WHERE job_id = ?2 AND job_token = ?3
                   AND response_kind = 'unavailable-notice' AND state = 'ready')",
            rusqlite::params![
                workspace_id,
                job_id.to_string(),
                token.to_string(),
                observed_at_unix_ms
            ],
        )?;
        migrated = migrated.saturating_add(changed);
    }
    Ok(migrated)
}

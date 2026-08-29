use anyhow::Result;

pub(super) fn terminalize_invalid_semantic_responses(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    observed_at_unix_ms: i64,
) -> Result<Vec<super::super::result::DeliveryLifecycle>> {
    let invalid = {
        let mut statement = transaction.prepare(
            "SELECT delivery.delivery_id, delivery.job_id, delivery.job_token,
                    delivery.state, delivery.envelope_json, job.state
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
              AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
             WHERE (delivery.state = 'cleanup-gated' AND job.state = 'failed')
                OR (delivery.state = 'ready' AND job.state = 'answer-ready')
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
    invalid
        .into_iter()
        .map(|_| {
            super::super::result::DeliveryLifecycle::new(
                "failed",
                "failed",
                crate::logging::ReceiverLifecycleReason::InvalidRequest,
            )
        })
        .collect()
}

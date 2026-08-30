use anyhow::Result;
use rusqlite::Connection;

pub(super) fn restore_acknowledged_jobs(connection: &Connection) -> Result<usize> {
    let has_fallback_decision: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_info('receiver_deliveries')
           WHERE name = 'fallback_decision'
         )",
        [],
        |row| row.get(0),
    )?;
    if !has_fallback_decision {
        return Ok(0);
    }
    let jobs = {
        let mut statement = connection.prepare(
            "SELECT fallback.job_id, fallback.job_token
             FROM receiver_deliveries AS fallback
             WHERE fallback.response_kind = 'fallback-notice'
               AND fallback.state = 'acknowledged'
               AND fallback.provider_reference IS NOT NULL
               AND length(trim(fallback.provider_reference)) > 0
               AND EXISTS (
                 SELECT 1 FROM receiver_deliveries AS source
                 WHERE source.job_id = fallback.job_id
                   AND source.job_token = fallback.job_token
                   AND source.response_kind != 'fallback-notice'
                   AND source.state IN ('failed', 'ambiguous')
                   AND source.fallback_decision = 'fallback-planned'
               )
               AND NOT EXISTS (
                 SELECT 1 FROM receiver_deliveries AS unfinished
                 WHERE unfinished.job_id = fallback.job_id
                   AND unfinished.job_token = fallback.job_token
                   AND NOT (
                     (unfinished.response_kind = 'fallback-notice'
                       AND unfinished.state = 'acknowledged'
                       AND unfinished.provider_reference IS NOT NULL
                       AND length(trim(unfinished.provider_reference)) > 0)
                     OR
                     (unfinished.response_kind != 'fallback-notice'
                       AND unfinished.state IN ('failed', 'ambiguous')
                       AND unfinished.fallback_decision = 'fallback-planned')
                   )
               )
             ORDER BY fallback.job_id",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut restored = 0usize;
    for (job_id, token) in jobs {
        restored = restored.saturating_add(connection.execute(
            "UPDATE receiver_jobs
             SET state = 'done', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL, last_error = NULL
             WHERE job_id = ?1 AND job_token = ?2",
            rusqlite::params![job_id, token],
        )?);
    }
    Ok(restored)
}

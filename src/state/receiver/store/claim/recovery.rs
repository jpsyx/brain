//! Atomic pre-acceptance recovery for an expired receiver launch.

use anyhow::Result;

use crate::state::MAX_RECEIVER_LAUNCH_ATTEMPTS;

pub(super) enum ExpiredLaunchingRecovery {
    Retrying,
    Exhausted,
    ChangedElsewhere,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn recover_expired_launching(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job_id: &str,
    conversation_id: &str,
    stale_owner: Option<&str>,
    retry_count: i64,
    next_owner: &str,
    now: i64,
    expires: i64,
) -> Result<ExpiredLaunchingRecovery> {
    let Some(stale_owner) = stale_owner else {
        return Ok(ExpiredLaunchingRecovery::ChangedElsewhere);
    };
    let next_count = retry_count
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("receiver launch retry count is exhausted"))?;
    let exhausted = next_count >= i64::from(MAX_RECEIVER_LAUNCH_ATTEMPTS);
    let (state, retry_at, retry_from, owner, expiry) = if exhausted {
        ("failed", None, None, None, None)
    } else {
        (
            "retrying",
            Some(now),
            Some("launching"),
            Some(next_owner),
            Some(expires),
        )
    };
    let changed = transaction.execute(
        "UPDATE receiver_jobs
         SET state = ?7, retry_count = ?8, retry_at_unix_ms = ?9,
             retry_from_state = ?10, last_error = 'launch-spawn',
             claim_owner = ?11, claim_expires_at_unix_ms = ?12,
             updated_at_unix_ms = ?6
         WHERE workspace_id = ?1 AND job_id = ?2 AND conversation_id = ?3
           AND state = 'launching' AND claim_owner = ?4
           AND claim_expires_at_unix_ms <= ?6 AND retry_count = ?5",
        rusqlite::params![
            workspace_id,
            job_id,
            conversation_id,
            stale_owner,
            retry_count,
            now,
            state,
            next_count,
            retry_at,
            retry_from,
            owner,
            expiry,
        ],
    )?;
    if changed != 1 {
        return Ok(ExpiredLaunchingRecovery::ChangedElsewhere);
    }
    cleanup_stale_registration(transaction, workspace_id, conversation_id, stale_owner)?;
    Ok(if exhausted {
        ExpiredLaunchingRecovery::Exhausted
    } else {
        ExpiredLaunchingRecovery::Retrying
    })
}

fn cleanup_stale_registration(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    conversation_id: &str,
    stale_owner: &str,
) -> Result<()> {
    transaction.execute(
        "UPDATE brain_sessions SET locked_pid = NULL
         WHERE workspace_id = ?1 AND brain_instance_id = ?3
           AND locked_pid IS NOT NULL
           AND EXISTS (
             SELECT 1 FROM receiver_session_registrations AS registration
             WHERE registration.workspace_id = ?1
               AND registration.conversation_id = ?2
               AND registration.brain_instance_id = ?3
               AND registration.agent_kind = brain_sessions.agent_kind
               AND registration.actor_id = brain_sessions.actor_id
               AND registration.channel = brain_sessions.channel
               AND (
                 registration.registered_session_id = brain_sessions.agent_session_id
                 OR registration.actual_session_id = brain_sessions.agent_session_id
               )
           )",
        rusqlite::params![workspace_id, conversation_id, stale_owner],
    )?;
    transaction.execute(
        "DELETE FROM receiver_session_registrations
         WHERE workspace_id = ?1 AND conversation_id = ?2
           AND brain_instance_id = ?3",
        rusqlite::params![workspace_id, conversation_id, stale_owner],
    )?;
    Ok(())
}

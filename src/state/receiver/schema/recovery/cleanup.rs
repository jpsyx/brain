use anyhow::Result;
use rusqlite::Connection;

pub(super) fn reconcile_partial_fences(connection: &Connection) -> Result<()> {
    let has_partial_fence = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM receiver_jobs
           WHERE (recovery_cleanup_instance IS NULL)
              != (recovery_cleanup_session_id IS NULL)
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !has_partial_fence {
        return Ok(());
    }
    connection.execute_batch(
        "UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL,
             claim_expires_at_unix_ms = NULL, retry_at_unix_ms = NULL,
             retry_from_state = NULL,
             last_error = 'recovery-native-session-unavailable',
             launch_expires_at_unix_ms = NULL,
             acceptance_expires_at_unix_ms = NULL,
             progress_expires_at_unix_ms = NULL,
             pending_unavailable_notice = 1,
             recovery_cleanup_instance = (
               SELECT registration.brain_instance_id
               FROM receiver_session_registrations AS registration
               JOIN brain_sessions AS session
                 ON session.workspace_id = registration.workspace_id
                AND session.brain_instance_id = registration.brain_instance_id
                AND session.agent_kind = registration.agent_kind
                AND session.actor_id = registration.actor_id
                AND session.channel = registration.channel
                AND session.agent_session_id = COALESCE(
                      registration.actual_session_id,
                      registration.registered_session_id
                    )
               JOIN receiver_conversations AS conversation
                 ON conversation.workspace_id = registration.workspace_id
                AND conversation.conversation_id = registration.conversation_id
                AND conversation.user_id = registration.actor_id
                AND conversation.channel = registration.channel
                AND conversation.agent_kind = registration.agent_kind
                AND conversation.agent_session_id = session.agent_session_id
               WHERE registration.workspace_id = receiver_jobs.workspace_id
                 AND registration.conversation_id = receiver_jobs.conversation_id
                 AND conversation.channel = receiver_jobs.channel
                 AND session.agent_session_id = receiver_jobs.recovery_cleanup_session_id
             )
         WHERE recovery_cleanup_instance IS NULL
           AND recovery_cleanup_session_id IS NOT NULL
           AND 1 = (
             SELECT COUNT(*)
             FROM receiver_session_registrations AS registration
             JOIN brain_sessions AS session
               ON session.workspace_id = registration.workspace_id
              AND session.brain_instance_id = registration.brain_instance_id
              AND session.agent_kind = registration.agent_kind
              AND session.actor_id = registration.actor_id
              AND session.channel = registration.channel
              AND session.agent_session_id = COALESCE(
                    registration.actual_session_id,
                    registration.registered_session_id
                  )
             JOIN receiver_conversations AS conversation
               ON conversation.workspace_id = registration.workspace_id
              AND conversation.conversation_id = registration.conversation_id
              AND conversation.user_id = registration.actor_id
              AND conversation.channel = registration.channel
              AND conversation.agent_kind = registration.agent_kind
              AND conversation.agent_session_id = session.agent_session_id
             WHERE registration.workspace_id = receiver_jobs.workspace_id
               AND registration.conversation_id = receiver_jobs.conversation_id
               AND conversation.channel = receiver_jobs.channel
               AND session.agent_session_id = receiver_jobs.recovery_cleanup_session_id
           )
           AND 1 = (
             SELECT COUNT(*)
             FROM receiver_session_registrations AS registration
             JOIN brain_sessions AS session
               ON session.workspace_id = registration.workspace_id
              AND session.brain_instance_id = registration.brain_instance_id
              AND session.agent_kind = registration.agent_kind
              AND session.actor_id = registration.actor_id
              AND session.channel = registration.channel
              AND session.agent_session_id = COALESCE(
                    registration.actual_session_id,
                    registration.registered_session_id
                  )
             WHERE registration.workspace_id = receiver_jobs.workspace_id
               AND registration.conversation_id = receiver_jobs.conversation_id
               AND session.agent_session_id = receiver_jobs.recovery_cleanup_session_id
           );
         UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL,
             claim_expires_at_unix_ms = NULL, retry_at_unix_ms = NULL,
             retry_from_state = NULL,
             last_error = 'recovery-native-session-unavailable',
             launch_expires_at_unix_ms = NULL,
             acceptance_expires_at_unix_ms = NULL,
             progress_expires_at_unix_ms = NULL,
             pending_unavailable_notice = 1,
             recovery_cleanup_session_id = (
               SELECT session.agent_session_id
               FROM receiver_session_registrations AS registration
               JOIN brain_sessions AS session
                 ON session.workspace_id = registration.workspace_id
                AND session.brain_instance_id = registration.brain_instance_id
                AND session.agent_kind = registration.agent_kind
                AND session.actor_id = registration.actor_id
                AND session.channel = registration.channel
                AND session.agent_session_id = COALESCE(
                      registration.actual_session_id,
                      registration.registered_session_id
                    )
               JOIN receiver_conversations AS conversation
                 ON conversation.workspace_id = registration.workspace_id
                AND conversation.conversation_id = registration.conversation_id
                AND conversation.user_id = registration.actor_id
                AND conversation.channel = registration.channel
                AND conversation.agent_kind = registration.agent_kind
                AND conversation.agent_session_id = session.agent_session_id
               WHERE registration.workspace_id = receiver_jobs.workspace_id
                 AND registration.conversation_id = receiver_jobs.conversation_id
                 AND conversation.channel = receiver_jobs.channel
                 AND registration.brain_instance_id = receiver_jobs.recovery_cleanup_instance
             )
         WHERE recovery_cleanup_instance IS NOT NULL
           AND recovery_cleanup_session_id IS NULL
           AND 1 = (
             SELECT COUNT(*)
             FROM receiver_session_registrations AS registration
             JOIN brain_sessions AS session
               ON session.workspace_id = registration.workspace_id
              AND session.brain_instance_id = registration.brain_instance_id
              AND session.agent_kind = registration.agent_kind
              AND session.actor_id = registration.actor_id
              AND session.channel = registration.channel
              AND session.agent_session_id = COALESCE(
                    registration.actual_session_id,
                    registration.registered_session_id
                  )
             JOIN receiver_conversations AS conversation
               ON conversation.workspace_id = registration.workspace_id
              AND conversation.conversation_id = registration.conversation_id
              AND conversation.user_id = registration.actor_id
              AND conversation.channel = registration.channel
              AND conversation.agent_kind = registration.agent_kind
              AND conversation.agent_session_id = session.agent_session_id
             WHERE registration.workspace_id = receiver_jobs.workspace_id
               AND registration.conversation_id = receiver_jobs.conversation_id
               AND conversation.channel = receiver_jobs.channel
               AND registration.brain_instance_id = receiver_jobs.recovery_cleanup_instance
           )
           AND 1 = (
             SELECT COUNT(*)
             FROM receiver_session_registrations AS registration
             JOIN brain_sessions AS session
               ON session.workspace_id = registration.workspace_id
              AND session.brain_instance_id = registration.brain_instance_id
              AND session.agent_kind = registration.agent_kind
              AND session.actor_id = registration.actor_id
              AND session.channel = registration.channel
              AND session.agent_session_id = COALESCE(
                    registration.actual_session_id,
                    registration.registered_session_id
                  )
             WHERE registration.workspace_id = receiver_jobs.workspace_id
               AND registration.conversation_id = receiver_jobs.conversation_id
               AND registration.brain_instance_id = receiver_jobs.recovery_cleanup_instance
           );
         UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL,
             claim_expires_at_unix_ms = NULL, retry_at_unix_ms = NULL,
             retry_from_state = NULL,
             last_error = 'recovery-native-session-unavailable',
             launch_expires_at_unix_ms = NULL,
             acceptance_expires_at_unix_ms = NULL,
             progress_expires_at_unix_ms = NULL,
             pending_unavailable_notice = 1,
             recovery_cleanup_instance = NULL,
             recovery_cleanup_session_id = NULL
         WHERE (recovery_cleanup_instance IS NULL)
             != (recovery_cleanup_session_id IS NULL);",
    )?;
    Ok(())
}

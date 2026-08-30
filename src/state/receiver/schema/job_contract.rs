use anyhow::Result;
use rusqlite::Connection;

const CREATE_V11_TABLE: &str = "CREATE TABLE IF NOT EXISTS receiver_jobs (
           job_id                    TEXT PRIMARY KEY,
           job_token                 TEXT NOT NULL UNIQUE,
           workspace_id              TEXT NOT NULL,
           conversation_id           TEXT NOT NULL REFERENCES receiver_conversations(conversation_id),
           channel                   TEXT NOT NULL CHECK (channel IN ('sms', 'email')),
           provider_id               TEXT,
           inbound_json              TEXT NOT NULL,
           state                     TEXT NOT NULL CHECK (state IN (
             'queued', 'claimed', 'launching', 'launched', 'accepted', 'processing',
             'answer-ready', 'delivering', 'retrying', 'failed', 'done'
           )),
           received_at_unix_ms       INTEGER NOT NULL,
           updated_at_unix_ms        INTEGER NOT NULL,
           claim_owner               TEXT,
           claim_expires_at_unix_ms  INTEGER,
           retry_count               INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
           retry_at_unix_ms           INTEGER,
           retry_from_state           TEXT CHECK (retry_from_state IN (
             'claimed', 'launching', 'accepted', 'processing', 'delivering'
           )),
           last_error                TEXT,
           launched_at_unix_ms       INTEGER,
           accepted_at_unix_ms       INTEGER,
           progressing_at_unix_ms    INTEGER,
           completed_at_unix_ms      INTEGER,
           observation_instance      TEXT,
           observation_session_id    TEXT,
           observation_revision      INTEGER NOT NULL DEFAULT 0 CHECK (observation_revision >= 0),
           attempt_accepted_at_unix_ms INTEGER,
           attempt_progressing_at_unix_ms INTEGER,
           latest_progress_at_unix_ms INTEGER,
           launch_expires_at_unix_ms INTEGER,
           acceptance_expires_at_unix_ms INTEGER,
           progress_expires_at_unix_ms INTEGER,
           recovery_expires_at_unix_ms INTEGER,
           absolute_work_expires_at_unix_ms INTEGER,
           recovery_count            INTEGER NOT NULL DEFAULT 0 CHECK (recovery_count >= 0),
           attempt_kind              TEXT NOT NULL DEFAULT 'ordinary'
             CHECK (attempt_kind IN ('ordinary', 'recovery')),
           pending_unavailable_notice INTEGER NOT NULL DEFAULT 0
             CHECK (pending_unavailable_notice IN (0, 1)),
           recovery_cleanup_instance  TEXT,
           recovery_cleanup_session_id TEXT,
           unavailable_notice_owner TEXT,
           unavailable_notice_expires_at_unix_ms INTEGER,
           UNIQUE (workspace_id, channel, provider_id),
           CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL)),
           CHECK ((recovery_cleanup_instance IS NULL) =
                  (recovery_cleanup_session_id IS NULL))
         );";

pub(super) fn create_v11_table_if_missing(connection: &Connection) -> Result<()> {
    connection.execute_batch(CREATE_V11_TABLE)?;
    Ok(())
}

pub(super) fn rebuild_exact_v11(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "DROP INDEX IF EXISTS receiver_jobs_ready;
         DROP INDEX IF EXISTS receiver_jobs_job_token;
         ALTER TABLE receiver_jobs RENAME TO receiver_jobs_v12_source;",
    )?;
    create_v11_table_if_missing(connection)?;
    connection.execute_batch(
        "INSERT INTO receiver_jobs
           (job_id, job_token, workspace_id, conversation_id, channel, provider_id,
            inbound_json, state, received_at_unix_ms, updated_at_unix_ms,
            claim_owner, claim_expires_at_unix_ms, retry_count, retry_at_unix_ms,
            retry_from_state, last_error, launched_at_unix_ms, accepted_at_unix_ms,
            progressing_at_unix_ms, completed_at_unix_ms, observation_instance,
            observation_session_id, observation_revision, attempt_accepted_at_unix_ms,
            attempt_progressing_at_unix_ms, latest_progress_at_unix_ms,
            launch_expires_at_unix_ms, acceptance_expires_at_unix_ms,
            progress_expires_at_unix_ms, recovery_expires_at_unix_ms,
            absolute_work_expires_at_unix_ms, recovery_count, attempt_kind,
            pending_unavailable_notice, recovery_cleanup_instance,
            recovery_cleanup_session_id, unavailable_notice_owner,
            unavailable_notice_expires_at_unix_ms)
         SELECT job_id, job_token, workspace_id, conversation_id, channel, provider_id,
            inbound_json, state, received_at_unix_ms, updated_at_unix_ms,
            claim_owner, claim_expires_at_unix_ms, retry_count, retry_at_unix_ms,
            retry_from_state, last_error, launched_at_unix_ms, accepted_at_unix_ms,
            progressing_at_unix_ms, completed_at_unix_ms, observation_instance,
            observation_session_id, observation_revision, attempt_accepted_at_unix_ms,
            attempt_progressing_at_unix_ms, latest_progress_at_unix_ms,
            launch_expires_at_unix_ms, acceptance_expires_at_unix_ms,
            progress_expires_at_unix_ms, recovery_expires_at_unix_ms,
            absolute_work_expires_at_unix_ms, recovery_count, attempt_kind,
            pending_unavailable_notice, recovery_cleanup_instance,
            recovery_cleanup_session_id, unavailable_notice_owner,
            unavailable_notice_expires_at_unix_ms
         FROM receiver_jobs_v12_source;
         DROP TABLE receiver_jobs_v12_source;
         CREATE UNIQUE INDEX receiver_jobs_job_token ON receiver_jobs(job_token);
         CREATE INDEX receiver_jobs_ready
           ON receiver_jobs(state, retry_at_unix_ms, received_at_unix_ms, job_id);",
    )?;
    Ok(())
}

pub(super) fn rebuild_exact_v12(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TEMP TABLE receiver_job_response_senders AS
           SELECT job_id, response_sender FROM receiver_jobs;",
    )?;
    rebuild_exact_v11(connection)?;
    connection.execute_batch(
        "ALTER TABLE receiver_jobs ADD COLUMN response_sender TEXT;
         UPDATE receiver_jobs
         SET response_sender = (
           SELECT staged.response_sender
           FROM receiver_job_response_senders AS staged
           WHERE staged.job_id = receiver_jobs.job_id
         );
         DROP TABLE receiver_job_response_senders;",
    )?;
    Ok(())
}

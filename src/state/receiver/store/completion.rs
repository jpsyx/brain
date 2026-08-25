use anyhow::Result;

use super::{to_i64, validated_owner};
use crate::state::{Db, ReceiverCompletionRequest};

impl Db {
    /// Commit the exact native binding and terminal job transition together.
    pub fn complete_receiver_job_with_binding(
        &self,
        request: &ReceiverCompletionRequest<'_>,
    ) -> Result<bool> {
        let owner = validated_owner(request.owner)?;
        anyhow::ensure!(
            request.registration.scope().workspace_id().to_string() == self.workspace_id,
            "receiver session scope belongs to another workspace"
        );
        let observed = to_i64(request.observed_at_unix_ms, "receiver completion time")?;
        let authorized = to_i64(
            request.authorized_at_unix_ms,
            "receiver completion authorization time",
        )?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'done', completed_at_unix_ms = ?4,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?5
               AND claim_expires_at_unix_ms > ?6
               AND observation_instance = ?7 AND state IN ('launched', 'accepted', 'processing')
               AND conversation_id = ?8",
            rusqlite::params![
                self.workspace_id,
                request.job_id.to_string(),
                request.token.to_string(),
                observed,
                owner,
                authorized,
                request.registration.instance(),
                request.registration.conversation_id().to_string(),
            ],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        if !super::session::replace_receiver_binding_in_transaction(
            &transaction,
            &self.workspace_id,
            request.registration,
            super::session::ReceiverBindingTarget::ExactCompleted(request.completed_session),
            request.observed_at_unix_ms,
        )? {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }
}

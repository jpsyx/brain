use anyhow::Result;

use super::{to_i64, validated_owner};
use crate::state::{Db, ReceiverJobId, ReceiverSessionAttribution};

impl Db {
    /// Commit the exact native binding and terminal job transition together.
    pub fn complete_receiver_job_with_binding(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        registration: &ReceiverSessionAttribution,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let owner = validated_owner(owner)?;
        anyhow::ensure!(
            registration.scope().workspace_id().to_string() == self.workspace_id,
            "receiver session scope belongs to another workspace"
        );
        let observed = to_i64(observed_at_unix_ms, "receiver completion time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if !super::session::replace_receiver_binding_in_transaction(
            &transaction,
            &self.workspace_id,
            registration,
            observed_at_unix_ms,
        )? {
            return Ok(false);
        }
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'done', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
               AND claim_expires_at_unix_ms > ?4 AND state = 'launching'
               AND conversation_id = ?5",
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                owner,
                observed,
                registration.conversation_id().to_string(),
            ],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }
}

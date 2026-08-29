use anyhow::Result;

use crate::state::{
    Db, ReceiverCompletionOutcome, ReceiverCompletionRequest, ReceiverObservationSet,
};

mod authorization;
mod duplicate;
mod lifecycle;
mod preparation;
mod transaction;

impl Db {
    /// Commit the exact native binding, transcript, answer outbox, and answer-ready transition.
    pub fn complete_receiver_job_with_binding(
        &self,
        request: &ReceiverCompletionRequest<'_>,
    ) -> Result<Option<ReceiverCompletionOutcome>> {
        self.complete_receiver_job_with_observation(request, None)
    }

    /// Commit optional lifecycle evidence and one exact durable answer atomically.
    pub fn complete_receiver_job_with_observation(
        &self,
        request: &ReceiverCompletionRequest<'_>,
        observation: Option<&ReceiverObservationSet>,
    ) -> Result<Option<ReceiverCompletionOutcome>> {
        let authorized = authorization::validate_request(&self.workspace_id, request)?;
        transaction::complete(self, request, observation, &authorized)
    }
}

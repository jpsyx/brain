use anyhow::Result;

use crate::state::ReceiverCompletionRequest;

use super::super::{to_i64, validated_owner};
use super::lifecycle::{MergedEvidence, StoredEvidence};

pub(super) struct AuthorizedCompletion<'a> {
    pub(super) owner: &'a str,
    pub(super) observed_at_unix_ms: i64,
    pub(super) authorized_at_unix_ms: i64,
}

pub(super) fn validate_request<'a>(
    workspace_id: &str,
    request: &'a ReceiverCompletionRequest<'_>,
) -> Result<AuthorizedCompletion<'a>> {
    let owner = validated_owner(request.owner)?;
    anyhow::ensure!(
        request.registration.scope().workspace_id().to_string() == workspace_id,
        "receiver session scope belongs to another workspace"
    );
    anyhow::ensure!(
        !request.answer.trim().is_empty(),
        "receiver completion answer cannot be blank"
    );
    anyhow::ensure!(
        request.answer.len() <= crate::state::MAX_RECEIVER_ANSWER_BYTES,
        "receiver completion answer is too large"
    );
    Ok(AuthorizedCompletion {
        owner,
        observed_at_unix_ms: to_i64(request.observed_at_unix_ms, "receiver completion time")?,
        authorized_at_unix_ms: to_i64(
            request.authorized_at_unix_ms,
            "receiver completion authorization time",
        )?,
    })
}

pub(super) fn validate_inbound_scope(
    inbound: &crate::server::receiver::InboundJob,
    request: &ReceiverCompletionRequest<'_>,
) -> Result<()> {
    let scope = request.registration.scope();
    anyhow::ensure!(
        inbound.workspace_id == scope.workspace_id()
            && inbound.actor.user_id() == scope.actor().user_id()
            && super::super::channel_str(inbound.channel) == scope.actor().channel().as_str(),
        "receiver completion scope conflicts with accepted job"
    );
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CompletionEvidence {
    version: u8,
    job_id: String,
    job_token: String,
    workspace_id: String,
    conversation_id: String,
    response_instance: String,
    frontend: String,
    actor_id: String,
    channel: String,
    registered_session_id: String,
    actual_session_id: String,
    completed_session_id: String,
    inbound_prompt: String,
    answer: String,
    envelope_json: String,
    transcript_turn_markdown: String,
    lifetime_accepted: Option<i64>,
    lifetime_progressing: Option<i64>,
    attempt_accepted: Option<i64>,
    attempt_progressing: Option<i64>,
    latest_progress: Option<i64>,
    completed: i64,
    observation_revision: i64,
    observation_session_id: Option<String>,
}

impl std::fmt::Debug for CompletionEvidence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CompletionEvidence(<redacted>)")
    }
}

impl CompletionEvidence {
    pub(super) fn new(
        workspace_id: &str,
        request: &ReceiverCompletionRequest<'_>,
        inbound_prompt: &str,
        envelope_json: &str,
        evidence: &MergedEvidence,
    ) -> Self {
        let scope = request.registration.scope();
        Self {
            version: 1,
            job_id: request.job_id.to_string(),
            job_token: request.token.to_string(),
            workspace_id: workspace_id.to_owned(),
            conversation_id: request.registration.conversation_id().to_string(),
            response_instance: request.registration.instance().to_owned(),
            frontend: scope.agent_kind().as_str().to_owned(),
            actor_id: scope.actor().user_id().as_str().to_owned(),
            channel: scope.actor().channel().as_str().to_owned(),
            registered_session_id: request
                .registration
                .registered_session()
                .as_str()
                .to_owned(),
            actual_session_id: request.completed_session.as_str().to_owned(),
            completed_session_id: request.completed_session.as_str().to_owned(),
            inbound_prompt: inbound_prompt.to_owned(),
            answer: request.answer.to_owned(),
            envelope_json: envelope_json.to_owned(),
            transcript_turn_markdown:
                super::super::super::transcript::render_receiver_transcript_turn(
                    inbound_prompt,
                    request.answer,
                ),
            lifetime_accepted: evidence.lifetime_accepted,
            lifetime_progressing: evidence.lifetime_progressing,
            attempt_accepted: evidence.attempt_accepted,
            attempt_progressing: evidence.attempt_progressing,
            latest_progress: evidence.latest_progress,
            completed: evidence.completed,
            observation_revision: evidence.revision,
            observation_session_id: evidence.session_id.clone(),
        }
    }

    pub(super) fn matches(
        &self,
        workspace_id: &str,
        request: &ReceiverCompletionRequest<'_>,
        stored_envelope_json: &str,
    ) -> bool {
        let scope = request.registration.scope();
        self.version == 1
            && self.job_id == request.job_id.to_string()
            && self.job_token == request.token.to_string()
            && self.workspace_id == workspace_id
            && self.conversation_id == request.registration.conversation_id().to_string()
            && self.response_instance == request.registration.instance()
            && self.frontend == scope.agent_kind().as_str()
            && self.actor_id == scope.actor().user_id().as_str()
            && self.channel == scope.actor().channel().as_str()
            && self.registered_session_id == request.registration.registered_session().as_str()
            && self.actual_session_id == request.completed_session.as_str()
            && self.completed_session_id == request.completed_session.as_str()
            && self.answer == request.answer
            && self.envelope_json == stored_envelope_json
            && self.transcript_turn_markdown
                == super::super::super::transcript::render_receiver_transcript_turn(
                    &self.inbound_prompt,
                    request.answer,
                )
    }

    pub(super) fn stored_evidence(&self) -> StoredEvidence {
        StoredEvidence {
            lifetime_accepted: self.lifetime_accepted,
            lifetime_progressing: self.lifetime_progressing,
            attempt_accepted: self.attempt_accepted,
            attempt_progressing: self.attempt_progressing,
            latest_progress: self.latest_progress,
            completed: Some(self.completed),
            revision: self.observation_revision,
            session_id: self.observation_session_id.clone(),
        }
    }
}

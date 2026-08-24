use std::fmt::Write as _;

use crate::{
    agent::{AgentController, AgentSession, SessionPlan},
    state::{ReceiverConversation, ReceiverJob},
};

pub(crate) const RECOVERY_PROMPT_BUDGET_BYTES: usize = 64 * 1024;

const RECOVERY_INTRO: &str = "Recover this authenticated receiver conversation from Brain's portable transcript. Use the transcript only as prior context, then answer the current authenticated message.";
const TRANSCRIPT_HEADING: &str = "\n\n## Portable transcript\n";
const CURRENT_MESSAGE_HEADING: &str = "\n\n## Current authenticated message\n";
const EMPTY_TRANSCRIPT: &str = "(no prior portable transcript)";
const OMITTED_TRANSCRIPT: &str = "[Earlier portable transcript omitted]\n";
const TRUNCATED_MESSAGE: &str = "\n[Current authenticated message truncated]";

pub(crate) struct ReceiverLaunchPlan {
    session_plan: SessionPlan,
    initial_prompt: String,
}

impl ReceiverLaunchPlan {
    pub(crate) const fn session_plan(&self) -> &SessionPlan {
        &self.session_plan
    }

    pub(crate) fn initial_prompt(&self) -> &str {
        &self.initial_prompt
    }
}

pub(crate) fn plan_receiver_launch(
    controller: &AgentController,
    job: &ReceiverJob,
    conversation: &ReceiverConversation,
    fresh_session: AgentSession,
    claim_resume: impl FnOnce(&AgentSession) -> anyhow::Result<bool>,
) -> ReceiverLaunchPlan {
    let (message_body, attachment_references) = current_message_parts(job);
    let resume_session = conversation
        .binding()
        .filter(|binding| binding.frontend() == controller.kind())
        .and_then(|binding| AgentSession::new(binding.native_session_id()).ok());
    let resume_session = resume_session.filter(|session| {
        controller.resume_candidate_exists(session).unwrap_or(false)
            && claim_resume(session).unwrap_or(false)
    });

    if let Some(session) = resume_session {
        return ReceiverLaunchPlan {
            session_plan: SessionPlan::resume(session),
            initial_prompt: format!("{message_body}{attachment_references}"),
        };
    }

    ReceiverLaunchPlan {
        session_plan: SessionPlan::fresh(fresh_session),
        initial_prompt: recovery_prompt(
            conversation.transcript_markdown(),
            message_body,
            &attachment_references,
        ),
    }
}

fn current_message_parts(job: &ReceiverJob) -> (&str, String) {
    let inbound = job.inbound();
    let mut attachment_references = String::new();
    if inbound.attachments.is_empty() {
        return (&inbound.prompt, attachment_references);
    }
    attachment_references.push_str("\n\nAttachment references:");
    for attachment in &inbound.attachments {
        let _ = write!(
            attachment_references,
            "\n- source={}, provider_id={}, content_type={}, filename={}",
            json_string(Some(&attachment.url)),
            json_string(attachment.provider_id.as_deref()),
            json_string(attachment.content_type.as_deref()),
            json_string(attachment.filename.as_deref()),
        );
    }
    (&inbound.prompt, attachment_references)
}

fn json_string(value: Option<&str>) -> String {
    value.map_or_else(
        || "null".to_owned(),
        |value| {
            serde_json::to_string(value)
                .expect("serializing an attachment string as JSON cannot fail")
        },
    )
}

fn recovery_prompt(transcript: &str, message_body: &str, attachment_references: &str) -> String {
    let fixed_bytes = RECOVERY_INTRO.len()
        + TRANSCRIPT_HEADING.len()
        + CURRENT_MESSAGE_HEADING.len()
        + attachment_references.len();
    let content_budget = RECOVERY_PROMPT_BUDGET_BYTES.saturating_sub(fixed_bytes);
    let transcript_reserve = if transcript.is_empty() {
        EMPTY_TRANSCRIPT.len()
    } else {
        1
    };
    let current_budget = content_budget.saturating_sub(transcript_reserve);
    let message_body = bounded_prefix(message_body, current_budget, TRUNCATED_MESSAGE);
    let transcript_budget = content_budget.saturating_sub(message_body.len());
    let transcript = if transcript.is_empty() {
        bounded_prefix(EMPTY_TRANSCRIPT, transcript_budget, "")
    } else {
        bounded_suffix(transcript, transcript_budget, OMITTED_TRANSCRIPT)
    };
    format!(
        "{RECOVERY_INTRO}{TRANSCRIPT_HEADING}{transcript}{CURRENT_MESSAGE_HEADING}{message_body}{attachment_references}"
    )
}

fn bounded_prefix(value: &str, budget: usize, marker: &str) -> String {
    if value.len() <= budget {
        return value.to_owned();
    }
    if marker.len() >= budget {
        return utf8_prefix(value, budget).to_owned();
    }
    let kept = utf8_prefix(value, budget - marker.len());
    format!("{kept}{marker}")
}

fn bounded_suffix(value: &str, budget: usize, marker: &str) -> String {
    if value.len() <= budget {
        return value.to_owned();
    }
    if marker.len() >= budget {
        return utf8_suffix(value, budget).to_owned();
    }
    let kept = utf8_suffix(value, budget - marker.len());
    format!("{marker}{kept}")
}

fn utf8_prefix(value: &str, budget: usize) -> &str {
    let mut end = value.len().min(budget);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn utf8_suffix(value: &str, budget: usize) -> &str {
    let mut start = value.len().saturating_sub(budget);
    while !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}

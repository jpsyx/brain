use std::fmt::Write as _;

use crate::{
    agent::{AgentController, AgentSession, SessionPlan},
    state::{ReceiverConversation, ReceiverJob},
};

pub(crate) const RECOVERY_PROMPT_BUDGET_BYTES: usize = 64 * 1024;

const TRANSCRIPT_RESERVED_BYTES: usize = 8 * 1024;
const CURRENT_MESSAGE_RESERVED_BYTES: usize = 16 * 1024;
const TASK_CAPTURE_POLICY: &str = "If the message asks to add, create, capture, remember, or track a task, create it in Brain's task system; do not perform the task now unless the sender explicitly asks you to.";
const RECOVERY_INTRO: &str = "Recover this authenticated receiver conversation from Brain's portable transcript. Use the transcript only as prior context, then answer the current authenticated message.";
const TRANSCRIPT_HEADING: &str = "\n\n## Portable transcript\n";
const CURRENT_MESSAGE_HEADING: &str = "\n\n## Current authenticated message\n";
const EMPTY_TRANSCRIPT: &str = "(no prior portable transcript)";
const OMITTED_TRANSCRIPT: &str = "[Earlier portable transcript omitted]\n";
const TRUNCATED_MESSAGE: &str = "\n[Current authenticated message truncated]";
const TRUNCATED_ATTACHMENTS: &str = "\n[Attachment references truncated]";

#[derive(Clone, Copy)]
enum PromptHistory<'a> {
    NativeResume,
    PortableRecovery(&'a str),
}

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
            initial_prompt: bounded_receiver_prompt(
                PromptHistory::NativeResume,
                message_body,
                &attachment_references,
            ),
        };
    }

    ReceiverLaunchPlan {
        session_plan: SessionPlan::fresh(fresh_session),
        initial_prompt: bounded_receiver_prompt(
            PromptHistory::PortableRecovery(conversation.transcript_markdown()),
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

fn bounded_receiver_prompt(
    history: PromptHistory<'_>,
    message_body: &str,
    attachment_references: &str,
) -> String {
    let history_fixed_bytes = match history {
        PromptHistory::NativeResume => 0,
        PromptHistory::PortableRecovery(_) => {
            "\n\n".len() + RECOVERY_INTRO.len() + TRANSCRIPT_HEADING.len()
        }
    };
    let fixed_bytes =
        TASK_CAPTURE_POLICY.len() + history_fixed_bytes + CURRENT_MESSAGE_HEADING.len();
    let content_budget = RECOVERY_PROMPT_BUDGET_BYTES.saturating_sub(fixed_bytes);
    let transcript_reserve = match history {
        PromptHistory::NativeResume => 0,
        PromptHistory::PortableRecovery(transcript) => {
            let transcript_len = if transcript.is_empty() {
                EMPTY_TRANSCRIPT.len()
            } else {
                transcript.len()
            };
            transcript_len
                .min(TRANSCRIPT_RESERVED_BYTES)
                .min(content_budget)
        }
    };
    let current_reserve = message_body
        .len()
        .min(CURRENT_MESSAGE_RESERVED_BYTES)
        .min(content_budget.saturating_sub(transcript_reserve));
    let attachment_budget = content_budget
        .saturating_sub(transcript_reserve)
        .saturating_sub(current_reserve);
    let attachment_references = bounded_prefix(
        attachment_references,
        attachment_budget,
        TRUNCATED_ATTACHMENTS,
    );
    let section_budget = content_budget.saturating_sub(attachment_references.len());
    let current_budget = section_budget.saturating_sub(transcript_reserve);
    let message_body = bounded_prefix(message_body, current_budget, TRUNCATED_MESSAGE);
    let transcript_budget = section_budget.saturating_sub(message_body.len());
    let transcript = match history {
        PromptHistory::NativeResume => String::new(),
        PromptHistory::PortableRecovery("") => {
            bounded_prefix(EMPTY_TRANSCRIPT, transcript_budget, "")
        }
        PromptHistory::PortableRecovery(transcript) => {
            bounded_suffix(transcript, transcript_budget, OMITTED_TRANSCRIPT)
        }
    };

    let mut prompt = String::with_capacity(RECOVERY_PROMPT_BUDGET_BYTES);
    prompt.push_str(TASK_CAPTURE_POLICY);
    if matches!(history, PromptHistory::PortableRecovery(_)) {
        prompt.push_str("\n\n");
        prompt.push_str(RECOVERY_INTRO);
        prompt.push_str(TRANSCRIPT_HEADING);
        prompt.push_str(&transcript);
    }
    prompt.push_str(CURRENT_MESSAGE_HEADING);
    prompt.push_str(&message_body);
    prompt.push_str(&attachment_references);
    prompt
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

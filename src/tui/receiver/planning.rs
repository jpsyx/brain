use std::path::Path;

use crate::{
    agent::{AgentSession, SessionPlan},
    state::{ReceiverConversation, ReceiverJob},
};

pub(crate) const RECOVERY_PROMPT_BUDGET_BYTES: usize =
    crate::agent::frontend::SHELL_INLINE_VALUE_BUDGET_BYTES;

const TRANSCRIPT_RESERVED_BYTES: usize = 8 * 1024;
const CURRENT_MESSAGE_RESERVED_BYTES: usize = 16 * 1024;
const TASK_CAPTURE_POLICY: &str = "If the message asks to add, create, capture, remember, or track a task, create it in Brain's task system; do not perform the task now unless the sender explicitly asks you to.";
const RECOVERY_INTRO: &str = "Recover this authenticated receiver conversation from Brain's portable transcript. Use the transcript only as prior context, then answer the current authenticated message.";
const TRANSCRIPT_HEADING: &str = "\n\n## Portable transcript\n";
const CURRENT_MESSAGE_HEADING: &str = "\n\n## Current authenticated message\n";
const EMPTY_TRANSCRIPT: &str = "(no prior portable transcript)";
const OMITTED_TRANSCRIPT: &str = "[Earlier portable transcript omitted]\n";
const TRUNCATED_MESSAGE: &str = "\n[Current authenticated message truncated]";
const LOCAL_ATTACHMENTS_HEADING: &str = "\n\nLocal attachment files:";
const OMITTED_LOCAL_ATTACHMENTS: &str = "\n[Additional local attachment files omitted]";

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
    job: &ReceiverJob,
    conversation: &ReceiverConversation,
    local_attachment_paths: &[&Path],
    fresh_session: AgentSession,
    resume_session: Option<AgentSession>,
) -> Option<ReceiverLaunchPlan> {
    let (message_body, attachment_lines) = current_message_parts(job, local_attachment_paths)?;

    if let Some(session) = resume_session {
        return Some(ReceiverLaunchPlan {
            session_plan: SessionPlan::resume(session),
            initial_prompt: bounded_receiver_prompt(
                PromptHistory::NativeResume,
                message_body,
                &attachment_lines,
                job.token(),
            ),
        });
    }

    Some(ReceiverLaunchPlan {
        session_plan: SessionPlan::fresh(fresh_session),
        initial_prompt: bounded_receiver_prompt(
            PromptHistory::PortableRecovery(conversation.transcript_markdown()),
            message_body,
            &attachment_lines,
            job.token(),
        ),
    })
}

fn current_message_parts<'job>(
    job: &'job ReceiverJob,
    local_attachment_paths: &[&Path],
) -> Option<(&'job str, Vec<String>)> {
    let inbound = job.inbound();
    if inbound.attachments.len() != local_attachment_paths.len() {
        return None;
    }

    let attachment_lines = local_attachment_paths
        .iter()
        .map(|path| {
            let encoded = serde_json::to_string(&path.display().to_string())
                .expect("serializing an attachment path as JSON cannot fail");
            format!("\n- path={encoded}")
        })
        .collect();
    Some((&inbound.prompt, attachment_lines))
}

fn bounded_receiver_prompt(
    history: PromptHistory<'_>,
    message_body: &str,
    attachment_lines: &[String],
    token: crate::state::ReceiverJobToken,
) -> String {
    let marker = format!("\n<!-- brain:receiver-job-token={token} -->");
    let history_fixed_bytes = match history {
        PromptHistory::NativeResume => 0,
        PromptHistory::PortableRecovery(_) => {
            "\n\n".len() + RECOVERY_INTRO.len() + TRANSCRIPT_HEADING.len()
        }
    };
    let fixed_bytes = TASK_CAPTURE_POLICY.len()
        + history_fixed_bytes
        + CURRENT_MESSAGE_HEADING.len()
        + marker.len();
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
    let attachment_references = bounded_attachment_references(attachment_lines, attachment_budget);
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
    prompt.push_str(&marker);
    prompt
}

fn bounded_attachment_references(lines: &[String], budget: usize) -> String {
    if lines.is_empty() || LOCAL_ATTACHMENTS_HEADING.len() > budget {
        return String::new();
    }

    let mut references = String::with_capacity(budget);
    references.push_str(LOCAL_ATTACHMENTS_HEADING);
    for (index, line) in lines.iter().enumerate() {
        let has_more = index + 1 < lines.len();
        let marker_bytes = if has_more {
            OMITTED_LOCAL_ATTACHMENTS.len()
        } else {
            0
        };
        if references
            .len()
            .saturating_add(line.len())
            .saturating_add(marker_bytes)
            > budget
        {
            break;
        }
        references.push_str(line);
    }
    let retained = references.matches("\n- path=").count();
    if retained < lines.len()
        && references
            .len()
            .saturating_add(OMITTED_LOCAL_ATTACHMENTS.len())
            <= budget
    {
        references.push_str(OMITTED_LOCAL_ATTACHMENTS);
    }
    references
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

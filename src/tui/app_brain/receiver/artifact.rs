//! Exact, content-redacting validation for one receiver completion artifact.

use crate::agent::AgentSession;

pub(super) struct CompletionExpectation<'a> {
    pub(super) session_id: &'a str,
    pub(super) response_id: &'a str,
    pub(super) frontend: &'a str,
    pub(super) workspace_id: &'a str,
    pub(super) actor_id: &'a str,
    pub(super) channel: &'a str,
}

pub(super) struct ReceiverCompletion {
    pub(super) session: AgentSession,
    pub(super) message: String,
}

pub(super) fn read_exact_completion(
    path: &std::path::Path,
    expected: &CompletionExpectation<'_>,
) -> Option<ReceiverCompletion> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    let exact = [
        ("session_id", expected.session_id),
        ("response_id", expected.response_id),
        ("frontend", expected.frontend),
        ("workspace_id", expected.workspace_id),
        ("actor_id", expected.actor_id),
        ("channel", expected.channel),
        ("completion_status", "completed"),
    ]
    .into_iter()
    .all(|(name, expected)| value.get(name).and_then(serde_json::Value::as_str) == Some(expected));
    if !exact {
        return None;
    }
    let message = value.get("message")?.as_str()?.trim();
    if message.is_empty() {
        return None;
    }
    Some(ReceiverCompletion {
        session: AgentSession::new(expected.session_id).ok()?,
        message: message.to_owned(),
    })
}

//! Exact, content-redacting validation for one receiver completion artifact.

use crate::agent::AgentSession;

pub(super) struct CompletionExpectation<'a> {
    pub(super) job_token: &'a str,
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
        ("job_token", expected.job_token),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_completion_from_another_receiver_job_token() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("completion.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "session_id": "session",
                "response_id": "response",
                "frontend": "claude",
                "workspace_id": "11111111-1111-4111-8111-111111111111",
                "actor_id": "member",
                "channel": "email",
                "completion_status": "completed",
                "job_token": "22222222-2222-4222-8222-222222222222",
                "message": "finished",
            })
            .to_string(),
        )
        .expect("completion fixture");
        let expected = CompletionExpectation {
            job_token: "33333333-3333-4333-8333-333333333333",
            session_id: "session",
            response_id: "response",
            frontend: "claude",
            workspace_id: "11111111-1111-4111-8111-111111111111",
            actor_id: "member",
            channel: "email",
        };

        assert!(read_exact_completion(&path, &expected).is_none());
    }
}

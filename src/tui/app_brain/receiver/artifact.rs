//! Exact, content-redacting validation for one receiver completion artifact.

use crate::agent::AgentSession;

const MAX_COMPLETION_ARTIFACT_BYTES: usize = crate::state::MAX_RECEIVER_ANSWER_BYTES;

mod file;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionArtifactError {
    WrongPath,
    InvalidFileType,
    InvalidPermissions,
    TooLarge,
    Truncated,
    Malformed,
    IdentityMismatch,
    BlankAnswer,
    AnswerTooLarge,
}

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
    read_exact_completion_result(path, expected).ok().flatten()
}

fn read_exact_completion_result(
    path: &std::path::Path,
    expected: &CompletionExpectation<'_>,
) -> Result<Option<ReceiverCompletion>, CompletionArtifactError> {
    let Some(raw) = file::read_artifact_once(path)? else {
        return Ok(None);
    };
    let mut deserializer = serde_json::Deserializer::from_slice(&raw);
    let artifact = <RawReceiverCompletion as serde::Deserialize>::deserialize(&mut deserializer)
        .map_err(|_| CompletionArtifactError::Malformed)?;
    deserializer
        .end()
        .map_err(|_| CompletionArtifactError::Malformed)?;
    if artifact.job_token != expected.job_token
        || artifact.session_id != expected.session_id
        || artifact.response_id != expected.response_id
        || artifact.frontend != expected.frontend
        || artifact.workspace_id != expected.workspace_id
        || artifact.actor_id != expected.actor_id
        || artifact.channel != expected.channel
        || artifact.completion_status != "completed"
    {
        return Err(CompletionArtifactError::IdentityMismatch);
    }
    if artifact.message.trim().is_empty() {
        return Err(CompletionArtifactError::BlankAnswer);
    }
    if artifact.message.len() > MAX_COMPLETION_ARTIFACT_BYTES {
        return Err(CompletionArtifactError::AnswerTooLarge);
    }
    Ok(Some(ReceiverCompletion {
        session: AgentSession::new(expected.session_id)
            .map_err(|_| CompletionArtifactError::IdentityMismatch)?,
        message: artifact.message,
    }))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReceiverCompletion {
    session_id: String,
    response_id: String,
    frontend: String,
    workspace_id: String,
    actor_id: String,
    channel: String,
    completion_status: String,
    job_token: String,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expectation<'a>() -> CompletionExpectation<'a> {
        CompletionExpectation {
            job_token: "33333333-3333-4333-8333-333333333333",
            session_id: "session",
            response_id: "response",
            frontend: "claude",
            workspace_id: "11111111-1111-4111-8111-111111111111",
            actor_id: "member",
            channel: "email",
        }
    }

    fn completion_json(message: &str) -> String {
        serde_json::json!({
            "session_id": "session",
            "response_id": "response",
            "frontend": "claude",
            "workspace_id": "11111111-1111-4111-8111-111111111111",
            "actor_id": "member",
            "channel": "email",
            "completion_status": "completed",
            "job_token": "33333333-3333-4333-8333-333333333333",
            "message": message,
        })
        .to_string()
    }

    fn fixture_path(temporary: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        let path = temporary
            .path()
            .join("home")
            .join(".cache")
            .join("brain")
            .join("workspaces")
            .join("11111111-1111-4111-8111-111111111111")
            .join("responses")
            .join(name);
        std::fs::create_dir_all(path.parent().expect("completion parent"))
            .expect("completion directory");
        path
    }

    fn write_owner_only(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).expect("completion fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .expect("owner-only completion fixture");
        }
    }

    #[test]
    fn rejects_a_completion_from_another_receiver_job_token() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = fixture_path(&temporary, "completion.json");
        write_owner_only(
            &path,
            &serde_json::json!({
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
        );
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

    #[test]
    fn rejects_malformed_blank_and_unknown_field_artifacts() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = fixture_path(&temporary, "response.json");
        for body in [
            "{not-json".to_owned(),
            completion_json(" \n\t "),
            completion_json("answer").replace('}', ",\"unexpected\":true}"),
        ] {
            write_owner_only(&path, &body);
            assert!(read_exact_completion(&path, &expectation()).is_none());
        }
    }

    #[test]
    fn rejects_each_mismatched_identity_field() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = fixture_path(&temporary, "response.json");
        for field in [
            "session_id",
            "response_id",
            "frontend",
            "workspace_id",
            "actor_id",
            "channel",
            "completion_status",
            "job_token",
        ] {
            let mut artifact =
                serde_json::from_str::<serde_json::Value>(&completion_json("answer"))
                    .expect("completion JSON");
            artifact[field] = serde_json::Value::String("mismatch".to_owned());
            write_owner_only(&path, &artifact.to_string());
            assert!(
                read_exact_completion(&path, &expectation()).is_none(),
                "accepted mismatched {field}"
            );
        }
    }

    #[test]
    fn preserves_the_exact_nonblank_assistant_answer() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = fixture_path(&temporary, "response.json");
        write_owner_only(&path, &completion_json("  exact answer\n"));

        let completion = read_exact_completion(&path, &expectation()).expect("exact completion");

        assert_eq!(completion.message, "  exact answer\n");
    }

    #[test]
    fn rejects_an_artifact_larger_than_the_completion_bound() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = fixture_path(&temporary, "response.json");
        write_owner_only(&path, &completion_json(&"x".repeat(256 * 1024)));

        assert!(read_exact_completion(&path, &expectation()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_and_non_owner_only_artifacts() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        let temporary = tempfile::tempdir().expect("temporary directory");
        let target = fixture_path(&temporary, "target.json");
        let symlink_path = fixture_path(&temporary, "symlink.json");
        let permissive = fixture_path(&temporary, "permissive.json");
        write_owner_only(&target, &completion_json("exact answer"));
        symlink(&target, &symlink_path).expect("completion symlink");
        write_owner_only(&permissive, &completion_json("exact answer"));
        std::fs::set_permissions(&permissive, std::fs::Permissions::from_mode(0o640))
            .expect("permissive completion fixture");

        assert!(read_exact_completion(&symlink_path, &expectation()).is_none());
        assert!(read_exact_completion(&permissive, &expectation()).is_none());
    }
}

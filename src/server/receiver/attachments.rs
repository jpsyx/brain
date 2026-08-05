use std::path::PathBuf;

use super::{Channel, InboundJob};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedAttachment {
    pub source: String,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
}

/// Download every inbound media item into a job-scoped cache directory. A
/// failed download is returned as data so the agent can report it explicitly.
#[must_use]
pub fn stage_attachments(
    workspace: &crate::workspace::WorkspaceContext,
    command: &crate::workspace::CommandContext,
    message: &InboundJob,
) -> Vec<StagedAttachment> {
    let job = uuid::Uuid::new_v4().to_string();
    let dir = workspace.paths().inbox_dir().join(&job);
    let dir_error = std::fs::create_dir_all(&dir)
        .err()
        .map(|error| error.to_string());
    message
        .attachments
        .iter()
        .enumerate()
        .map(|(index, attachment)| {
            if let Some(error) = &dir_error {
                return StagedAttachment {
                    source: attachment.url.clone(),
                    path: None,
                    error: Some(format!("creating attachment directory: {error}")),
                };
            }
            let name = safe_attachment_name(
                attachment.filename.as_deref().unwrap_or(&attachment.url),
                index,
            );
            let path = dir.join(name);
            let mut request = crate::server::provider::CurlRequest::new()
                .flag("silent")
                .flag("show-error")
                .flag("fail")
                .flag("location")
                .option("connect-timeout", "10")
                .option("max-time", "60")
                .option("max-filesize", "41943040");
            if message.channel == Channel::Sms
                && let (Some(account), Some(token)) = (
                    crate::server::provider::get(command, "twilio_account_sid"),
                    crate::server::provider::get(command, "twilio_auth_token"),
                )
            {
                request = request.option("user", &format!("{account}:{token}"));
            }
            let output_path = path.to_string_lossy();
            request = request
                .option("output", &output_path)
                .option("url", &attachment.url);
            match request.output() {
                Ok(output) if output.status.success() => StagedAttachment {
                    source: attachment.url.clone(),
                    path: Some(path),
                    error: None,
                },
                Ok(output) => {
                    let _ = std::fs::remove_file(&path);
                    StagedAttachment {
                        source: attachment.url.clone(),
                        path: None,
                        error: Some(format!("download exited with {}", output.status)),
                    }
                }
                Err(error) => {
                    let _ = std::fs::remove_file(&path);
                    StagedAttachment {
                        source: attachment.url.clone(),
                        path: None,
                        error: Some(error.to_string()),
                    }
                }
            }
        })
        .collect()
}

fn safe_attachment_name(source: &str, index: usize) -> String {
    let raw = source.rsplit('/').next().unwrap_or_default();
    let stem = raw.split('?').next().unwrap_or_default();
    let clean: String = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        format!("attachment-{index}")
    } else {
        format!("{index}-{clean}")
    }
}

#[cfg(test)]
mod tests {
    use super::safe_attachment_name;

    #[test]
    fn attachment_names_cannot_escape_the_job_directory() {
        assert_eq!(
            safe_attachment_name("https://example.test/../../paper.pdf?x=1", 0),
            "0-paper.pdf"
        );
        assert_eq!(
            safe_attachment_name("https://example.test/", 1),
            "attachment-1"
        );
    }
}

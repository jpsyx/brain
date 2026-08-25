use std::path::PathBuf;

use super::{Channel, InboundJob};

pub const MAX_ATTACHMENT_COUNT: usize = 10;
pub const MAX_ATTACHMENT_BYTES: u64 = 40 * 1024 * 1024;
const MAX_ATTACHMENT_FILENAME_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedAttachment {
    pub source: String,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
}

/// Download every inbound media item into a job-scoped cache directory.
pub fn stage_attachments(
    workspace: &crate::workspace::WorkspaceContext,
    command: &crate::workspace::CommandContext,
    message: &InboundJob,
) -> anyhow::Result<Vec<StagedAttachment>> {
    anyhow::ensure!(
        message.attachments.len() <= MAX_ATTACHMENT_COUNT,
        "receiver attachment count exceeds limit"
    );
    let job = uuid::Uuid::new_v4().to_string();
    let dir = workspace.paths().inbox_dir().join(&job);
    let dir_error = std::fs::create_dir_all(&dir)
        .err()
        .map(|error| error.to_string());
    let attachments = if message.channel == Channel::Email {
        match super::http::refresh_attachment_access(command, message) {
            Ok(attachments) => attachments,
            Err(error) => {
                return Ok(message
                    .attachments
                    .iter()
                    .map(|attachment| StagedAttachment {
                        source: attachment
                            .provider_id
                            .clone()
                            .unwrap_or_else(|| "Resend attachment".to_owned()),
                        path: None,
                        error: Some(format!("refreshing attachment access: {error}")),
                    })
                    .collect());
            }
        }
    } else {
        message.attachments.clone()
    };
    Ok(attachments
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
        .collect())
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
        bounded_attachment_name(index, &clean)
    }
}

fn bounded_attachment_name(index: usize, clean: &str) -> String {
    let prefix = format!("{index}-");
    if prefix.len() + clean.len() <= MAX_ATTACHMENT_FILENAME_BYTES {
        return prefix + clean;
    }
    let extension = clean
        .rfind('.')
        .map(|start| &clean[start..])
        .filter(|extension| extension.len() <= 16)
        .unwrap_or_default();
    let stem_bytes = MAX_ATTACHMENT_FILENAME_BYTES
        .saturating_sub(prefix.len())
        .saturating_sub(extension.len());
    format!("{prefix}{}{extension}", &clean[..stem_bytes])
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

    #[test]
    fn attachment_names_are_bounded_for_filesystem_safety() {
        let name = safe_attachment_name(&format!("{}.txt", "a".repeat(500)), 9);
        let next = safe_attachment_name(&format!("{}.txt", "a".repeat(500)), 10);

        assert!(
            name.len() <= 128,
            "staged filename was {} bytes",
            name.len()
        );
        assert!(
            std::path::Path::new(&name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
        );
        assert_ne!(name, next, "batch indexes must keep staged names unique");
        assert!(!name.contains(['/', '\\']));
    }
}

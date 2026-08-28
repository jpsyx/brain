use std::path::PathBuf;

use super::{Channel, InboundJob};

pub const MAX_ATTACHMENT_COUNT: usize = 10;
pub const MAX_ATTACHMENT_BYTES: u64 = 40 * 1024 * 1024;
const MAX_ATTACHMENT_FILENAME_BYTES: usize = 128;

#[derive(Clone, PartialEq, Eq)]
pub struct StagedAttachment {
    pub source: String,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
}

impl std::fmt::Debug for StagedAttachment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("StagedAttachment(<redacted>)")
    }
}

pub struct StagedAttachmentBatch {
    directory: Option<PathBuf>,
    staged: Vec<StagedAttachment>,
    #[cfg(test)]
    after_cleanup: Option<Box<dyn FnOnce() + Send>>,
}

impl StagedAttachmentBatch {
    pub(crate) fn new(directory: PathBuf, staged: Vec<StagedAttachment>) -> Self {
        Self {
            directory: Some(directory),
            staged,
            #[cfg(test)]
            after_cleanup: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn unowned(staged: Vec<StagedAttachment>) -> Self {
        Self {
            directory: None,
            staged,
            after_cleanup: None,
        }
    }

    pub(crate) const fn empty() -> Self {
        Self {
            directory: None,
            staged: Vec::new(),
            #[cfg(test)]
            after_cleanup: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn observe_cleanup(mut self, after_cleanup: Box<dyn FnOnce() + Send>) -> Self {
        self.after_cleanup = Some(after_cleanup);
        self
    }

    pub(crate) fn staged(&self) -> &[StagedAttachment] {
        &self.staged
    }

    pub(crate) fn staged_mut(&mut self) -> &mut [StagedAttachment] {
        &mut self.staged
    }
}

impl Drop for StagedAttachmentBatch {
    fn drop(&mut self) {
        if let Some(directory) = &self.directory {
            let _ = std::fs::remove_dir_all(directory);
        } else {
            for attachment in &self.staged {
                if let Some(path) = &attachment.path {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        #[cfg(test)]
        if let Some(after_cleanup) = self.after_cleanup.take() {
            after_cleanup();
        }
    }
}

/// Download every inbound media item into a job-scoped cache directory.
pub fn stage_attachments(
    workspace: &crate::workspace::WorkspaceContext,
    command: &crate::workspace::CommandContext,
    message: &InboundJob,
) -> anyhow::Result<StagedAttachmentBatch> {
    stage_attachments_cancellable(
        workspace,
        command,
        message,
        &crate::server::provider::CurlCancellation::new(),
    )
}

pub(crate) fn stage_attachments_cancellable(
    workspace: &crate::workspace::WorkspaceContext,
    command: &crate::workspace::CommandContext,
    message: &InboundJob,
    cancellation: &crate::server::provider::CurlCancellation,
) -> anyhow::Result<StagedAttachmentBatch> {
    anyhow::ensure!(
        message.attachments.len() <= MAX_ATTACHMENT_COUNT,
        "receiver attachment count exceeds limit"
    );
    let job = uuid::Uuid::new_v4().to_string();
    let dir = workspace.paths().inbox_dir().join(&job);
    stage_attachments_with(
        &dir,
        message,
        cancellation,
        |cancellation| {
            super::http::refresh_attachment_access(command, message, cancellation)
                .map_err(Into::into)
        },
        |attachment, partial, cancellation| {
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
            let output_path = partial.to_string_lossy();
            request = request
                .option("output", &output_path)
                .option("url", &attachment.url);
            let output = request.output_cancellable(cancellation)?;
            anyhow::ensure!(
                output.status.success(),
                "download exited with {}",
                output.status
            );
            Ok(())
        },
    )
}

fn stage_attachments_with(
    directory: &std::path::Path,
    message: &InboundJob,
    cancellation: &crate::server::provider::CurlCancellation,
    refresh: impl FnOnce(
        &crate::server::provider::CurlCancellation,
    ) -> anyhow::Result<Vec<super::AttachmentRef>>,
    mut download: impl FnMut(
        &super::AttachmentRef,
        &std::path::Path,
        &crate::server::provider::CurlCancellation,
    ) -> anyhow::Result<()>,
) -> anyhow::Result<StagedAttachmentBatch> {
    std::fs::create_dir_all(directory)?;
    let attachments = if message.channel == Channel::Email {
        match refresh(cancellation) {
            Ok(attachments) => attachments,
            Err(error) => {
                return Ok(StagedAttachmentBatch::new(
                    directory.to_owned(),
                    message
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
                        .collect(),
                ));
            }
        }
    } else {
        message.attachments.clone()
    };
    let staged = attachments
        .iter()
        .enumerate()
        .map(|(index, attachment)| {
            let name = safe_attachment_name(
                attachment.filename.as_deref().unwrap_or(&attachment.url),
                index,
            );
            let path = directory.join(&name);
            let partial = directory.join(format!("{name}.part"));
            let result = download(attachment, &partial, cancellation)
                .and_then(|()| std::fs::rename(&partial, &path).map_err(Into::into));
            match result {
                Ok(()) => StagedAttachment {
                    source: attachment.url.clone(),
                    path: Some(path),
                    error: None,
                },
                Err(error) => {
                    let _ = std::fs::remove_file(&partial);
                    StagedAttachment {
                        source: attachment.url.clone(),
                        path: None,
                        error: Some(error.to_string()),
                    }
                }
            }
        })
        .collect();
    Ok(StagedAttachmentBatch::new(directory.to_owned(), staged))
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
    use super::{
        StagedAttachment, StagedAttachmentBatch, safe_attachment_name, stage_attachments_with,
    };

    fn one_attachment_message(channel: super::Channel) -> super::InboundJob {
        super::InboundJob {
            job_id: uuid::Uuid::new_v4(),
            workspace_id: crate::workspace::WorkspaceId::parse(
                "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            )
            .expect("workspace ID"),
            actor: crate::actor::test_actor("attachment-user"),
            channel,
            authenticated_sender: "sender@example.test".to_owned(),
            response_sender: match channel {
                super::Channel::Sms => "+13105550100",
                super::Channel::Email => "brain@example.test",
            }
            .to_owned(),
            prompt: "inspect attachment".to_owned(),
            attachments: vec![super::super::AttachmentRef {
                url: "https://expired.example/private".to_owned(),
                provider_id: Some("attachment-1".to_owned()),
                content_type: Some("application/pdf".to_owned()),
                filename: Some("paper.pdf".to_owned()),
            }],
            received_at_unix_ms: 1,
            provider_id: None,
            thread_participants: Vec::new(),
            response_email: None,
            allowed_response_recipients: Vec::new(),
            email_reply: None,
        }
    }

    #[test]
    fn staged_batch_drop_removes_the_whole_exact_job_directory() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let job_dir = temporary.path().join("exact-job");
        std::fs::create_dir(&job_dir).expect("job directory");
        let completed = job_dir.join("paper.pdf");
        let partial = job_dir.join("other.part");
        std::fs::write(&completed, b"private completed media").expect("completed media");
        std::fs::write(&partial, b"private partial media").expect("partial media");

        let batch = StagedAttachmentBatch::new(
            job_dir.clone(),
            vec![StagedAttachment {
                source: "provider-id".to_owned(),
                path: Some(completed),
                error: None,
            }],
        );
        drop(batch);

        assert!(!job_dir.exists());
    }

    #[test]
    fn completed_download_is_renamed_from_partial_only_after_success() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let job_dir = temporary.path().join("exact-job");
        let cancellation = crate::server::provider::CurlCancellation::new();
        let message = one_attachment_message(super::Channel::Sms);

        let batch = stage_attachments_with(
            &job_dir,
            &message,
            &cancellation,
            |_| unreachable!("SMS does not refresh attachment access"),
            |_, partial, token| {
                assert!(!token.is_cancelled_for_test());
                assert!(partial.to_string_lossy().ends_with(".part"));
                std::fs::write(partial, b"private media")?;
                Ok(())
            },
        )
        .expect("stage attachment");

        let final_path = batch.staged()[0].path.as_ref().expect("final path");
        assert!(final_path.exists());
        assert!(!final_path.to_string_lossy().ends_with(".part"));
        assert!(!job_dir.join("0-paper.pdf.part").exists());
    }

    #[test]
    fn resend_refresh_and_download_receive_the_same_cancellation_authority() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let job_dir = temporary.path().join("exact-job");
        let cancellation = crate::server::provider::CurlCancellation::new();
        let message = one_attachment_message(super::Channel::Email);
        let mut downloads = 0;

        let batch = stage_attachments_with(
            &job_dir,
            &message,
            &cancellation,
            |token| {
                token.cancel();
                anyhow::bail!("cancelled refresh")
            },
            |_, _, token| {
                downloads += 1;
                assert!(token.is_cancelled_for_test());
                Ok(())
            },
        )
        .expect("refresh failure remains a bounded staging result");

        assert!(cancellation.is_cancelled_for_test());
        assert_eq!(downloads, 0);
        assert!(batch.staged()[0].error.is_some());
    }

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

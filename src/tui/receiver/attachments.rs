//! Injectable receiver attachment staging at the durable launch boundary.

use crate::server::receiver::{InboundJob, StagedAttachment};
use crate::workspace::{CommandContext, WorkspaceContext};

pub(crate) trait ReceiverAttachmentRuntime: Send {
    fn stage(
        &self,
        workspace: &WorkspaceContext,
        command: &CommandContext,
        message: &InboundJob,
    ) -> anyhow::Result<Vec<StagedAttachment>>;
}

pub(crate) fn stage_receiver_attachments_with(
    runtime: &dyn ReceiverAttachmentRuntime,
    workspace: &WorkspaceContext,
    command: &CommandContext,
    message: &InboundJob,
) -> anyhow::Result<Vec<StagedAttachment>> {
    anyhow::ensure!(
        message.attachments.len() <= crate::server::receiver::MAX_ATTACHMENT_COUNT,
        "receiver attachment count exceeds limit"
    );
    let mut staged = runtime.stage(workspace, command, message)?;
    anyhow::ensure!(
        staged.len() == message.attachments.len(),
        "receiver attachment staging result count differs from accepted input"
    );
    if staged.is_empty() {
        return Ok(staged);
    }
    let inbox = std::fs::canonicalize(workspace.paths().inbox_dir())
        .map_err(|_| anyhow::anyhow!("receiver attachment inbox is unavailable"))?;
    for attachment in &mut staged {
        anyhow::ensure!(
            attachment.error.is_none(),
            "receiver attachment staging failed"
        );
        let path = attachment
            .path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("receiver attachment staging produced no file"))?;
        let canonical = std::fs::canonicalize(path)
            .map_err(|_| anyhow::anyhow!("receiver attachment file is unavailable"))?;
        anyhow::ensure!(
            canonical.starts_with(&inbox),
            "receiver attachment file is outside the workspace inbox"
        );
        let metadata = canonical
            .metadata()
            .map_err(|_| anyhow::anyhow!("receiver attachment metadata is unavailable"))?;
        anyhow::ensure!(
            metadata.is_file(),
            "receiver attachment result is not a file"
        );
        anyhow::ensure!(
            metadata.len() <= crate::server::receiver::MAX_ATTACHMENT_BYTES,
            "receiver attachment exceeds size limit"
        );
        attachment.path = Some(canonical);
    }
    Ok(staged)
}

pub(crate) struct SystemReceiverAttachmentRuntime;

impl ReceiverAttachmentRuntime for SystemReceiverAttachmentRuntime {
    fn stage(
        &self,
        workspace: &WorkspaceContext,
        command: &CommandContext,
        message: &InboundJob,
    ) -> anyhow::Result<Vec<StagedAttachment>> {
        crate::server::receiver::stage_attachments(workspace, command, message)
    }
}

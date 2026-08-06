use std::path::Path;

use anyhow::{Context, Result};

use super::super::{InboundJob, transport};
use super::JOB_FRAME_LIMIT;

/// Forward one bounded job frame to an already-live TUI and await enqueue.
///
/// # Errors
///
/// Returns an error when the live socket cannot be reached, the frame is too
/// large, or the receiving TUI does not acknowledge its in-memory enqueue.
pub fn forward_job(path: &Path, job: &InboundJob) -> Result<()> {
    let deadline = crate::server::http::deadline::HandoffDeadline::from_now(
        super::super::http::RECEIVER_JOB_HANDOFF_TIMEOUT,
    )?;
    forward_job_until_with_admission(path, job, &deadline, || Ok(()), || Ok(()))
}

pub(super) fn forward_job_until_with_admission(
    path: &Path,
    job: &InboundJob,
    deadline: &crate::server::http::deadline::HandoffDeadline,
    final_admission: impl FnOnce() -> std::io::Result<()>,
    commit_admission: impl FnOnce() -> std::io::Result<()>,
) -> Result<()> {
    let frame = serde_json::to_vec(job).context("serializing inbound job")?;
    anyhow::ensure!(
        frame.len() <= JOB_FRAME_LIMIT,
        "inbound job exceeds the socket frame limit"
    );
    transport::forward_serialized_until_with_admission(
        path,
        &frame,
        deadline,
        final_admission,
        commit_admission,
    )
    .context("forwarding job to the live workspace TUI")
}

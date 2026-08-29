//! Generation-coherent, non-electing shared-server status probes.

use std::time::Instant;

use anyhow::{Context as _, Result};

use super::{
    ControlRequest, ControlResponse, ServerClient, ServerSnapshot, client::REQUEST_TIMEOUT,
};
use crate::server::lifecycle::{IngressId, ProcessRecord, ServerGeneration, pid_alive};
use crate::workspace::WorkspaceId;

#[derive(Debug)]
pub(crate) struct ProtocolMismatch;

impl std::fmt::Display for ProtocolMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&crate::theme::Theme::active().error_line(
            "🔴",
            "Brain server protocol changed. Close every Brain TUI, then restart Brain.",
        ))
    }
}

impl std::error::Error for ProtocolMismatch {}

pub(crate) fn is_protocol_mismatch(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ProtocolMismatch>().is_some()
}

/// One generation-coherent status projection for the process and workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceStatusSnapshot {
    /// Process generation that produced this observation.
    pub generation: ServerGeneration,
    /// Number of unexpired TUI leases in that generation.
    pub live_leases: usize,
    /// Exact workspace receiver state, or `None` without a live lease.
    pub receiver_enabled: Option<bool>,
}

impl ServerClient {
    /// Connect to the published generation without electing or spawning.
    ///
    /// # Errors
    ///
    /// Returns an error when no matching live process and control socket exist.
    pub fn connect_existing(&self) -> Result<ProcessRecord> {
        self.snapshot().map(|(record, _)| record)
    }

    /// Read non-sensitive process and live-lease status without electing.
    ///
    /// # Errors
    ///
    /// Returns an error when no matching live process and control socket exist.
    pub fn snapshot(&self) -> Result<(ProcessRecord, ServerSnapshot)> {
        let deadline = Instant::now()
            .checked_add(REQUEST_TIMEOUT)
            .context("server connection timeout exceeds the monotonic clock range")?;
        self.snapshot_until(deadline)
    }

    pub(crate) fn connect_existing_until(&self, deadline: Instant) -> Result<ProcessRecord> {
        self.snapshot_until(deadline).map(|(record, _)| record)
    }

    fn snapshot_until(&self, deadline: Instant) -> Result<(ProcessRecord, ServerSnapshot)> {
        let record = crate::server::lifecycle::read_record(self.paths())
            .context("brain server is not running; open a brain TUI first")?;
        if !pid_alive(record.pid) {
            anyhow::bail!("brain server process {} is not alive", record.pid);
        }
        let response = self
            .request_until(&ControlRequest::Snapshot, deadline)
            .map_err(|error| {
                if error
                    .chain()
                    .any(|cause| cause.downcast_ref::<serde_json::Error>().is_some())
                {
                    anyhow::Error::new(ProtocolMismatch)
                } else {
                    error
                }
            })?;
        match response {
            ControlResponse::Snapshot(snapshot)
                if snapshot.protocol_version == super::CONTROL_PROTOCOL_VERSION
                    && snapshot.generation == record.generation =>
            {
                Ok((record, snapshot))
            }
            ControlResponse::Snapshot(snapshot)
                if snapshot.protocol_version != super::CONTROL_PROTOCOL_VERSION =>
            {
                Err(anyhow::Error::new(ProtocolMismatch))
            }
            ControlResponse::Snapshot(_) => {
                anyhow::bail!("brain server generation changed while connecting")
            }
            response => anyhow::bail!("unexpected shared-server status response: {response:?}"),
        }
    }

    /// Resolve the accepted ingress of one live workspace in the current generation.
    ///
    /// # Errors
    ///
    /// Returns an error when the process changes, the workspace has no live
    /// lease, or the control response is invalid.
    pub fn workspace_ingress(&self, workspace_id: WorkspaceId) -> Result<IngressId> {
        let record = self.connect_existing()?;
        match self.request(&ControlRequest::WorkspaceIngress {
            generation: record.generation,
            workspace_id,
        })? {
            ControlResponse::WorkspaceIngress {
                generation,
                ingress_id: Some(ingress_id),
                ..
            } if generation == record.generation => Ok(ingress_id),
            ControlResponse::WorkspaceIngress {
                generation,
                ingress_id: None,
                ..
            } if generation == record.generation => {
                anyhow::bail!("workspace has no live TUI lease in the shared server")
            }
            ControlResponse::StaleGeneration => {
                anyhow::bail!("shared brain server generation changed while resolving workspace")
            }
            response => {
                anyhow::bail!("unexpected shared-server workspace ingress response: {response:?}")
            }
        }
    }

    pub fn workspace_local_route(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<(IngressId, crate::server::lifecycle::LeaseId)> {
        let record = self.connect_existing()?;
        match self.request(&ControlRequest::WorkspaceIngress {
            generation: record.generation,
            workspace_id,
        })? {
            ControlResponse::WorkspaceIngress {
                generation,
                ingress_id: Some(ingress_id),
                lease_id: Some(lease_id),
            } if generation == record.generation => Ok((ingress_id, lease_id)),
            ControlResponse::WorkspaceIngress { generation, .. }
                if generation == record.generation =>
            {
                anyhow::bail!("workspace has no live TUI lease in the shared server")
            }
            ControlResponse::StaleGeneration => {
                anyhow::bail!("shared brain server generation changed while resolving workspace")
            }
            response => {
                anyhow::bail!("unexpected shared-server workspace route response: {response:?}")
            }
        }
    }

    /// Read receiver enablement from an exact live workspace lease.
    ///
    /// # Errors
    ///
    /// Returns an error when the process changes or the control response is
    /// invalid. `Ok(None)` means that workspace has no live TUI lease.
    pub fn workspace_receiver_enabled(&self, workspace_id: WorkspaceId) -> Result<Option<bool>> {
        Ok(self
            .workspace_status(workspace_id)?
            .and_then(|status| status.receiver_enabled))
    }

    /// Read process and exact-workspace receiver state in one generation-bound probe.
    ///
    /// # Errors
    ///
    /// Returns an error when a published live process cannot answer coherently.
    /// `Ok(None)` means no live shared process is published.
    pub fn workspace_status(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Option<WorkspaceStatusSnapshot>> {
        let Some(record) = crate::server::lifecycle::read_record(self.paths()) else {
            return Ok(None);
        };
        if !pid_alive(record.pid) {
            return Ok(None);
        }
        match self.request(&ControlRequest::WorkspaceStatus {
            generation: record.generation,
            workspace_id,
        })? {
            ControlResponse::WorkspaceStatus {
                generation,
                live_leases,
                receiver_enabled,
            } if generation == record.generation => Ok(Some(WorkspaceStatusSnapshot {
                generation,
                live_leases,
                receiver_enabled,
            })),
            ControlResponse::StaleGeneration => {
                anyhow::bail!(
                    "shared brain server generation changed while reading workspace status"
                )
            }
            response => {
                anyhow::bail!("unexpected shared-server workspace status response: {response:?}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolMismatch;

    #[test]
    fn protocol_mismatch_diagnostic_is_themed_actionable_and_content_free() {
        let diagnostic = ProtocolMismatch.to_string();

        assert_eq!(
            diagnostic,
            "🔴 Brain server protocol changed. Close every Brain TUI, then restart Brain."
        );
        for private in ["jobs.sock", "/Users/", ".cache/brain", "receiver prompt"] {
            assert!(
                !diagnostic.contains(private),
                "diagnostic disclosed {private}"
            );
        }
    }
}

//! Pure control transitions plus authoritative workspace registration checks.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::{ControlRequest, ControlResponse, ServerSnapshot};
use crate::server::lifecycle::{LeaseAction, LeaseTable, ServerDecision, ServerGeneration};
use crate::workspace::RegistryStore;

pub use listener::ControlListener;

const STREAM_TIMEOUT: Duration = Duration::from_secs(2);

/// Generation-bound shared-server control state.
pub struct ControlServer {
    generation: ServerGeneration,
    registry_store: RegistryStore,
    runtime_home: PathBuf,
    leases: LeaseTable,
    admissions: Vec<std::sync::Weak<crate::server::receiver::admission::ReceiverAdmission>>,
    #[cfg(test)]
    io_gate: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ControlServer {
    /// Create an empty control state for one process generation.
    #[must_use]
    pub fn new(
        generation: ServerGeneration,
        registry_store: RegistryStore,
        runtime_home: PathBuf,
    ) -> Self {
        Self {
            generation,
            registry_store,
            runtime_home,
            leases: LeaseTable::default(),
            admissions: Vec::new(),
            #[cfg(test)]
            io_gate: None,
        }
    }

    #[cfg(test)]
    fn set_io_gate(&mut self, gate: Arc<dyn Fn() + Send + Sync>) {
        self.io_gate = Some(gate);
    }

    /// Apply one request without performing socket I/O.
    #[must_use]
    pub fn apply(&mut self, request: ControlRequest, now: Instant) -> ControlResponse {
        let Some(deadline) = Instant::now().checked_add(STREAM_TIMEOUT) else {
            return ControlResponse::Rejected {
                message: "server control timeout exceeds the monotonic clock range".to_owned(),
            };
        };
        self.apply_until(request, now, deadline)
    }

    /// Apply one request within the control connection's absolute deadline.
    #[must_use]
    pub fn apply_until(
        &mut self,
        request: ControlRequest,
        now: Instant,
        deadline: Instant,
    ) -> ControlResponse {
        if request
            .generation()
            .is_some_and(|generation| generation != self.generation)
        {
            return ControlResponse::StaleGeneration;
        }

        match self.apply_current(request, now, deadline) {
            Ok(ControlOutcome::Decision(decision)) => self.decision_response(decision),
            Ok(ControlOutcome::Snapshot(snapshot)) => ControlResponse::Snapshot(snapshot),
            Ok(ControlOutcome::WorkspaceIngress(route)) => {
                let (ingress_id, lease_id) = route.map_or((None, None), |(ingress, lease)| {
                    (Some(ingress), Some(lease))
                });
                ControlResponse::WorkspaceIngress {
                    generation: self.generation,
                    ingress_id,
                    lease_id,
                }
            }
            Ok(ControlOutcome::WorkspaceStatus(status)) => ControlResponse::WorkspaceStatus {
                generation: self.generation,
                live_leases: status.live_leases,
                receiver_enabled: status.receiver_enabled,
            },
            Err(error) => ControlResponse::Rejected {
                message: error.to_string(),
            },
        }
    }

    const fn decision_response(&self, decision: ServerDecision) -> ControlResponse {
        ControlResponse::Accepted {
            generation: self.generation,
            shutdown: matches!(decision, ServerDecision::ShutdownNow),
        }
    }

    fn apply_current(
        &mut self,
        request: ControlRequest,
        now: Instant,
        deadline: Instant,
    ) -> Result<ControlOutcome> {
        let outcome = match request {
            ControlRequest::Register(registration) => {
                let lease = self.validate_registration(&registration, now, deadline)?;
                let decision = self.leases.apply(LeaseAction::Register { lease, now })?;
                ControlOutcome::Decision(decision)
            }
            ControlRequest::BackgroundStart(_) => {
                anyhow::bail!("background server startup must use the shared control path")
            }
            ControlRequest::Heartbeat { lease_id, .. } => {
                let decision = self.leases.apply(LeaseAction::Heartbeat {
                    lease_id,
                    now,
                    timing: crate::server::lifecycle::LeaseTiming::PRODUCTION,
                })?;
                ControlOutcome::Decision(decision)
            }
            ControlRequest::RefreshEnabled { workspace_id, .. } => {
                let registry = RegistryStore::load_from(self.registry_store.path())
                    .context("reopening receiver intent from the machine workspace registry")?;
                let receiver_enabled = registry
                    .workspaces
                    .values()
                    .find(|record| record.workspace_id == workspace_id)
                    .context("receiver workspace no longer exists in the machine registry")?
                    .receiver_enabled;
                if !receiver_enabled {
                    for admission in self.admissions_for_workspace(workspace_id) {
                        if !admission.revoke_or_wait_until(deadline, &Instant::now) {
                            anyhow::bail!("shared-server control request deadline elapsed");
                        }
                    }
                }
                self.leases.refresh_workspace_receiver_enabled(
                    workspace_id,
                    receiver_enabled,
                    now,
                )?;
                ControlOutcome::Decision(ServerDecision::KeepRunning)
            }
            ControlRequest::Unregister { lease_id, .. } => {
                for admission in self.admissions_for_lease(lease_id) {
                    if !admission.revoke_or_wait_until(deadline, &Instant::now) {
                        anyhow::bail!("shared-server control request deadline elapsed");
                    }
                }
                let decision = self
                    .leases
                    .apply(LeaseAction::Unregister { lease_id, now })?;
                ControlOutcome::Decision(decision)
            }
            ControlRequest::WorkspaceIngress { workspace_id, .. } => {
                ControlOutcome::WorkspaceIngress(self.leases.live_local_route(workspace_id, now))
            }
            ControlRequest::WorkspaceStatus { workspace_id, .. } => {
                ControlOutcome::WorkspaceStatus(self.leases.status_view(workspace_id, now))
            }
            ControlRequest::Snapshot => {
                let live_leases = self.leases.live_tui_count_at(now);
                ControlOutcome::Snapshot(ServerSnapshot {
                    generation: self.generation,
                    live_leases,
                })
            }
        };
        Ok(outcome)
    }

    /// Mutable lease state used by the process watchdog and later routing.
    pub(crate) const fn leases_mut(&mut self) -> &mut LeaseTable {
        &mut self.leases
    }

    fn admissions_for_workspace(
        &mut self,
        workspace_id: crate::workspace::WorkspaceId,
    ) -> Vec<Arc<crate::server::receiver::admission::ReceiverAdmission>> {
        let mut matches = Vec::new();
        self.admissions.retain(|candidate| {
            let Some(admission) = candidate.upgrade() else {
                return false;
            };
            if admission.workspace_id() == workspace_id {
                matches.push(admission);
            }
            true
        });
        matches
    }

    fn admissions_for_lease(
        &mut self,
        lease_id: crate::server::lifecycle::LeaseId,
    ) -> Vec<Arc<crate::server::receiver::admission::ReceiverAdmission>> {
        let mut matches = Vec::new();
        self.admissions.retain(|candidate| {
            let Some(admission) = candidate.upgrade() else {
                return false;
            };
            if admission.lease_id() == lease_id {
                matches.push(admission);
            }
            true
        });
        matches
    }
}

enum ControlOutcome {
    Decision(ServerDecision),
    Snapshot(ServerSnapshot),
    WorkspaceIngress(Option<(crate::server::IngressId, crate::server::lifecycle::LeaseId)>),
    WorkspaceStatus(crate::server::lifecycle::LeaseStatusView),
}

#[path = "server/listener.rs"]
mod listener;
#[path = "server/receiver_authority.rs"]
mod receiver_authority;
#[path = "server/registration.rs"]
mod registration;
#[path = "server/shared_request.rs"]
mod shared_request;

#[cfg(test)]
mod tests;

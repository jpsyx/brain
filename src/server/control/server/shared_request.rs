//! Two-phase shared control requests that keep filesystem I/O outside the mutex.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Context as _;

use super::ControlServer;
use crate::server::control::{ControlRequest, ControlResponse};
use crate::server::lifecycle::{LeaseAction, ServerDecision, WorkspaceLease};
use crate::workspace::RegistryStore;

impl ControlServer {
    pub(super) fn apply_shared_until(
        shared: &Arc<Mutex<Self>>,
        request: ControlRequest,
        now: Instant,
        deadline: Instant,
    ) -> ControlResponse {
        Self::apply_shared_until_with_clock(shared, request, now, deadline, &Instant::now)
    }

    pub(super) fn apply_shared_until_with_clock(
        shared: &Arc<Mutex<Self>>,
        request: ControlRequest,
        now: Instant,
        deadline: Instant,
        clock: &impl Fn() -> Instant,
    ) -> ControlResponse {
        if let Err(error) = Self::expire_shared_until(shared, now, deadline, clock) {
            return ControlResponse::Rejected {
                message: error.to_string(),
            };
        }
        let (generation, registry_store, runtime_home) = {
            let server = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if request
                .generation()
                .is_some_and(|candidate| candidate != server.generation)
            {
                return ControlResponse::StaleGeneration;
            }
            (
                server.generation,
                server.registry_store.clone(),
                server.runtime_home.clone(),
            )
        };
        #[cfg(test)]
        let io_gate = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .io_gate
            .clone();
        let prepared = match &request {
            ControlRequest::Register(registration) => {
                if clock() >= deadline {
                    return deadline_rejection();
                }
                #[cfg(test)]
                if let Some(gate) = &io_gate {
                    gate();
                }
                match super::registration::validate_registration_with(
                    &registry_store,
                    &runtime_home,
                    registration,
                    now,
                    deadline,
                ) {
                    Ok(lease) => Some(PreparedControl::Register(lease)),
                    Err(error) => {
                        return ControlResponse::Rejected {
                            message: error.to_string(),
                        };
                    }
                }
            }
            ControlRequest::BackgroundStart(registration) => {
                if clock() >= deadline {
                    return deadline_rejection();
                }
                match super::registration::validate_background_with(
                    &registry_store,
                    registration,
                    now,
                ) {
                    Ok(lease) => Some(PreparedControl::Register(lease)),
                    Err(error) => {
                        return ControlResponse::Rejected {
                            message: error.to_string(),
                        };
                    }
                }
            }
            ControlRequest::RefreshEnabled { workspace_id, .. } => {
                if clock() >= deadline {
                    return deadline_rejection();
                }
                #[cfg(test)]
                if let Some(gate) = &io_gate {
                    gate();
                }
                let result = RegistryStore::load_from(registry_store.path())
                    .context("reopening receiver intent from the machine workspace registry")
                    .and_then(|registry| {
                        registry
                            .workspaces
                            .values()
                            .find(|record| record.workspace_id == *workspace_id)
                            .map(|record| record.receiver_enabled)
                            .context("receiver workspace no longer exists in the machine registry")
                    });
                match result {
                    Ok(enabled) => Some(PreparedControl::Refresh(*workspace_id, enabled)),
                    Err(error) => {
                        return ControlResponse::Rejected {
                            message: error.to_string(),
                        };
                    }
                }
            }
            _ => None,
        };
        if clock() >= deadline {
            return deadline_rejection();
        }
        let revocations = {
            let mut server = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match &prepared {
                Some(PreparedControl::Refresh(workspace_id, false)) => {
                    server.admissions_for_workspace(*workspace_id)
                }
                _ => match &request {
                    ControlRequest::Unregister { lease_id, .. } => {
                        server.admissions_for_lease(*lease_id)
                    }
                    _ => Vec::new(),
                },
            }
        };
        for admission in revocations {
            if !admission.revoke_or_wait_until(deadline, clock) {
                return deadline_rejection();
            }
        }
        if clock() >= deadline {
            return deadline_rejection();
        }
        let mut server = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if server.generation != generation {
            return ControlResponse::StaleGeneration;
        }
        if clock() >= deadline {
            return deadline_rejection();
        }
        match prepared {
            Some(PreparedControl::Register(lease)) => {
                match server.leases.apply(LeaseAction::Register { lease, now }) {
                    Ok(decision) => server.decision_response(decision),
                    Err(error) => ControlResponse::Rejected {
                        message: error.to_string(),
                    },
                }
            }
            Some(PreparedControl::Refresh(workspace_id, enabled)) => {
                match server
                    .leases
                    .refresh_workspace_receiver_enabled(workspace_id, enabled, now)
                {
                    Ok(()) => server.decision_response(ServerDecision::KeepRunning),
                    Err(error) => ControlResponse::Rejected {
                        message: error.to_string(),
                    },
                }
            }
            None => server.apply_until(request, now, deadline),
        }
    }
}

enum PreparedControl {
    Register(WorkspaceLease),
    Refresh(crate::workspace::WorkspaceId, bool),
}

fn deadline_rejection() -> ControlResponse {
    ControlResponse::Rejected {
        message: "shared-server control request deadline elapsed".to_owned(),
    }
}

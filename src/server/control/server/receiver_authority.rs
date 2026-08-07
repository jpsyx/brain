//! Receiver-only authority transitions shared by watchdog and HTTP ingress.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::server::lifecycle::{LeaseAction, LeaseError, ServerDecision};
use crate::workspace::RegistryStore;

use super::ControlServer;

impl ControlServer {
    /// Capture route authority and a filesystem loader without doing IO.
    pub(crate) fn begin_workspace_route(
        &self,
        ingress: crate::server::IngressId,
        now: Instant,
    ) -> Result<
        (
            crate::server::workspace_route::WorkspaceRouteTicket,
            crate::server::workspace_route::VerifiedWorkspaceContextLoader,
        ),
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        let ticket = crate::server::workspace_route::WorkspaceRouteAuthority::begin(
            &self.leases,
            self.generation,
            ingress,
            now,
        )?;
        let loader = crate::server::workspace_route::VerifiedWorkspaceContextLoader::new(
            self.registry_store.clone(),
            self.runtime_home.clone(),
        );
        Ok((ticket, loader))
    }

    pub(crate) fn begin_local_workspace_route(
        &self,
        ingress: crate::server::IngressId,
        capability: crate::server::lifecycle::LeaseId,
        now: Instant,
    ) -> Result<
        (
            crate::server::workspace_route::WorkspaceRouteTicket,
            crate::server::workspace_route::VerifiedWorkspaceContextLoader,
        ),
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        let ticket = crate::server::workspace_route::WorkspaceRouteAuthority::begin_local(
            &self.leases,
            self.generation,
            ingress,
            capability,
            now,
        )?;
        let loader = crate::server::workspace_route::VerifiedWorkspaceContextLoader::new_local(
            self.registry_store.clone(),
            self.runtime_home.clone(),
        );
        Ok((ticket, loader))
    }

    /// Revalidate captured route authority after filesystem loading.
    pub(crate) fn finish_workspace_route(
        &self,
        ticket: &crate::server::workspace_route::WorkspaceRouteTicket,
        context: crate::workspace::WorkspaceContext,
        now: Instant,
    ) -> Result<
        crate::server::workspace_route::ResolvedWorkspaceRoute,
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        crate::server::workspace_route::WorkspaceRouteAuthority::finish(
            &self.leases,
            self.generation,
            ticket,
            now,
        )?;
        Ok(
            crate::server::workspace_route::ResolvedWorkspaceRoute::with_authority(
                context,
                ticket.lease().clone(),
                self.registry_store.clone(),
                ticket.clone(),
            ),
        )
    }

    pub(crate) fn finish_local_workspace_route(
        &self,
        ticket: &crate::server::workspace_route::WorkspaceRouteTicket,
        context: crate::workspace::WorkspaceContext,
        now: Instant,
    ) -> Result<
        crate::server::workspace_route::ResolvedWorkspaceRoute,
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        crate::server::workspace_route::WorkspaceRouteAuthority::finish_local(
            &self.leases,
            self.generation,
            ticket,
            now,
        )?;
        Ok(
            crate::server::workspace_route::ResolvedWorkspaceRoute::with_authority(
                context,
                ticket.lease().clone(),
                self.registry_store.clone(),
                ticket.clone(),
            ),
        )
    }

    /// Revalidate one resolved route immediately before receiver handoff.
    pub(crate) fn revalidate_workspace_route(
        &self,
        route: &crate::server::workspace_route::ResolvedWorkspaceRoute,
        now: Instant,
    ) -> Result<(), crate::server::workspace_route::WorkspaceRouteError> {
        crate::server::workspace_route::WorkspaceRouteAuthority::finish(
            &self.leases,
            self.generation,
            route.authority_ticket()?,
            now,
        )
    }

    pub(crate) fn begin_receiver_admission(
        &mut self,
        route: &crate::server::workspace_route::ResolvedWorkspaceRoute,
        now: Instant,
    ) -> Result<
        Arc<crate::server::receiver::admission::ReceiverAdmission>,
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        self.revalidate_workspace_route(route, now)?;
        let admission = Arc::new(crate::server::receiver::admission::ReceiverAdmission::new(
            route.lease().workspace_id,
            route.lease().lease_id,
        ));
        self.admissions
            .retain(|candidate| candidate.strong_count() > 0);
        self.admissions.push(Arc::downgrade(&admission));
        Ok(admission)
    }

    /// Reap expired authority and revoke its exact in-flight admissions.
    pub(crate) fn expire_shared_until(
        shared: &Arc<Mutex<Self>>,
        now: Instant,
        deadline: Instant,
        clock: &impl Fn() -> Instant,
    ) -> Result<ServerDecision, LeaseError> {
        let (decision, revocations) = {
            let mut server = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let expired = server.leases.expired_lease_ids(now);
            let decision = server.leases.apply(LeaseAction::Expire { now })?;
            let mut revocations = Vec::new();
            for lease_id in expired {
                revocations.extend(server.admissions_for_lease(lease_id));
            }
            drop(server);
            (decision, revocations)
        };
        for admission in revocations {
            admission.revoke_or_wait_until(deadline, clock);
        }
        Ok(decision)
    }

    pub(crate) fn unavailable_receiver_target(
        &self,
        ingress: crate::server::IngressId,
        _now: Instant,
    ) -> Option<(crate::workspace::WorkspaceId, RegistryStore)> {
        self.leases
            .known_workspace(ingress)
            .map(|workspace_id| (workspace_id, self.registry_store.clone()))
    }
}

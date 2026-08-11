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

    /// Capture route authority for a workspace already selected by the address
    /// an inbound message arrived at.
    ///
    /// Provider URLs carry no ingress any more, so the remembered ingress comes
    /// from this process's lease table; resolution from there is identical to
    /// what an ingress-carrying URL used to get.
    pub(crate) fn begin_receiver_route(
        &self,
        workspace_id: crate::workspace::WorkspaceId,
        now: Instant,
    ) -> Result<
        (
            crate::server::workspace_route::WorkspaceRouteTicket,
            crate::server::workspace_route::VerifiedWorkspaceContextLoader,
        ),
        crate::server::workspace_route::WorkspaceRouteError,
    > {
        let ingress = self.receiver_ingress(workspace_id)?;
        self.begin_workspace_route(ingress, now)
    }

    /// The ingress this process remembers for an addressed workspace.
    pub(crate) fn receiver_ingress(
        &self,
        workspace_id: crate::workspace::WorkspaceId,
    ) -> Result<crate::server::IngressId, crate::server::workspace_route::WorkspaceRouteError> {
        self.leases
            .known_workspace_ingress(workspace_id)
            .ok_or_else(|| {
                crate::server::workspace_route::WorkspaceRouteError::new(
                    404,
                    "workspace route not found",
                )
            })
    }

    /// The machine registry this process routes against.
    pub(crate) fn registry_store(&self) -> RegistryStore {
        self.registry_store.clone()
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

    /// The registry capability for replying to an addressed workspace that
    /// cannot accept work right now, or `None` when this process knows no such
    /// workspace.
    pub(crate) fn unavailable_receiver_target(
        &self,
        workspace_id: crate::workspace::WorkspaceId,
    ) -> Option<RegistryStore> {
        self.leases
            .known_workspace_ingress(workspace_id)
            .map(|_| self.registry_store.clone())
    }
}

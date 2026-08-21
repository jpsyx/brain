//! The machine-wide TUI-lifetime HTTP server and receiver admission boundary.
//!
//! One shared daemon per machine, reused across every `brain` invocation and
//! tab. [`lifecycle`] owns election, generation state, leases, and final-TUI
//! shutdown; [`router`] owns the pure method+path to [`router::Route`] mapping.
//!
//! Lease-capability-scoped `GET /local/<lease>/w/<ingress>/habits` renders
//! today's habits page (see [`routes::habits`]); its completion endpoint
//! delegates to brain's native completion machinery.

pub mod control;
pub mod delivery;
pub(super) mod http;
pub(super) mod http_workers;
pub mod lifecycle;
pub(super) mod provider;
pub mod receiver;
pub mod reply;
pub mod router;
pub mod routes;
pub mod security;
pub mod workspace_route;

pub use lifecycle::IngressId;

use anyhow::Result;

/// The URL of a brain-server route on localhost. Pure.
///
/// `url(8787, "/status")` == `"http://127.0.0.1:8787/status"`.
#[must_use]
pub fn url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

/// The selected workspace's habits page URL on the shared local server.
#[must_use]
pub fn habits_url(port: u16, ingress: IngressId, capability: lifecycle::LeaseId) -> String {
    url(port, &format!("/local/{capability}/w/{ingress}/habits"))
}

/// The selected workspace's completion route for the rendered habits page.
#[must_use]
pub fn habits_done_path(ingress: IngressId, capability: lifecycle::LeaseId) -> String {
    format!("/local/{capability}/w/{ingress}/habits/done")
}

/// The selected workspace's skill-session completion route.
#[must_use]
pub fn session_done_path(ingress: IngressId, capability: lifecycle::LeaseId) -> String {
    format!("/local/{capability}/w/{ingress}/session/done")
}

/// Reload the selected workspace's stable portable ingress identity.
///
/// # Errors
///
/// Returns an error when the portable manifest is unavailable or invalid.
pub fn workspace_ingress(workspace: &crate::workspace::WorkspaceContext) -> Result<IngressId> {
    crate::workspace::WorkspaceManifest::load(workspace.root(), env!("CARGO_PKG_VERSION"))
        .map(|manifest| manifest.receiver_ingress_id().into())
        .map_err(Into::into)
}

/// Run the blocking brain-server accept loop.
///
/// Binds `127.0.0.1:port` (`0` lets the OS assign an ephemeral port),
/// publishes the actual bound port to the daemon record, then serves requests
/// forever. Used internally by the spawned daemon (`brain server run`).
///
/// # Errors
/// Returns an error if the address can't be bound or the bound port can't be
/// resolved.
pub fn run(generation: lifecycle::ServerGeneration, port: u16) -> Result<()> {
    lifecycle::run_process(&lifecycle::ServerPaths::default(), generation, port, false)
}

pub fn run_background(generation: lifecycle::ServerGeneration, port: u16) -> Result<()> {
    lifecycle::run_process(&lifecycle::ServerPaths::default(), generation, port, true)
}

mod request;

pub(in crate::server) use request::respond;
#[cfg(test)]
use request::{
    ReceiverFailureLog, provider_http_status, receiver_failure_log,
    resolve_workspace_route_with_loader,
};

#[cfg(test)]
mod tests;

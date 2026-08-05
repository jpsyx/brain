//! The shared local HTTP server and transitional TUI-owned receiver server.
//!
//! One shared daemon per machine, reused across every `brain` invocation and
//! tab. [`lifecycle`] owns election, generation state, leases, and final-TUI
//! shutdown; [`router`] owns the pure method+path to [`router::Route`] mapping.
//!
//! Ingress-scoped `GET /w/<ingress>/habits` renders today's habits page (see
//! [`routes::habits`]); its completion endpoint delegates to brain's native
//! completion machinery.

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

use std::sync::Mutex;

use anyhow::Result;

use self::router::Route;

const LOCAL_ACTION_BODY_LIMIT_BYTES: usize = 16 * 1024;

/// The URL of a brain-server route on localhost. Pure.
///
/// `url(8787, "/status")` == `"http://127.0.0.1:8787/status"`.
#[must_use]
pub fn url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

/// The selected workspace's habits page URL on the shared local server.
#[must_use]
pub fn habits_url(port: u16, ingress: IngressId) -> String {
    url(port, &format!("/w/{ingress}/habits"))
}

/// The selected workspace's completion route for the rendered habits page.
#[must_use]
pub fn habits_done_path(ingress: IngressId) -> String {
    format!("/w/{ingress}/habits/done")
}

/// The selected workspace's daily-triage completion route.
#[must_use]
pub fn triage_done_path(ingress: IngressId) -> String {
    format!("/w/{ingress}/triage/done")
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
    lifecycle::run_process(&lifecycle::ServerPaths::default(), generation, port)
}

/// Build the response for a single request. The routing decision itself is the
/// pure [`router::route`]; the handlers ([`routes::habits`]) own the HTML/JSON.
pub(in crate::server) fn respond(
    request: &mut http::Request,
    control: &Mutex<control::ControlServer>,
    now: std::time::Instant,
) -> http::Response {
    let route = router::route(request.method(), request.url());
    match route {
        Route::HabitsPage { ingress } => match resolve_workspace_route(control, ingress, now) {
            Ok(workspace) => {
                http::Response::html(200, routes::habits::page(workspace.context(), ingress))
            }
            Err(error) => http::Response::text(error.status(), error.to_string()),
        },
        Route::HabitsDone { ingress } => match resolve_workspace_route(control, ingress, now) {
            Ok(workspace) => match read_local_action_body(request) {
                Ok(body) => {
                    let (status, json) =
                        routes::habits::done(workspace.context(), &body).response();
                    http::Response::json(status, json)
                }
                Err(error) => error.response(),
            },
            Err(error) => workspace_route_error_response(&error),
        },
        Route::TriageDone { ingress } => match resolve_workspace_route(control, ingress, now) {
            Ok(workspace) => match read_local_action_body(request) {
                Ok(body) => {
                    let (status, json) = routes::triage::done(workspace.context(), &body);
                    http::Response::json(status, json)
                }
                Err(error) => error.response(),
            },
            Err(error) => workspace_route_error_response(&error),
        },
        Route::Sms { ingress } | Route::Email { ingress } => {
            match resolve_workspace_route(control, ingress, now) {
                Ok(_) => http::Response::text(503, "receiver forwarding is not available yet"),
                Err(error) => http::Response::text(error.status(), error.to_string()),
            }
        }
        Route::NotFound => http::Response::empty(404),
    }
}

fn resolve_workspace_route(
    control: &Mutex<control::ControlServer>,
    ingress: IngressId,
    now: std::time::Instant,
) -> Result<workspace_route::ResolvedWorkspaceRoute, workspace_route::WorkspaceRouteError> {
    let (ticket, loader) = {
        let mut server = control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let route = server.begin_workspace_route(ingress, now)?;
        drop(server);
        route
    };
    resolve_workspace_route_ticket(control, &ticket, std::time::Instant::now, &loader)
}

#[cfg(test)]
pub(super) fn resolve_workspace_route_with_loader(
    control: &Mutex<control::ControlServer>,
    ingress: IngressId,
    now: impl Fn() -> std::time::Instant,
    loader: &impl workspace_route::WorkspaceContextLoader,
) -> Result<workspace_route::ResolvedWorkspaceRoute, workspace_route::WorkspaceRouteError> {
    let ticket = control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .begin_workspace_route(ingress, now())?
        .0;
    resolve_workspace_route_ticket(control, &ticket, now, loader)
}

fn resolve_workspace_route_ticket(
    control: &Mutex<control::ControlServer>,
    ticket: &workspace_route::WorkspaceRouteTicket,
    now: impl Fn() -> std::time::Instant,
    loader: &impl workspace_route::WorkspaceContextLoader,
) -> Result<workspace_route::ResolvedWorkspaceRoute, workspace_route::WorkspaceRouteError> {
    let context = loader.load(ticket.lease())?;
    control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish_workspace_route(ticket, context, now())
}

fn read_local_action_body(request: &mut http::Request) -> Result<String, LocalActionBodyError> {
    let bytes = request
        .read_body(LOCAL_ACTION_BODY_LIMIT_BYTES)
        .map_err(|error| match error {
            http::BodyError::TooLarge => LocalActionBodyError::TooLarge,
            http::BodyError::Io(error) => {
                crate::logging::log(format!("shared-server HTTP body read failed: {error}"));
                LocalActionBodyError::Unreadable
            }
            http::BodyError::Malformed => LocalActionBodyError::Unreadable,
        })?;
    String::from_utf8(bytes).map_err(|_| LocalActionBodyError::Unreadable)
}

fn workspace_route_error_response(error: &workspace_route::WorkspaceRouteError) -> http::Response {
    http::Response::text(error.status(), error.to_string())
}

enum LocalActionBodyError {
    TooLarge,
    Unreadable,
}

impl LocalActionBodyError {
    fn response(self) -> http::Response {
        let (status, message) = match self {
            Self::TooLarge => (413, "request body is too large"),
            Self::Unreadable => (400, "request body is invalid"),
        };
        http::Response::text(status, message)
    }
}

#[cfg(test)]
mod tests {
    use super::{habits_done_path, habits_url, triage_done_path, url};

    const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

    #[test]
    fn url_builds_a_localhost_route() {
        assert_eq!(url(8787, "/habits"), "http://127.0.0.1:8787/habits");
    }

    #[test]
    fn url_always_includes_the_path() {
        assert!(url(8787, "/habits").ends_with("/habits"));
        assert!(url(1, "/habits").contains("/habits"));
    }

    #[test]
    fn workspace_urls_carry_the_stable_opaque_ingress() {
        let ingress = crate::server::IngressId::parse(FAMILY_ID).unwrap();

        assert_eq!(
            habits_url(8787, ingress),
            format!("http://127.0.0.1:8787/w/{FAMILY_ID}/habits")
        );
        assert_eq!(
            habits_done_path(ingress),
            format!("/w/{FAMILY_ID}/habits/done")
        );
        assert_eq!(
            triage_done_path(ingress),
            format!("/w/{FAMILY_ID}/triage/done")
        );
    }
}

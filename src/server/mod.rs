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

/// Build the response for a single request. The routing decision itself is the
/// pure [`router::route`]; the handlers ([`routes::habits`]) own the HTML/JSON.
pub(in crate::server) fn respond(
    request: &mut http::Request,
    control: &Mutex<control::ControlServer>,
    now: std::time::Instant,
) -> http::Response {
    let route = router::route(request.method(), request.url());
    match route {
        Route::HabitsPage {
            ingress,
            capability,
        } => match resolve_local_workspace_route(control, ingress, capability, now) {
            Ok(workspace) => http::Response::html(
                200,
                routes::habits::page(workspace.context(), ingress, capability),
            ),
            Err(error) => http::Response::text(error.status(), error.to_string()),
        },
        Route::HabitsDone {
            ingress,
            capability,
        } => match resolve_local_workspace_route(control, ingress, capability, now) {
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
        Route::SkillSessionDone {
            ingress,
            capability,
        } => match resolve_local_workspace_route(control, ingress, capability, now) {
            Ok(workspace) => match read_local_action_body(request) {
                Ok(body) => {
                    let (status, json) = routes::session::done(workspace.context(), &body);
                    http::Response::json(status, json)
                }
                Err(error) => error.response(),
            },
            Err(error) => workspace_route_error_response(&error),
        },
        Route::Sms => receiver_response(control, now, request, receiver::Channel::Sms),
        Route::Email => receiver_response(control, now, request, receiver::Channel::Email),
        Route::NotFound => http::Response::empty(404),
    }
}

fn resolve_local_workspace_route(
    control: &Mutex<control::ControlServer>,
    ingress: IngressId,
    capability: lifecycle::LeaseId,
    now: std::time::Instant,
) -> Result<workspace_route::ResolvedWorkspaceRoute, workspace_route::WorkspaceRouteError> {
    let (ticket, loader) = {
        let server = control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let route = server.begin_local_workspace_route(ingress, capability, now)?;
        drop(server);
        route
    };
    resolve_local_workspace_route_ticket(control, &ticket, std::time::Instant::now, &loader)
}

fn receiver_response(
    control: &Mutex<control::ControlServer>,
    now: std::time::Instant,
    request: &mut http::Request,
    channel: receiver::Channel,
) -> http::Response {
    // The body must be read before anything is routed: with one machine-wide URL
    // per channel, the destination inside the payload is what selects the
    // workspace. A body can only be consumed once, so it is carried from here.
    let body = match receiver::http::read_webhook_body(request) {
        Ok(body) => body,
        Err(error) => {
            crate::logging::log(format!(
                "receiver request body rejected channel={channel:?} status={} error={error}",
                error.status()
            ));
            // Same provider-facing status translation an authentication failure
            // gets: Resend must not be told to retry a body it will resend
            // identically.
            return http::Response::text(
                provider_http_status(error.status(), error.unavailable(), channel),
                error.to_string(),
            );
        }
    };
    let workspace_id = match addressed_workspace(control, channel, &body) {
        Ok(workspace_id) => workspace_id,
        Err(error) => {
            crate::logging::log(format!(
                "receiver request unrouted channel={channel:?} status={} error={error}",
                error.status()
            ));
            // An address nobody serves is answered like any unknown URL: an
            // empty 404 that confirms nothing to whoever probed it.
            if error.status() == 404 {
                return http::Response::empty(404);
            }
            return http::Response::text(error.status(), error.to_string());
        }
    };
    let route = match resolve_receiver_route(control, workspace_id, now) {
        Ok(route) => route,
        Err(error) if error.status() == 404 => return http::Response::empty(404),
        Err(error) => {
            crate::logging::log(format!(
                "receiver request unavailable channel={channel:?} status={} error={error}",
                error.status()
            ));
            if channel == receiver::Channel::Email {
                return unavailable_email_response(control, workspace_id, request, &body);
            }
            return unavailable_receiver_response(channel);
        }
    };
    match receiver::dispatch::dispatch_http(route, request, &body, control, channel) {
        Ok(job) => {
            crate::logging::log(format!(
                "receiver job accepted workspace={} job={} channel={:?}",
                job.workspace_id, job.job_id, job.channel
            ));
            match channel {
                receiver::Channel::Sms => http::Response::xml(
                    200,
                    "<Response><Message>Received. I’ll get back to you shortly.</Message></Response>",
                ),
                receiver::Channel::Email => {
                    http::Response::json(200, r#"{"ok":true,"queued":true}"#)
                }
            }
        }
        Err(error) => match receiver_failure_log(error.status(), error.unavailable()) {
            ReceiverFailureLog::Unavailable => {
                crate::logging::log(format!(
                    "receiver request unavailable channel={channel:?} status={} error={error}",
                    error.status()
                ));
                unavailable_receiver_response(channel)
            }
            ReceiverFailureLog::AcceptedWithoutEnqueue => {
                crate::logging::log(format!(
                    "receiver event accepted without enqueue channel={channel:?} status={}",
                    error.status()
                ));
                http::Response::text(
                    provider_http_status(error.status(), error.unavailable(), channel),
                    error.to_string(),
                )
            }
            ReceiverFailureLog::Rejected => {
                crate::logging::log(format!(
                    "receiver request rejected channel={channel:?} status={} error={error}",
                    error.status()
                ));
                http::Response::text(
                    provider_http_status(error.status(), error.unavailable(), channel),
                    error.to_string(),
                )
            }
        },
    }
}

/// The workspace an inbound provider payload was addressed to.
///
/// Reads the machine registry outside the control mutex: routing needs every
/// registered workspace's published address, and no lease state is involved.
fn addressed_workspace(
    control: &Mutex<control::ControlServer>,
    channel: receiver::Channel,
    body: &[u8],
) -> Result<crate::workspace::WorkspaceId, workspace_route::WorkspaceRouteError> {
    let store = control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .registry_store();
    let registry = crate::workspace::RegistryStore::load_from(store.path()).map_err(|error| {
        workspace_route::WorkspaceRouteError::new(
            500,
            format!("workspace registry unavailable: {error}"),
        )
    })?;
    let destinations = receiver::http::destinations(channel, body);
    match receiver::routing::select_workspace(&registry, channel, &destinations) {
        receiver::routing::ReceiverRoute::Workspace(workspace_id) => Ok(workspace_id),
        // Two workspaces publishing one address is a fixable misconfiguration,
        // so the provider is told to come back rather than that nothing exists.
        receiver::routing::ReceiverRoute::Ambiguous => {
            Err(workspace_route::WorkspaceRouteError::new(
                503,
                "several workspaces publish this receiver address",
            ))
        }
        // The provider is answered with an empty 404 either way, so this text
        // exists for the machine owner's log: it is the only place a wrong or
        // stale configured address can be spotted.
        receiver::routing::ReceiverRoute::Unknown => {
            Err(workspace_route::WorkspaceRouteError::new(
                404,
                receiver::routing::unrouted_explanation(&registry, channel, &destinations),
            ))
        }
    }
}

fn resolve_receiver_route(
    control: &Mutex<control::ControlServer>,
    workspace_id: crate::workspace::WorkspaceId,
    now: std::time::Instant,
) -> Result<workspace_route::ResolvedWorkspaceRoute, workspace_route::WorkspaceRouteError> {
    let (ticket, loader) = {
        let server = control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let route = server.begin_receiver_route(workspace_id, now)?;
        drop(server);
        route
    };
    resolve_workspace_route_ticket(control, &ticket, std::time::Instant::now, &loader)
}

fn unavailable_email_response(
    control: &Mutex<control::ControlServer>,
    workspace_id: crate::workspace::WorkspaceId,
    request: &http::Request,
    body: &[u8],
) -> http::Response {
    let target = control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .unavailable_receiver_target(workspace_id);
    let Some(store) = target else {
        return http::Response::empty(503);
    };
    let config = match receiver::http::ProviderConfig::load_for_workspace(&store, workspace_id) {
        Ok(config) => config,
        Err(error) => return http::Response::text(503, error.to_string()),
    };
    match receiver::http::verify_unavailable_email(request, body, &config) {
        Ok(provider_id) => {
            receiver::dispatch::remember_verified_unavailable_email(workspace_id, provider_id);
            unavailable_receiver_response(receiver::Channel::Email)
        }
        Err(error) => http::Response::text(
            provider_http_status(error.status(), false, receiver::Channel::Email),
            error.to_string(),
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiverFailureLog {
    Unavailable,
    AcceptedWithoutEnqueue,
    Rejected,
}

const fn receiver_failure_log(status: u16, unavailable: bool) -> ReceiverFailureLog {
    if unavailable {
        ReceiverFailureLog::Unavailable
    } else if status == 202 {
        ReceiverFailureLog::AcceptedWithoutEnqueue
    } else {
        ReceiverFailureLog::Rejected
    }
}

fn unavailable_receiver_response(channel: receiver::Channel) -> http::Response {
    let message = receiver::unavailable_message();
    match channel {
        receiver::Channel::Sms => http::Response::xml(
            200,
            format!("<Response><Message>{message}</Message></Response>"),
        ),
        receiver::Channel::Email => http::Response::json(
            200,
            serde_json::json!({"ok": false, "error": message}).to_string(),
        ),
    }
}

const fn provider_http_status(status: u16, unavailable: bool, channel: receiver::Channel) -> u16 {
    if matches!(channel, receiver::Channel::Email)
        && (unavailable || status == 202 || (status >= 400 && status < 500 && status != 401))
    {
        200
    } else {
        status
    }
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

fn resolve_local_workspace_route_ticket(
    control: &Mutex<control::ControlServer>,
    ticket: &workspace_route::WorkspaceRouteTicket,
    now: impl Fn() -> std::time::Instant,
    loader: &impl workspace_route::WorkspaceContextLoader,
) -> Result<workspace_route::ResolvedWorkspaceRoute, workspace_route::WorkspaceRouteError> {
    let context = loader.load(ticket.lease())?;
    control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish_local_workspace_route(ticket, context, now())
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
mod tests;

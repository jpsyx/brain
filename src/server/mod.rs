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
use tiny_http::{Header, Request, Response};

use self::router::Route;

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
pub(super) fn respond(
    request: &mut Request,
    control: &mut control::ControlServer,
    now: std::time::Instant,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let route = router::route(request.method().as_str(), request.url());
    match route {
        Route::HabitsPage { ingress } => match control.resolve_workspace_route(ingress, now) {
            Ok(workspace) => {
                Response::from_string(routes::habits::page(workspace.context(), ingress))
                    .with_header(content_type("text/html; charset=utf-8"))
            }
            Err(error) => Response::from_string(error.to_string())
                .with_status_code(error.status())
                .with_header(content_type("text/plain; charset=utf-8")),
        },
        Route::HabitsDone { ingress } => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            match control.resolve_workspace_route(ingress, now) {
                Ok(workspace) => {
                    let (status, json) =
                        routes::habits::done(workspace.context(), &body).response();
                    Response::from_string(json)
                        .with_status_code(status)
                        .with_header(content_type("application/json"))
                }
                Err(error) => Response::from_string(error.to_string())
                    .with_status_code(error.status())
                    .with_header(content_type("text/plain; charset=utf-8")),
            }
        }
        Route::TriageDone { ingress } => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            match control.resolve_workspace_route(ingress, now) {
                Ok(workspace) => {
                    let (status, json) = routes::triage::done(workspace.context(), &body);
                    Response::from_string(json)
                        .with_status_code(status)
                        .with_header(content_type("application/json"))
                }
                Err(error) => Response::from_string(error.to_string())
                    .with_status_code(error.status())
                    .with_header(content_type("text/plain; charset=utf-8")),
            }
        }
        Route::Sms { ingress } | Route::Email { ingress } => {
            match control.resolve_workspace_route(ingress, now) {
                Ok(_) => Response::from_string("receiver forwarding is not available yet")
                    .with_status_code(503),
                Err(error) => {
                    Response::from_string(error.to_string()).with_status_code(error.status())
                }
            }
        }
        Route::NotFound => Response::from_string(String::new()).with_status_code(404),
    }
}

/// A `Content-Type` header. The value is a compile-time-safe ASCII literal, so
/// [`Header::from_bytes`] cannot fail here.
fn content_type(value: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], value.as_bytes())
        .unwrap_or_else(|()| unreachable!("static content-type header is valid ASCII"))
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

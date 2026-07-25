//! The brain server: a small, sync, localhost HTTP daemon.
//!
//! One shared daemon per machine, reused across every `brain` invocation and
//! tab. [`lifecycle`] owns the on-disk daemon record and the `start` /
//! `status` / `kill` actions; [`router`] owns the pure method+path to
//! [`router::Route`] mapping. [`run`] is the blocking accept loop the detached
//! daemon runs.
//!
//! `GET /habits` renders today's habits page (see [`routes::habits`]) and
//! `POST /habits/done` marks a habit done by delegating to brain's own
//! completion machinery. Everything else, including the bare root `/`, is a
//! 404 (the brain server has no root view).

pub mod lifecycle;
pub mod router;
pub mod routes;

use anyhow::{Context, Result};
use tiny_http::{Header, Request, Response, Server};

use self::router::Route;

/// The URL of a brain-server route on localhost. Pure.
///
/// `url(8787, "/habits")` == `"http://127.0.0.1:8787/habits"`.
#[must_use]
pub fn url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
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
pub fn run(port: u16) -> Result<()> {
    let server = Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow::anyhow!("binding 127.0.0.1:{port}: {e}"))?;
    let actual = server
        .server_addr()
        .to_ip()
        .context("resolving the bound server address")?
        .port();
    lifecycle::write_state(lifecycle::ServerState { pid: std::process::id(), port: actual })?;

    for mut request in server.incoming_requests() {
        let response = respond(&mut request);
        let _ = request.respond(response);
    }
    Ok(())
}

/// Build the response for a single request. The routing decision itself is the
/// pure [`router::route`]; the handlers ([`routes::habits`]) own the HTML/JSON.
fn respond(request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let route = router::route(request.method().as_str(), request.url());
    match route {
        Route::HabitsPage => {
            let root = crate::paths::brain_root_path();
            Response::from_string(routes::habits::page(&root))
                .with_header(content_type("text/html; charset=utf-8"))
        }
        Route::HabitsDone => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let root = crate::paths::brain_root_path();
            let (status, json) = routes::habits::done(&root, &body).response();
            Response::from_string(json)
                .with_status_code(status)
                .with_header(content_type("application/json"))
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
    use super::url;

    #[test]
    fn url_builds_a_localhost_route() {
        assert_eq!(url(8787, "/habits"), "http://127.0.0.1:8787/habits");
    }

    #[test]
    fn url_always_includes_the_path() {
        assert!(url(8787, "/habits").ends_with("/habits"));
        assert!(url(1, "/habits").contains("/habits"));
    }
}

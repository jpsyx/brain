//! The brain server: a small, sync, localhost HTTP daemon.
//!
//! One shared daemon per machine, reused across every `brain` invocation and
//! tab. [`lifecycle`] owns the on-disk daemon record and the `start` /
//! `status` / `kill` actions; [`router`] owns the pure method+path to
//! [`router::Route`] mapping. [`run`] is the blocking accept loop the detached
//! daemon runs.
//!
//! Routes are placeholders for now: `GET /habits` renders a stub page and
//! `POST /habits/done` accepts an empty JSON body. Real habits rendering
//! lands in a later task. Everything else, including the bare root `/`, is a
//! 404 (the brain server has no root view).

pub mod lifecycle;
pub mod router;

use anyhow::{Context, Result};
use tiny_http::{Header, Response, Server};

use self::router::Route;

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

    for request in server.incoming_requests() {
        let response = respond(request.method().as_str(), request.url());
        let _ = request.respond(response);
    }
    Ok(())
}

/// Build the response for a single request, given its method and URL. The
/// routing decision itself is the pure [`router::route`].
fn respond(method: &str, url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    match router::route(method, url) {
        Route::HabitsPage => Response::from_string("habits view (coming soon)")
            .with_header(content_type("text/plain; charset=utf-8")),
        Route::HabitsDone => {
            Response::from_string("{}").with_header(content_type("application/json"))
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

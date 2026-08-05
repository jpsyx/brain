use std::io::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::{super::BodyError, super::Request, super::RequestError, tcp_pair};
use crate::server::http::Response;
use crate::server::http::deadline::ConnectionClock;

#[test]
fn drip_fed_request_head_cannot_outlive_one_absolute_injected_deadline() {
    let (mut client, server) = tcp_pair();
    let clock = Arc::new(ManualClock::new());
    let (result_tx, result_rx) = mpsc::sync_channel(0);
    let request_clock = Arc::clone(&clock);
    std::thread::spawn(move || {
        result_tx
            .send(Request::read_with_clock(
                server,
                request_clock,
                Duration::from_secs(2),
            ))
            .expect("report request parse");
    });

    clock.wait_for_calls(2);
    client.write_all(b"G").expect("write first head byte");
    clock.wait_for_calls(3);
    clock.advance(Duration::from_secs(2));
    client
        .write_all(b"ET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("write remaining head after the absolute deadline");

    let result = result_rx
        .recv_timeout(Duration::from_millis(500))
        .expect("absolute deadline must finish the parser");
    let Err(error) = result else {
        panic!("successful drip progress must not reset the deadline");
    };
    assert!(
        matches!(error, RequestError::Io(error) if error.kind() == std::io::ErrorKind::TimedOut)
    );
}

#[test]
fn drip_fed_body_and_response_share_the_request_head_deadline() {
    let (mut client, server) = tcp_pair();
    client
        .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n")
        .expect("write request head");
    let clock = Arc::new(ManualClock::new());
    let mut request = Request::read_with_clock(server, clock.clone(), Duration::from_secs(2))
        .expect("parse request head");
    let base_calls = clock.calls();
    let (result_tx, result_rx) = mpsc::sync_channel(0);
    let body = std::thread::spawn(move || {
        result_tx
            .send(request.read_body(16))
            .expect("report body read");
        request
    });

    clock.wait_for_calls(base_calls + 1);
    client.write_all(b"a").expect("write first body byte");
    clock.wait_for_calls(base_calls + 3);
    clock.advance(Duration::from_secs(2));
    client
        .write_all(b"b")
        .expect("write remaining body after deadline");
    let error = result_rx
        .recv_timeout(Duration::from_millis(500))
        .expect("absolute body deadline must finish")
        .expect_err("body drip must not reset the connection deadline");
    assert!(matches!(error, BodyError::Io(error) if error.kind() == std::io::ErrorKind::TimedOut));

    let mut request = body.join().expect("body reader");
    let error = request
        .write_response(&Response::text(200, "late"))
        .expect_err("response must share the already-expired connection deadline");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
}

#[test]
fn bounded_handler_phase_replaces_the_parse_deadline_and_reserves_handoff_response_time() {
    let (mut client, server) = tcp_pair();
    client
        .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n")
        .expect("write request head");
    let clock = Arc::new(ManualClock::new());
    let mut request = Request::read_with_clock(server, clock.clone(), Duration::from_secs(2))
        .expect("parse request head");
    clock.advance(Duration::from_secs(2));

    request
        .begin_handler_phase()
        .expect("start bounded handler phase");
    client
        .write_all(b"ok")
        .expect("write body in handler phase");
    assert_eq!(request.read_body(16).unwrap(), b"ok");

    clock.advance(Duration::from_secs(26));
    request
        .ensure_acceptance_budget()
        .expect_err("enqueue must stop when handoff plus response no longer fit");
}

struct ManualClock {
    now: Mutex<Instant>,
    calls: AtomicUsize,
}

impl ManualClock {
    fn new() -> Self {
        Self {
            now: Mutex::new(Instant::now()),
            calls: AtomicUsize::new(0),
        }
    }

    fn advance(&self, duration: Duration) {
        let mut now = self.now.lock().expect("clock lock");
        *now += duration;
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Acquire)
    }

    fn wait_for_calls(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.calls() < expected && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            self.calls() >= expected,
            "clock was not polled {expected} times"
        );
    }
}

impl ConnectionClock for ManualClock {
    fn now(&self) -> Instant {
        self.calls.fetch_add(1, Ordering::AcqRel);
        *self.now.lock().expect("clock lock")
    }
}

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
fn handler_phase_cannot_revive_an_expired_parse_deadline() {
    let (mut client, server) = tcp_pair();
    client
        .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n")
        .expect("write request head");
    let clock = Arc::new(ManualClock::new());
    let mut request = Request::read_with_clock(server, clock.clone(), Duration::from_secs(2))
        .expect("parse request head");
    clock.advance(Duration::from_secs(2));

    let error = request
        .begin_handler_phase()
        .expect_err("expired parse work must not enter the handler phase");

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
}

#[test]
fn live_handler_limits_handoff_to_two_seconds_and_keeps_response_budget_separate() {
    let (mut client, server) = tcp_pair();
    client
        .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n")
        .expect("write request");
    let clock = Arc::new(ManualClock::new());
    let mut request = Request::read_with_clock(server, clock.clone(), Duration::from_secs(2))
        .expect("parse request head");
    request
        .begin_handler_phase()
        .expect("start bounded handler phase");

    let handoff = request
        .job_handoff_deadline()
        .expect("derive one bounded handoff deadline");
    clock.advance(Duration::from_secs(2));

    let error = handoff
        .ensure_open()
        .expect_err("the short handoff deadline must expire without renewal");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    request
        .write_response(&Response::text(200, "still reserved"))
        .expect("response reserve remains open after handoff cutoff");
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

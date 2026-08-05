use std::io::{ErrorKind, Read as _, Write as _};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use brain::server::control::{ControlRequest, ControlResponse};

use super::support::*;

const SHARED_HTTP_CONNECTION_LIMIT: usize = 4;

#[test]
fn shared_http_admission_stays_at_four_while_control_remains_responsive() {
    let server = ServerFixture::new(FAMILY_ID);
    let path = format!(
        "/local/{}/w/{}/habits/done",
        server.family_lease, server.family_ingress
    );
    let mut held = Vec::with_capacity(SHARED_HTTP_CONNECTION_LIMIT);

    for _ in 0..SHARED_HTTP_CONNECTION_LIMIT {
        held.push(accepted_partial_body(server.port, &path));
    }

    let mut overflow = complete_get(
        server.port,
        &format!(
            "/local/{}/w/{}/habits",
            server.personal_lease, server.personal_ingress
        ),
    );
    let mut first = [0_u8; 1];
    let error = overflow
        .read(&mut first)
        .expect_err("the fifth connection must wait behind four admitted connections");
    assert!(matches!(
        error.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut
    ));

    assert!(matches!(
        server
            .client
            .request_with_timeout(&ControlRequest::Snapshot, Duration::from_millis(500))
            .expect("HTTP saturation must not block the control plane"),
        ControlResponse::Snapshot(_)
    ));

    drop(held.pop());
    overflow
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("bound released response read");
    let mut response = Vec::new();
    overflow
        .read_to_end(&mut response)
        .expect("the waiting request must complete after one slot is released");
    assert!(response.starts_with(b"HTTP/1.1 200"), "{response:?}");
}

#[test]
fn incomplete_headers_cannot_grow_the_shared_process_thread_set() {
    let server = ServerFixture::new(FAMILY_ID);
    let baseline = process_thread_count(server.pid());
    let mut held = Vec::new();
    for _ in 0..24 {
        let mut stream = TcpStream::connect(("127.0.0.1", server.port))
            .expect("connect incomplete request head");
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost")
            .expect("write incomplete request head");
        held.push(stream);
    }

    let deadline = Instant::now() + Duration::from_millis(500);
    let observed = loop {
        let count = process_thread_count(server.pid());
        if count > baseline || Instant::now() >= deadline {
            break count;
        }
        std::thread::park_timeout(Duration::from_millis(1));
    };

    assert_eq!(
        observed, baseline,
        "connection admission must remain within the already-started fixed workers"
    );
    assert!(matches!(
        server
            .client
            .request_with_timeout(&ControlRequest::Snapshot, Duration::from_millis(500))
            .expect("incomplete headers must not block the control plane"),
        ControlResponse::Snapshot(_)
    ));
    assert_eq!(held.len(), 24, "all incomplete clients remain connected");
}

fn accepted_partial_body(port: u16, path: &str) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect partial request");
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("bound continue response");
    stream
        .write_all(
            format!(
                "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 32\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .expect("write partial request head");
    let response = read_response_head(&mut stream);
    assert!(
        response.starts_with(b"HTTP/1.1 100 Continue\r\n"),
        "{response:?}"
    );
    stream
}

fn read_response_head(stream: &mut TcpStream) -> Vec<u8> {
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        let mut byte = [0_u8; 1];
        stream
            .read_exact(&mut byte)
            .expect("read interim response head");
        response.push(byte[0]);
        assert!(response.len() <= 4096, "interim response head is bounded");
    }
    response
}

#[cfg(target_os = "macos")]
fn process_thread_count(pid: u32) -> usize {
    let output = std::process::Command::new("ps")
        .args(["-M", "-p", &pid.to_string()])
        .output()
        .expect("inspect process threads");
    assert!(output.status.success(), "ps failed: {output:?}");
    String::from_utf8(output.stdout)
        .expect("ps output is utf8")
        .lines()
        .skip(1)
        .count()
}

#[cfg(target_os = "linux")]
fn process_thread_count(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{pid}/task"))
        .expect("inspect process threads")
        .count()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_thread_count(_pid: u32) -> usize {
    0
}

fn complete_get(port: u16, path: &str) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect overflow request");
    stream
        .set_read_timeout(Some(Duration::from_millis(150)))
        .expect("bound saturated response read");
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .expect("write overflow request");
    stream
}

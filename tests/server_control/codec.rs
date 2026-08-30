use super::support::generation;
use brain::server::control::{ControlRequest, ControlResponse, LeaseRegistration};
use std::io::Write as _;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn register_request_round_trips_as_newline_delimited_json() {
    let request = ControlRequest::Register(LeaseRegistration {
        generation: generation(),
        lease_id: super::support::lease_id(),
        workspace_id: super::support::workspace_id(),
        canonical_name: "personal".to_owned(),
        ingress_id: super::support::ingress_id(),
        tui_pid: 101,
        resolved_root: PathBuf::from("/tmp/brain-test/workspace"),
    });

    let encoded = brain::server::control::codec::encode(&request).expect("encode request");
    assert_eq!(encoded.last(), Some(&b'\n'));
    assert!(String::from_utf8_lossy(&encoded).contains("resolved_root"));

    let decoded: ControlRequest =
        brain::server::control::codec::decode(&encoded).expect("decode request");
    assert_eq!(decoded, request);
}

#[test]
fn codec_rejects_malformed_and_oversized_frames() {
    assert!(brain::server::control::codec::decode::<ControlRequest>(b"not-json\n").is_err());
    assert!(brain::server::control::codec::decode::<ControlRequest>(b"{}{}").is_err());
    assert!(brain::server::control::codec::decode::<ControlRequest>(b"{}\n{}\n").is_err());
    assert!(
        brain::server::control::codec::decode::<ControlRequest>(&vec![
            b'x';
            brain::server::control::codec::MAX_FRAME_BYTES
                + 1
        ])
        .is_err()
    );
}

#[test]
fn codec_read_rejects_multiple_frames_from_a_real_stream() {
    let (mut reader, mut writer) = UnixStream::pair().expect("Unix stream pair");
    let first =
        brain::server::control::codec::encode(&ControlRequest::Snapshot).expect("first frame");
    let second =
        brain::server::control::codec::encode(&ControlRequest::Snapshot).expect("second frame");
    writer.write_all(&first).expect("write first frame");
    writer.write_all(&second).expect("write second frame");
    writer.shutdown(Shutdown::Write).expect("finish request");

    let error = brain::server::control::codec::read::<ControlRequest>(&mut reader)
        .expect_err("two frames must be rejected");

    assert!(
        error.to_string().contains("more than one frame"),
        "{error:#}"
    );
}

#[test]
fn deadline_read_succeeds_after_the_client_half_closes_its_write_side() {
    let (mut reader, mut writer) = UnixStream::pair().expect("Unix stream pair");
    let response = ControlResponse::Snapshot(brain::server::control::ServerSnapshot {
        protocol_version: brain::server::control::CONTROL_PROTOCOL_VERSION,
        generation: generation(),
        live_leases: 0,
    });
    writer
        .write_all(&brain::server::control::codec::encode(&response).expect("response frame"))
        .expect("write response");
    writer.shutdown(Shutdown::Write).expect("finish response");
    reader.shutdown(Shutdown::Write).expect("finish request");

    let decoded = brain::server::control::codec::read_until::<ControlResponse>(
        &mut reader,
        Instant::now() + Duration::from_secs(1),
    )
    .expect("read response after request EOF");

    assert_eq!(decoded, response);
}

#[test]
fn snapshot_without_the_current_protocol_version_is_rejected() {
    let legacy = format!(
        "{{\"result\":\"snapshot\",\"generation\":\"{}\",\"live_leases\":1}}\n",
        generation()
    );

    let error = brain::server::control::codec::decode::<ControlResponse>(legacy.as_bytes())
        .expect_err("a legacy server response must not cross the protocol fence");

    assert!(error.to_string().contains("decoding server control frame"));
}

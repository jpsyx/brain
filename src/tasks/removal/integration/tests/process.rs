use super::request;
use crate::tasks::removal::integration::{process::invoke_with_environment, protocol::ErrorKind};
use crate::tasks::removal::model::AdapterSnapshot;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

fn script(body: &str) -> (tempfile::TempDir, AdapterSnapshot) {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("adapter ; $(touch NEVER)");
    std::fs::write(&executable, format!("#!/usr/bin/python3\n{body}\n")).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    (
        dir,
        AdapterSnapshot {
            argv: vec![executable.to_string_lossy().into_owned()],
            environment_names: std::collections::BTreeSet::default(),
            timeout_ms: 30_000,
        },
    )
}

const REPLY: &str = r"import sys,json
r=json.load(sys.stdin)
print(json.dumps(dict(schema_version=1,operation_id=r['operation_id'],action_id=r['action_id'],status='canceled',message=r['issue'])))";

#[test]
fn removal_integration_process_success_preserves_metacharacters_as_data() {
    let (_dir, mut adapter) = script(REPLY);
    adapter.argv.push("$(touch NEVER); ' quote".into());
    let mut request = request();
    request.issue = "$(touch NEVER); ' quote".into();
    let response = invoke_with_environment(&adapter, &request, |_| None).unwrap();
    assert_eq!(response.status, "canceled");
    assert_eq!(response.message, Some(request.issue));
}

#[test]
fn removal_integration_process_rejects_malformed_wrong_identity_and_nonzero_exit() {
    for (body, expected) in [
        (
            "import sys\nsys.stdin.read()\nprint('not JSON')".to_owned(),
            ErrorKind::Protocol,
        ),
        (
            REPLY.replace("r['action_id']", "'wrong'"),
            ErrorKind::Protocol,
        ),
        (format!("{REPLY}\nsys.exit(2)"), ErrorKind::Exit),
    ] {
        let (_dir, adapter) = script(&body);
        let mut payload = request();
        payload.issue = "x".repeat(60_000);
        assert_eq!(
            invoke_with_environment(&adapter, &payload, |_| None)
                .unwrap_err()
                .kind,
            expected
        );
    }
}

#[test]
fn removal_integration_process_times_out_blocked_stdin_and_descendant_pipes() {
    for body in [
        "import time\ntime.sleep(20)",
        "import os,time\nif os.fork()==0: time.sleep(20)\nelse: os._exit(0)",
    ] {
        let (_dir, mut adapter) = script(body);
        adapter.timeout_ms = 150;
        let mut request = request();
        request.issue = "x".repeat(60_000);
        let start = Instant::now();
        assert_eq!(
            invoke_with_environment(&adapter, &request, |_| None)
                .unwrap_err()
                .kind,
            ErrorKind::Timeout
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn removal_integration_process_output_bounds_are_explicit_for_both_streams() {
    for (stream, kind) in [
        ("stdout", ErrorKind::ResponseTooLarge),
        ("stderr", ErrorKind::StderrTooLarge),
    ] {
        let (_dir, adapter) = script(&format!(
            "import sys,time\nsys.{stream}.write('s'*100_000)\nsys.{stream}.flush()\ntime.sleep(20)"
        ));
        let error = invoke_with_environment(&adapter, &request(), |_| None).unwrap_err();
        assert_eq!(error.kind, kind);
        assert!(error.stderr.is_empty());
    }
    let (_dir, adapter) = script(REPLY);
    let mut request = request();
    request.issue = "x".repeat(65_536);
    assert_eq!(
        invoke_with_environment(&adapter, &request, |_| None)
            .unwrap_err()
            .kind,
        ErrorKind::RequestTooLarge
    );
}

#[test]
fn removal_integration_process_clears_environment_and_redacts_stderr_and_message() {
    let body = r"import sys,json,os
r=json.load(sys.stdin)
assert 'HOME' not in os.environ
assert 'NOT_ALLOWED' not in os.environ
assert 'ABSENT' not in os.environ
secret=os.environ['TOKEN']
sys.stderr.write('diagnostic '+secret)
print(json.dumps(dict(schema_version=1,operation_id=r['operation_id'],action_id=r['action_id'],status='canceled',message='message '+secret)))";
    let (_dir, mut adapter) = script(body);
    adapter.environment_names = ["TOKEN".into(), "ABSENT".into()].into();
    let mut names = Vec::new();
    let response = invoke_with_environment(&adapter, &request(), |name| {
        names.push(name.to_owned());
        (name == "TOKEN").then(|| "synthetic-secret".into())
    })
    .unwrap();
    assert_eq!(names, ["ABSENT", "TOKEN"]);
    assert_eq!(response.message.as_deref(), Some("message [REDACTED]"));
    let (_dir, mut adapter) = script(&format!("{body}\nsys.exit(2)"));
    adapter.environment_names.insert("TOKEN".into());
    let error = invoke_with_environment(&adapter, &request(), |_| Some("synthetic-secret".into()))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Exit);
    assert_eq!(error.stderr, "diagnostic [REDACTED]");
    assert!(!format!("{error:?}").contains("synthetic-secret"));
}

#[test]
fn removal_integration_process_protocol_failure_terminates_descendants_after_leader_exit() {
    let (dir, mut adapter) = script(
        r"import os,sys,time
marker=sys.argv[1]
pid=os.fork()
if pid==0:
    for fd in [0,1,2]: os.close(fd)
    time.sleep(0.4)
    open(marker,'w').write('survived')
    os._exit(0)
sys.stdin.read()
print('invalid JSON')",
    );
    let marker = dir.path().join("survived");
    adapter.argv.push(marker.to_string_lossy().into_owned());
    assert_eq!(
        invoke_with_environment(&adapter, &request(), |_| None)
            .unwrap_err()
            .kind,
        ErrorKind::Protocol
    );
    std::thread::sleep(Duration::from_millis(600));
    assert!(!marker.exists(), "failed adapter left a descendant alive");
}

#[test]
fn removal_integration_process_early_output_eof_does_not_stop_request_delivery() {
    let (dir, mut adapter) = script(
        r"import os,sys,json,time
os.close(1)
os.close(2)
time.sleep(0.1)
r=json.load(sys.stdin)
open(sys.argv[1],'w').write(str(len(r['issue'])))",
    );
    let marker = dir.path().join("read");
    adapter.argv.push(marker.to_string_lossy().into_owned());
    let mut request = request();
    request.issue = "x".repeat(60_000);
    assert_eq!(
        invoke_with_environment(&adapter, &request, |_| None)
            .unwrap_err()
            .kind,
        ErrorKind::Protocol
    );
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "60000");
}

#[test]
fn removal_integration_process_escaped_pipe_holder_cannot_hold_worker_past_deadline() {
    let (_dir, mut adapter) = script(
        r"import os,time
if os.fork()==0:
    os.setsid()
    time.sleep(3)
    os._exit(0)
os._exit(0)",
    );
    adapter.timeout_ms = 150;
    let start = Instant::now();
    assert_eq!(
        invoke_with_environment(&adapter, &request(), |_| None)
            .unwrap_err()
            .kind,
        ErrorKind::Timeout
    );
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn removal_integration_process_non_unicode_secret_and_invalid_response_are_redacted() {
    use std::os::unix::ffi::OsStringExt;
    let (_dir, mut adapter) = script(
        r"import os,sys
sys.stdin.read()
sys.stderr.buffer.write(os.environb[b'TOKEN'])
print('not JSON')",
    );
    adapter.environment_names.insert("TOKEN".into());
    let error = invoke_with_environment(&adapter, &request(), |_| {
        Some(std::ffi::OsString::from_vec(
            b"synthetic\xffsecret".to_vec(),
        ))
    })
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Protocol);
    assert_eq!(error.stderr, "[REDACTED]");
}

#[test]
fn removal_integration_process_redaction_does_not_expand_diagnostics_past_limit() {
    let (_dir, mut adapter) =
        script("import sys\nsys.stdin.read()\nsys.stderr.write('s'*10000)\nsys.exit(2)");
    adapter.environment_names.insert("TOKEN".into());
    let error = invoke_with_environment(&adapter, &request(), |_| Some("s".into())).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Exit);
    assert!(error.stderr.len() <= 65_536);
    assert!(!error.stderr.contains('s'));
}

#[test]
fn removal_integration_process_error_status_also_cleans_up_owned_group() {
    let (dir, mut adapter) = script(
        r"import os,sys,time,json
if os.fork()==0:
    for fd in [0,1,2]: os.close(fd)
    time.sleep(0.4)
    open(sys.argv[1],'w').write('survived')
    os._exit(0)
r=json.load(sys.stdin)
print(json.dumps(dict(schema_version=1,operation_id=r['operation_id'],action_id=r['action_id'],status='error')))",
    );
    let marker = dir.path().join("survived");
    adapter.argv.push(marker.to_string_lossy().into_owned());
    assert_eq!(
        invoke_with_environment(&adapter, &request(), |_| None)
            .unwrap()
            .status,
        "error"
    );
    std::thread::sleep(Duration::from_millis(600));
    assert!(!marker.exists());
}

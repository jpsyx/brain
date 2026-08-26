use super::*;

fn run_bridge_at(snapshot: &Path, payload: &serde_json::Value, now_unix_ms: u64) -> Output {
    let script = r#"
import importlib.util
import os

spec = importlib.util.spec_from_file_location("brain_receiver_bridge", os.environ["BRAIN_TEST_BRIDGE"])
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)
bridge.time.time_ns = lambda: int(os.environ["BRAIN_TEST_NOW_MS"]) * 1_000_000
os.umask(0o077)
bridge.main()
"#;
    let mut command = Command::new("python3");
    command
        .args(["-c", script])
        .env("BRAIN_TEST_BRIDGE", bridge_path())
        .env("BRAIN_TEST_NOW_MS", now_unix_ms.to_string())
        .env("BRAIN_RECEIVER_JOB_TOKEN", JOB_TOKEN)
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", snapshot)
        .env("BRAIN_INSTANCE_ID", INSTANCE_ID)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn clock-controlled bridge");
    child
        .stdin
        .take()
        .expect("bridge stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write bridge payload");
    child.wait_with_output().expect("wait observation bridge")
}

#[test]
fn wall_clock_rollback_clamps_progress_and_completion_to_the_latest_boundary() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "observation.json");
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    assert!(
        run_bridge_at(&path, &accepted_payload(&marker), 2_000)
            .status
            .success()
    );
    assert!(
        run_bridge_at(
            &path,
            &progress_payload(SESSION_ID, "rollback-progress"),
            1_000
        )
        .status
        .success()
    );
    let progressing = snapshot(&path);
    assert_eq!(progressing["progressing_at_unix_ms"], 2_000);

    let completed = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": SESSION_ID,
        "turn_id": "rollback-completed",
    });
    assert!(run_bridge_at(&path, &completed, 500).status.success());
    let completed = snapshot(&path);
    assert_eq!(completed["completed_at_unix_ms"], 2_000);
    assert_eq!(completed["phase"], "completed");
    assert_eq!(completed["revision"], 3);
}

#[test]
fn lock_leaf_symlink_is_rejected_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "observation.json");
    let victim = observation_directory(&temporary).join("victim");
    std::fs::write(&victim, b"untouched").unwrap();
    std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
    symlink(&victim, path.with_extension("json.lock")).unwrap();
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");

    assert!(
        run_bridge(&path, &accepted_payload(&marker))
            .status
            .success()
    );
    assert!(!path.exists());
    assert_eq!(std::fs::read(&victim).unwrap(), b"untouched");
    assert_eq!(
        std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn symlinked_observation_parent_is_rejected() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().expect("temporary directory");
    let base = observation_directory(&temporary);
    let outside = base.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o700)).unwrap();
    let linked = base.join("linked");
    symlink(&outside, &linked).unwrap();
    let path = linked.join("observation.json");
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");

    assert!(
        run_bridge(&path, &accepted_payload(&marker))
            .status
            .success()
    );
    assert!(!outside.join("observation.json").exists());
    assert!(!outside.join("observation.json.lock").exists());
}

#[test]
fn post_replace_leaf_swap_never_chmods_an_attacker_target() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "observation.json");
    let victim = observation_directory(&temporary).join("victim");
    std::fs::write(&victim, b"attacker").unwrap();
    std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    let setup = r#"
original_replace = bridge.os.replace
def replace_and_swap(source, target, *args, **kwargs):
    original_replace(source, target, *args, **kwargs)
    directory = kwargs["dst_dir_fd"]
    os.unlink(target, dir_fd=directory)
    os.symlink(os.environ["BRAIN_TEST_VICTIM"], target, dir_fd=directory)
bridge.os.replace = replace_and_swap
"#;

    assert!(
        run_bridge_with_setup(
            &path,
            &accepted_payload(&marker),
            setup,
            &[("BRAIN_TEST_VICTIM", victim.as_path())],
        )
        .status
        .success()
    );
    assert_eq!(
        std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn failed_replace_cleanup_does_not_unlink_an_attacker_replacement() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "observation.json");
    let record = observation_directory(&temporary).join("replacement-path");
    let marker = format!("<!-- brain:receiver-job-token={JOB_TOKEN} -->");
    let setup = r#"
def replace_with_attacker(source, target, *args, **kwargs):
    directory = kwargs["src_dir_fd"]
    os.unlink(source, dir_fd=directory)
    attacker = os.open(source, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600, dir_fd=directory)
    os.write(attacker, b"attacker-owned")
    os.close(attacker)
    with open(os.environ["BRAIN_TEST_RECORD"], "w", encoding="utf-8") as record:
        record.write(str(source))
    raise OSError("injected replacement failure")
bridge.os.replace = replace_with_attacker
"#;

    assert!(
        run_bridge_with_setup(
            &path,
            &accepted_payload(&marker),
            setup,
            &[("BRAIN_TEST_RECORD", record.as_path())],
        )
        .status
        .success()
    );
    let replacement = path
        .parent()
        .unwrap()
        .join(std::fs::read_to_string(record).unwrap());
    assert_eq!(std::fs::read(replacement).unwrap(), b"attacker-owned");
}

#[test]
fn completion_first_writes_revision_one_with_null_intermediate_boundaries() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = observation_path(&temporary, "observation.json");
    let completed = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": SESSION_ID,
        "turn_id": "turn-final",
    });

    assert!(run_bridge(&path, &completed).status.success());
    let value = snapshot(&path);
    assert_eq!(value["revision"], 1);
    assert_eq!(value["phase"], "completed");
    assert_eq!(value["job_token"], JOB_TOKEN);
    assert_eq!(value["instance_id"], INSTANCE_ID);
    assert_eq!(value["session_id"], SESSION_ID);
    assert_eq!(value["turn_id"], "turn-final");
    assert!(value["accepted_at_unix_ms"].is_null());
    assert!(value["progressing_at_unix_ms"].is_null());
    assert!(value["completed_at_unix_ms"].as_u64().is_some());
}

#[test]
fn revision_saturation_preserves_the_last_valid_snapshot_for_later_events() {
    let cases = [
        (
            serde_json::json!({
                "version": 1,
                "revision": i64::MAX,
                "phase": "accepted",
                "job_token": JOB_TOKEN,
                "instance_id": INSTANCE_ID,
                "session_id": SESSION_ID,
                "turn_id": null,
                "accepted_at_unix_ms": 1_000,
                "progressing_at_unix_ms": null,
                "completed_at_unix_ms": null,
            }),
            progress_payload(SESSION_ID, "turn-after-saturation"),
        ),
        (
            serde_json::json!({
                "version": 1,
                "revision": i64::MAX,
                "phase": "progressing",
                "job_token": JOB_TOKEN,
                "instance_id": INSTANCE_ID,
                "session_id": SESSION_ID,
                "turn_id": "turn-before-saturation",
                "accepted_at_unix_ms": 1_000,
                "progressing_at_unix_ms": 1_100,
                "completed_at_unix_ms": null,
            }),
            serde_json::json!({
                "hook_event_name": "Stop",
                "session_id": SESSION_ID,
                "turn_id": "turn-after-saturation",
            }),
        ),
    ];

    for (index, (before, event)) in cases.into_iter().enumerate() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = observation_path(&temporary, format!("saturated-{index}.json"));
        std::fs::write(&path, before.to_string()).expect("saturated snapshot");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only saturated snapshot");

        let output = run_bridge(&path, &event);
        assert!(output.status.success(), "bridge failed: {output:?}");
        assert_eq!(
            snapshot(&path),
            before,
            "case {index} replaced the last representable revision"
        );
    }
}

use std::path::{Path, PathBuf};

use super::*;

const TOKEN: &str = "6c06c55a-a9cf-4d75-b14e-75a5900c9088";
const INSTANCE: &str = "5cbd43f1-cc3f-4bc4-81ad-acad2bf85d39";
const SESSION: &str = "native-session-7";

fn request(path: &Path) -> AgentObservationRequest {
    AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path.to_path_buf(),
        AgentSession::new(SESSION).expect("session"),
        AgentObservationCursor::launched(),
    )
}

fn write_owner_only(path: &Path, body: &[u8]) {
    std::fs::write(path, body).expect("snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only snapshot");
    }
}

fn observation_directory(temporary: &tempfile::TempDir) -> PathBuf {
    let directory = temporary
        .path()
        .join("home")
        .join(".cache")
        .join("brain")
        .join("workspaces")
        .join("f5ecda26-5e5d-4dd0-91f8-c49bd0fb4c31")
        .join("receiver-observations");
    std::fs::create_dir_all(&directory).expect("observation directory");
    directory
}

fn snapshot_value() -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "revision": 3,
        "phase": "completed",
        "job_token": TOKEN,
        "instance_id": INSTANCE,
        "session_id": SESSION,
        "turn_id": "turn-9",
        "accepted_at_unix_ms": 1000,
        "progressing_at_unix_ms": 1100,
        "latest_progress_at_unix_ms": 1100,
        "completed_at_unix_ms": 1200,
    })
}

fn read_body(body: impl AsRef<[u8]>) -> Result<AgentObservationResult, AgentObservationError> {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let path = observation_directory(&temporary).join("observation.json");
    write_owner_only(&path, body.as_ref());
    read_normalized_snapshot(&request(&path))
}

#[test]
fn trailing_json_after_the_eleven_field_snapshot_is_malformed() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let path = observation_directory(&temporary).join("observation.json");
    let body = format!(
        r#"{{"version":1,"revision":1,"phase":"accepted","job_token":"{TOKEN}","instance_id":"{INSTANCE}","session_id":"{SESSION}","turn_id":null,"accepted_at_unix_ms":1000,"progressing_at_unix_ms":null,"latest_progress_at_unix_ms":null,"completed_at_unix_ms":null}} []"#
    );
    write_owner_only(&path, body.as_bytes());

    assert_eq!(
        read_normalized_snapshot(&request(&path)),
        Err(AgentObservationError::MalformedSnapshot)
    );
}

#[test]
fn schema_v1_rejects_missing_duplicate_unknown_and_invalid_fields() {
    let mut cases = Vec::new();
    let mut missing = snapshot_value();
    missing.as_object_mut().unwrap().remove("turn_id");
    cases.push((
        "missing",
        missing.to_string(),
        AgentObservationError::MalformedSnapshot,
    ));
    let duplicate =
        snapshot_value()
            .to_string()
            .replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    cases.push((
        "duplicate",
        duplicate,
        AgentObservationError::MalformedSnapshot,
    ));
    let mut unknown = snapshot_value();
    unknown["extra"] = serde_json::json!(true);
    cases.push((
        "unknown",
        unknown.to_string(),
        AgentObservationError::MalformedSnapshot,
    ));
    for (label, field, field_value) in [
        ("version", "version", serde_json::json!(2)),
        ("zero revision", "revision", serde_json::json!(0)),
        (
            "large revision",
            "revision",
            serde_json::json!(u64::try_from(i64::MAX).unwrap() + 1),
        ),
        ("token", "job_token", serde_json::json!("not-a-uuid")),
        ("instance", "instance_id", serde_json::json!("not-a-uuid")),
        ("empty session", "session_id", serde_json::json!("")),
        (
            "control session",
            "session_id",
            serde_json::json!("native\nsession"),
        ),
        (
            "long session",
            "session_id",
            serde_json::json!("s".repeat(257)),
        ),
        ("empty turn", "turn_id", serde_json::json!("")),
        ("control turn", "turn_id", serde_json::json!("turn\n9")),
        ("long turn", "turn_id", serde_json::json!("t".repeat(257))),
    ] {
        let mut snapshot = snapshot_value();
        snapshot[field] = field_value;
        cases.push((
            label,
            snapshot.to_string(),
            AgentObservationError::MalformedSnapshot,
        ));
    }

    for (label, body, expected) in cases {
        assert_eq!(read_body(body), Err(expected), "{label}");
    }
}

#[test]
fn phase_timestamp_contract_accepts_only_unambiguous_nondecreasing_lifecycles() {
    let cases = [
        ("accepted without time", "accepted", None, None, None),
        (
            "accepted with completion",
            "accepted",
            Some(1),
            None,
            Some(2),
        ),
        (
            "progress without accepted",
            "progressing",
            None,
            Some(2),
            None,
        ),
        ("progress without time", "progressing", Some(1), None, None),
        (
            "complete without completion",
            "completed",
            Some(1),
            None,
            None,
        ),
        (
            "complete progress without accepted",
            "completed",
            None,
            Some(2),
            Some(3),
        ),
        (
            "descending accepted progress",
            "completed",
            Some(2),
            Some(1),
            Some(3),
        ),
        (
            "descending progress complete",
            "completed",
            Some(1),
            Some(3),
            Some(2),
        ),
    ];
    for (label, phase, accepted, progressing, completed) in cases {
        let mut value = snapshot_value();
        value["phase"] = serde_json::json!(phase);
        value["accepted_at_unix_ms"] = serde_json::json!(accepted);
        value["progressing_at_unix_ms"] = serde_json::json!(progressing);
        value["latest_progress_at_unix_ms"] = serde_json::json!(progressing);
        value["completed_at_unix_ms"] = serde_json::json!(completed);
        assert_eq!(
            read_body(value.to_string()),
            Err(AgentObservationError::AmbiguousLifecycle),
            "{label}"
        );
    }

    for (label, accepted, progressing) in [
        ("completion only", None, None),
        ("accepted then completion", Some(1), None),
        ("full lifecycle", Some(1), Some(2)),
    ] {
        let mut value = snapshot_value();
        value["accepted_at_unix_ms"] = serde_json::json!(accepted);
        value["progressing_at_unix_ms"] = serde_json::json!(progressing);
        value["latest_progress_at_unix_ms"] = serde_json::json!(progressing);
        value["completed_at_unix_ms"] = serde_json::json!(3);
        assert!(read_body(value.to_string()).is_ok(), "{label}");
    }
}

#[test]
fn missing_equal_regressed_and_mismatched_snapshots_are_conservative() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let observations = observation_directory(&temporary);
    let missing = observations.join("missing.json");
    let pending = read_normalized_snapshot(&request(&missing)).expect("missing is pending");
    assert!(pending.boundaries().is_empty());
    assert_eq!(pending.next_cursor(), AgentObservationCursor::launched());

    let path = observations.join("snapshot.json");
    let mut value = snapshot_value();
    value["revision"] = serde_json::json!(1);
    value["phase"] = serde_json::json!("accepted");
    value["progressing_at_unix_ms"] = serde_json::Value::Null;
    value["latest_progress_at_unix_ms"] = serde_json::Value::Null;
    value["completed_at_unix_ms"] = serde_json::Value::Null;
    write_owner_only(&path, value.to_string().as_bytes());
    let accepted = read_normalized_snapshot(&request(&path)).expect("accepted");
    let next = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path.clone(),
        AgentSession::new(SESSION).unwrap(),
        accepted.next_cursor(),
    );
    assert!(
        read_normalized_snapshot(&next)
            .unwrap()
            .boundaries()
            .is_empty()
    );

    let regressed = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path.clone(),
        AgentSession::new(SESSION).unwrap(),
        AgentObservationCursor {
            revision: 2,
            represented: LAUNCHED_BIT | ACCEPTED_BIT,
            accepted_at_unix_ms: Some(1_000),
            progressing_at_unix_ms: None,
            latest_progress_at_unix_ms: None,
            completed_at_unix_ms: None,
        },
    );
    assert_eq!(
        read_normalized_snapshot(&regressed),
        Err(AgentObservationError::RevisionRegression)
    );

    value["revision"] = serde_json::json!(2);
    value["job_token"] = serde_json::json!("fb2ce54d-846f-444a-87e4-4002b89c5468");
    write_owner_only(&path, value.to_string().as_bytes());
    assert_eq!(
        read_normalized_snapshot(&request(&path)),
        Err(AgentObservationError::IdentityMismatch)
    );
    value["job_token"] = serde_json::json!(TOKEN);
    value["session_id"] = serde_json::json!("prior-session");
    write_owner_only(&path, value.to_string().as_bytes());
    assert_eq!(
        read_normalized_snapshot(&request(&path)),
        Err(AgentObservationError::SessionMismatch)
    );
}

#[test]
fn durable_cursor_rebuild_emits_only_boundaries_not_already_persisted() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let path = observation_directory(&temporary).join("snapshot.json");
    let mut value = snapshot_value();
    value["revision"] = serde_json::json!(2);
    value["phase"] = serde_json::json!("progressing");
    value["completed_at_unix_ms"] = serde_json::Value::Null;
    write_owner_only(&path, value.to_string().as_bytes());
    let cursor = AgentObservationCursor::from_durable(1, Some(1_000), None, None, None)
        .expect("valid accepted durable cursor");

    let result = read_normalized_snapshot(&AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path,
        AgentSession::new(SESSION).expect("session"),
        cursor,
    ))
    .expect("newer progress observation");

    assert_eq!(
        result.boundaries(),
        &[AgentObservationBoundary::new(
            AgentObservationPhase::Progressing,
            1_100,
        )]
    );
}

#[test]
fn newer_revision_can_return_a_progress_pulse_without_a_new_lifecycle_phase() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let path = observation_directory(&temporary).join("snapshot.json");
    let mut value = snapshot_value();
    value["revision"] = serde_json::json!(2);
    value["phase"] = serde_json::json!("progressing");
    value["latest_progress_at_unix_ms"] = serde_json::json!(1_100);
    value["completed_at_unix_ms"] = serde_json::Value::Null;
    write_owner_only(&path, value.to_string().as_bytes());
    let first = read_normalized_snapshot(&request(&path)).expect("first progress observation");
    assert_eq!(
        first
            .progress_pulse()
            .expect("first progress pulse")
            .observed_at_unix_ms(),
        1_100
    );

    value["revision"] = serde_json::json!(3);
    value["turn_id"] = serde_json::json!("turn-10");
    value["latest_progress_at_unix_ms"] = serde_json::json!(1_200);
    write_owner_only(&path, value.to_string().as_bytes());
    let later = read_normalized_snapshot(&AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path,
        AgentSession::new(SESSION).expect("session"),
        first.next_cursor(),
    ))
    .expect("later progress pulse");

    assert!(later.boundaries().is_empty());
    assert_eq!(
        later
            .progress_pulse()
            .expect("later progress pulse")
            .observed_at_unix_ms(),
        1_200
    );
    assert_eq!(later.next_cursor().durable_revision(), 3);
}

#[test]
fn durable_cursor_rebuild_rejects_impossible_persisted_lifecycles() {
    for (label, revision, accepted, progressing, completed) in [
        ("revision without evidence", 1, None, None, None),
        ("evidence without revision", 0, Some(100), None, None),
        ("progress without acceptance", 2, None, Some(200), None),
        ("descending progress", 2, Some(200), Some(100), None),
        ("descending completion", 3, Some(100), Some(300), Some(200)),
    ] {
        assert_eq!(
            AgentObservationCursor::from_durable(
                revision,
                accepted,
                progressing,
                progressing,
                completed,
            ),
            Err(AgentObservationError::AmbiguousLifecycle),
            "{label}"
        );
    }
    assert!(AgentObservationCursor::from_durable(1, None, None, None, Some(300)).is_ok());
}

#[test]
fn higher_revision_cannot_rewrite_a_prior_timestamp_or_decrease_the_stream() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let path = observation_directory(&temporary).join("snapshot.json");
    let mut value = snapshot_value();
    value["revision"] = serde_json::json!(1);
    value["phase"] = serde_json::json!("accepted");
    value["accepted_at_unix_ms"] = serde_json::json!(100);
    value["progressing_at_unix_ms"] = serde_json::Value::Null;
    value["latest_progress_at_unix_ms"] = serde_json::Value::Null;
    value["completed_at_unix_ms"] = serde_json::Value::Null;
    write_owner_only(&path, value.to_string().as_bytes());
    let first = read_normalized_snapshot(&request(&path)).expect("first accepted boundary");

    value["revision"] = serde_json::json!(2);
    value["phase"] = serde_json::json!("progressing");
    value["accepted_at_unix_ms"] = serde_json::json!(50);
    value["progressing_at_unix_ms"] = serde_json::json!(60);
    value["latest_progress_at_unix_ms"] = serde_json::json!(60);
    write_owner_only(&path, value.to_string().as_bytes());
    let next = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path,
        AgentSession::new(SESSION).expect("session"),
        first.next_cursor(),
    );

    assert_eq!(
        read_normalized_snapshot(&next),
        Err(AgentObservationError::AmbiguousLifecycle)
    );
}

#[test]
fn higher_revision_cannot_introduce_an_earlier_phase_after_completion() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let path = observation_directory(&temporary).join("snapshot.json");
    let mut value = snapshot_value();
    value["revision"] = serde_json::json!(1);
    value["phase"] = serde_json::json!("completed");
    value["accepted_at_unix_ms"] = serde_json::Value::Null;
    value["progressing_at_unix_ms"] = serde_json::Value::Null;
    value["latest_progress_at_unix_ms"] = serde_json::Value::Null;
    value["completed_at_unix_ms"] = serde_json::json!(100);
    write_owner_only(&path, value.to_string().as_bytes());
    let first = read_normalized_snapshot(&request(&path)).expect("completion-only boundary");

    value["revision"] = serde_json::json!(2);
    value["accepted_at_unix_ms"] = serde_json::json!(50);
    write_owner_only(&path, value.to_string().as_bytes());
    let next = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path,
        AgentSession::new(SESSION).expect("session"),
        first.next_cursor(),
    );

    assert_eq!(
        read_normalized_snapshot(&next),
        Err(AgentObservationError::AmbiguousLifecycle)
    );
}

#[test]
fn higher_revision_cannot_erase_a_previously_observed_phase() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let path = observation_directory(&temporary).join("snapshot.json");
    let mut value = snapshot_value();
    value["revision"] = serde_json::json!(1);
    value["phase"] = serde_json::json!("progressing");
    value["accepted_at_unix_ms"] = serde_json::json!(100);
    value["progressing_at_unix_ms"] = serde_json::json!(110);
    value["latest_progress_at_unix_ms"] = serde_json::json!(110);
    value["completed_at_unix_ms"] = serde_json::Value::Null;
    write_owner_only(&path, value.to_string().as_bytes());
    let first = read_normalized_snapshot(&request(&path)).expect("progressing boundaries");

    value["revision"] = serde_json::json!(2);
    value["phase"] = serde_json::json!("accepted");
    value["progressing_at_unix_ms"] = serde_json::Value::Null;
    value["latest_progress_at_unix_ms"] = serde_json::Value::Null;
    write_owner_only(&path, value.to_string().as_bytes());
    let next = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path,
        AgentSession::new(SESSION).expect("session"),
        first.next_cursor(),
    );

    assert_eq!(
        read_normalized_snapshot(&next),
        Err(AgentObservationError::AmbiguousLifecycle)
    );
}

#[test]
fn exact_256_byte_session_and_turn_identifiers_are_accepted() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let path = observation_directory(&temporary).join("snapshot.json");
    let session_id = "s".repeat(256);
    let turn_id = "t".repeat(256);
    let mut value = snapshot_value();
    value["revision"] = serde_json::json!(1);
    value["phase"] = serde_json::json!("accepted");
    value["session_id"] = serde_json::json!(session_id);
    value["turn_id"] = serde_json::json!(turn_id);
    value["progressing_at_unix_ms"] = serde_json::Value::Null;
    value["latest_progress_at_unix_ms"] = serde_json::Value::Null;
    value["completed_at_unix_ms"] = serde_json::Value::Null;
    write_owner_only(&path, value.to_string().as_bytes());
    let request = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path,
        AgentSession::new(session_id).expect("exact identifier bound"),
        AgentObservationCursor::launched(),
    );

    let result = read_normalized_snapshot(&request).expect("exact identifier bounds");

    assert_eq!(
        result.boundaries(),
        &[AgentObservationBoundary::new(
            AgentObservationPhase::Accepted,
            1_000,
        )]
    );
}

#[test]
fn file_type_size_and_permissions_are_bounded_before_parsing() {
    let temporary = tempfile::tempdir().expect("temporary observation");
    let observations = observation_directory(&temporary);
    let directory = observations.join("directory.json");
    std::fs::create_dir(&directory).unwrap();
    assert_eq!(
        read_normalized_snapshot(&request(&directory)),
        Err(AgentObservationError::InvalidFileType)
    );

    let oversized = observations.join("oversized.json");
    write_owner_only(&oversized, &vec![b'x'; 4097]);
    assert_eq!(
        read_normalized_snapshot(&request(&oversized)),
        Err(AgentObservationError::SnapshotTooLarge)
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt as _, symlink};
        let permissive = observations.join("permissive.json");
        write_owner_only(&permissive, snapshot_value().to_string().as_bytes());
        std::fs::set_permissions(&permissive, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(
            read_normalized_snapshot(&request(&permissive)),
            Err(AgentObservationError::InvalidPermissions)
        );
        let link = observations.join("link.json");
        symlink(&permissive, &link).unwrap();
        assert_eq!(
            read_normalized_snapshot(&request(&link)),
            Err(AgentObservationError::InvalidFileType)
        );
    }
}

#[test]
fn observation_errors_never_render_private_request_or_snapshot_values() {
    let private = [
        TOKEN,
        INSTANCE,
        "/private/observation.json",
        "private prompt body",
    ];
    let request = AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        PathBuf::from(private[2]),
        AgentSession::new("private-session").unwrap(),
        AgentObservationCursor::launched(),
    );
    let rendered = format!(
        "{request:?} {:?} {}",
        AgentObservationError::MalformedSnapshot,
        AgentObservationError::MalformedSnapshot
    );
    for value in private {
        assert!(!rendered.contains(value), "leaked private value");
    }
}

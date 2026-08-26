use super::*;

const TOKEN: &str = "6c06c55a-a9cf-4d75-b14e-75a5900c9088";
const INSTANCE: &str = "5cbd43f1-cc3f-4bc4-81ad-acad2bf85d39";
const SESSION: &str = "native-session-7";

fn accepted_snapshot() -> String {
    format!(
        r#"{{"version":1,"revision":1,"phase":"accepted","job_token":"{TOKEN}","instance_id":"{INSTANCE}","session_id":"{SESSION}","turn_id":null,"accepted_at_unix_ms":1000,"progressing_at_unix_ms":null,"latest_progress_at_unix_ms":null,"completed_at_unix_ms":null}}"#
    )
}

fn request(path: PathBuf) -> AgentObservationRequest {
    AgentObservationRequest::new(
        TOKEN,
        INSTANCE,
        path,
        AgentSession::new(SESSION).expect("session"),
        AgentObservationCursor::launched(),
    )
}

#[cfg(unix)]
#[test]
fn symlinked_observation_ancestor_cannot_escape_the_workspace_cache() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let temporary = tempfile::tempdir().expect("temporary root");
    let cache = temporary
        .path()
        .join("home")
        .join(".cache")
        .join("brain")
        .join("workspaces")
        .join("f5ecda26-5e5d-4dd0-91f8-c49bd0fb4c31");
    let outside = temporary.path().join("outside");
    std::fs::create_dir_all(&cache).expect("cache root");
    std::fs::create_dir(&outside).expect("outside directory");
    let outside_snapshot = outside.join(format!("{INSTANCE}.json"));
    std::fs::write(&outside_snapshot, accepted_snapshot()).expect("outside snapshot");
    std::fs::set_permissions(&outside_snapshot, std::fs::Permissions::from_mode(0o600))
        .expect("owner-only snapshot");
    symlink(&outside, cache.join("receiver-observations")).expect("symlinked ancestor");
    let path = cache
        .join("receiver-observations")
        .join(format!("{INSTANCE}.json"));

    assert_eq!(
        read_normalized_snapshot(&request(path)),
        Err(AgentObservationError::InvalidFileType)
    );
}

#[cfg(unix)]
#[test]
fn symlinked_cache_ancestor_cannot_escape_the_workspace_home() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let temporary = tempfile::tempdir().expect("temporary root");
    let home = temporary.path().join("home");
    let outside = temporary.path().join("outside");
    let cache = outside
        .join("brain")
        .join("workspaces")
        .join("f5ecda26-5e5d-4dd0-91f8-c49bd0fb4c31");
    let observations = cache.join("receiver-observations");
    std::fs::create_dir(&home).expect("home");
    std::fs::create_dir_all(&observations).expect("outside cache");
    symlink(&outside, home.join(".cache")).expect("symlinked cache ancestor");
    let snapshot = observations.join(format!("{INSTANCE}.json"));
    std::fs::write(&snapshot, accepted_snapshot()).expect("outside snapshot");
    std::fs::set_permissions(&snapshot, std::fs::Permissions::from_mode(0o600))
        .expect("owner-only snapshot");
    let path = home
        .join(".cache")
        .join("brain")
        .join("workspaces")
        .join("f5ecda26-5e5d-4dd0-91f8-c49bd0fb4c31")
        .join("receiver-observations")
        .join(format!("{INSTANCE}.json"));

    assert_eq!(
        read_normalized_snapshot(&request(path)),
        Err(AgentObservationError::InvalidFileType)
    );
}

#[cfg(unix)]
#[test]
fn replacement_between_validation_and_open_is_validated_on_the_opened_handle() {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir().expect("temporary root");
    let observations = temporary
        .path()
        .join("home")
        .join(".cache")
        .join("brain")
        .join("workspaces")
        .join("f5ecda26-5e5d-4dd0-91f8-c49bd0fb4c31")
        .join("receiver-observations");
    std::fs::create_dir_all(&observations).expect("observation directory");
    let path = observations.join(format!("{INSTANCE}.json"));
    let replacement = observations.join("replacement.json");
    std::fs::write(&path, accepted_snapshot()).expect("checked snapshot");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("checked owner-only mode");
    std::fs::write(&replacement, accepted_snapshot()).expect("replacement snapshot");
    std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o644))
        .expect("replacement permissive mode");
    let request = request(path.clone());

    assert_eq!(
        snapshot::read_normalized_snapshot_with_open_hook(&request, || {
            std::fs::rename(&replacement, &path).expect("replace after validation");
        }),
        Err(AgentObservationError::InvalidPermissions)
    );
}

#[cfg(unix)]
#[test]
fn one_short_read_is_a_truncated_snapshot_even_when_handle_length_is_stable() {
    let body = accepted_snapshot();

    assert_eq!(
        snapshot::read_opened_snapshot_for_test(body.as_bytes(), body.len(), body.len() - 1,),
        Err(AgentObservationError::TruncatedSnapshot)
    );
}

#[cfg(unix)]
#[test]
fn exact_4096_byte_snapshot_is_accepted_by_the_bounded_reader() {
    use std::os::unix::fs::PermissionsExt as _;

    let mut body = accepted_snapshot().into_bytes();
    body.resize(4096, b' ');
    let temporary = tempfile::tempdir().expect("temporary root");
    let observations = temporary
        .path()
        .join("home")
        .join(".cache")
        .join("brain")
        .join("workspaces")
        .join("f5ecda26-5e5d-4dd0-91f8-c49bd0fb4c31")
        .join("receiver-observations");
    std::fs::create_dir_all(&observations).expect("observation directory");
    let path = observations.join(format!("{INSTANCE}.json"));
    std::fs::write(&path, body).expect("exact-bound snapshot");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("owner-only snapshot");

    let result = read_normalized_snapshot(&request(path)).expect("exact bound");

    assert_eq!(
        result.boundaries(),
        &[AgentObservationBoundary::new(
            AgentObservationPhase::Accepted,
            1_000,
        )]
    );
}

#[cfg(unix)]
#[test]
fn fifo_snapshot_is_rejected_without_blocking() {
    use nix::{sys::stat::Mode, unistd::mkfifo};

    let temporary = tempfile::tempdir().expect("temporary root");
    let observations = temporary
        .path()
        .join("home")
        .join(".cache")
        .join("brain")
        .join("workspaces")
        .join("f5ecda26-5e5d-4dd0-91f8-c49bd0fb4c31")
        .join("receiver-observations");
    std::fs::create_dir_all(&observations).expect("observation directory");
    let path = observations.join(format!("{INSTANCE}.json"));
    mkfifo(&path, Mode::S_IRUSR | Mode::S_IWUSR).expect("fifo");

    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        sender
            .send(read_normalized_snapshot(&request(path)))
            .expect("send observation result");
    });

    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("bounded FIFO observation"),
        Err(AgentObservationError::InvalidFileType)
    );
}

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use brain::command::sync::{WorkspaceLockOutcome, run_with_workspace_lock};
use brain::sync::args::Direction;
use brain::sync::trigger::{
    DetachedSyncRequest, DetachedSyncRunner, detached_sync_request, spawn_detached_sync_with,
};
use brain::users::{USERS_SCHEMA_VERSION, User, UserId, Users, UsersStore};
use brain::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceContext, WorkspaceId,
    WorkspaceManifest, WorkspaceName, WorkspaceRecord,
};

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

fn family_context(root: &std::path::Path) -> WorkspaceContext {
    WorkspaceContext::new(
        root,
        WorkspaceId::parse(FAMILY_ID).expect("family UUID"),
        WorkspaceName::parse("family").expect("family name"),
        &root.join("family"),
        "pablo",
        root,
    )
    .expect("family context")
}

fn ready_family_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join("family");
    std::fs::create_dir_all(&root).expect("workspace root");
    let family_id = WorkspaceId::parse(FAMILY_ID).expect("family UUID");
    WorkspaceManifest::new(family_id)
        .write_new(&root)
        .expect("workspace manifest");
    let name = WorkspaceName::parse("family").expect("family name");
    let workspace = WorkspaceContext::new(
        home.path(),
        family_id,
        name.clone(),
        &root,
        "pablo",
        home.path(),
    )
    .expect("workspace context");
    UsersStore::save(
        &workspace,
        &Users {
            schema_version: USERS_SCHEMA_VERSION,
            users: vec![User {
                id: UserId::parse("pablo").expect("user ID"),
                name: "Pablo".to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            }],
        },
    )
    .expect("portable users");
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: name.clone(),
        workspaces: BTreeMap::from([(
            name,
            WorkspaceRecord {
                workspace_id: family_id,
                root,
                aliases: BTreeSet::new(),
                local_user_id: "pablo".to_owned(),
                receiver_enabled: false,
                env: serde_json::Map::new(),
            },
        )]),
    };
    RegistryStore::from_path(home.path().join(".config/brain/env.json"))
        .replace(&registry)
        .expect("machine registry");
    home
}

#[test]
fn bootstrap_refuses_a_detached_child_whose_expected_uuid_disagrees_with_selection() {
    let home = ready_family_home();

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["--brain", "family", "sync", "status"])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env("BRAIN_WORKSPACE_ID", PERSONAL_ID)
        .env("NO_COLOR", "1")
        .current_dir(home.path())
        .output()
        .expect("run Brain child");

    assert!(
        !output.status.success(),
        "mismatched child must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(PERSONAL_ID), "{stderr}");
    assert!(stderr.contains(FAMILY_ID), "{stderr}");
}

#[test]
fn bootstrap_accepts_a_detached_child_whose_expected_uuid_matches_selection() {
    let home = ready_family_home();

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["--brain", "family", "sync", "status"])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env("BRAIN_WORKSPACE_ID", FAMILY_ID)
        .env("NO_COLOR", "1")
        .current_dir(home.path())
        .output()
        .expect("run Brain child");

    assert!(
        output.status.success(),
        "matching child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn detached_request_keeps_canonical_argv_and_carries_the_expected_workspace_uuid() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let workspace = family_context(temporary.path());

    let request = detached_sync_request(&workspace, Direction::Pull);

    assert_eq!(
        request.args,
        ["--workspace", "family", "sync", "--pull", "--if-idle"]
    );
    assert_eq!(
        request.env,
        [("BRAIN_WORKSPACE_ID".to_owned(), FAMILY_ID.to_owned())]
    );
}

#[derive(Default)]
struct RecordingChildRunner {
    requests: Mutex<Vec<DetachedSyncRequest>>,
}

impl DetachedSyncRunner for RecordingChildRunner {
    fn spawn(&self, request: DetachedSyncRequest) -> std::io::Result<u32> {
        self.requests.lock().expect("recording lock").push(request);
        Ok(42)
    }
}

#[test]
fn injected_child_runner_receives_the_immutable_workspace_request() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let workspace = family_context(temporary.path());
    let runner = RecordingChildRunner::default();

    let pid = spawn_detached_sync_with(&workspace, Direction::Push, &runner);

    assert_eq!(pid, Some(42));
    assert_eq!(
        *runner.requests.lock().expect("recording lock"),
        [DetachedSyncRequest {
            args: vec![
                "--workspace".to_owned(),
                "family".to_owned(),
                "sync".to_owned(),
                "--push".to_owned(),
                "--if-idle".to_owned(),
            ],
            env: vec![("BRAIN_WORKSPACE_ID".to_owned(), FAMILY_ID.to_owned())],
        }]
    );
}

#[test]
fn different_workspace_command_boundaries_enter_concurrently() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let personal = brain::workspace::WorkspacePaths::new(
        temporary.path(),
        WorkspaceId::parse(PERSONAL_ID).expect("personal UUID"),
    );
    let family = brain::workspace::WorkspacePaths::new(
        temporary.path(),
        WorkspaceId::parse(FAMILY_ID).expect("family UUID"),
    );
    let (entered_tx, entered_rx) = mpsc::channel();
    let (personal_release_tx, personal_release_rx) = mpsc::channel();
    let (family_release_tx, family_release_rx) = mpsc::channel();
    let personal_entered = entered_tx.clone();
    let personal_thread = std::thread::spawn(move || {
        run_with_workspace_lock(
            &personal,
            true,
            || {
                personal_entered.send(PERSONAL_ID).expect("report entry");
                personal_release_rx.recv().expect("release personal");
            },
            || panic!("an idle different-workspace run must not follow"),
        )
    });
    let family_thread = std::thread::spawn(move || {
        run_with_workspace_lock(
            &family,
            true,
            || {
                entered_tx.send(FAMILY_ID).expect("report entry");
                family_release_rx.recv().expect("release family");
            },
            || panic!("an idle different-workspace run must not follow"),
        )
    });

    let first = entered_rx.recv_timeout(Duration::from_secs(2));
    let second = entered_rx.recv_timeout(Duration::from_secs(2));
    personal_release_tx.send(()).expect("release personal");
    family_release_tx.send(()).expect("release family");

    let entered = [
        first.expect("first workspace entered"),
        second.expect("second workspace entered"),
    ];
    assert!(entered.contains(&PERSONAL_ID), "{entered:?}");
    assert!(entered.contains(&FAMILY_ID), "{entered:?}");
    assert_eq!(
        personal_thread.join().expect("personal thread"),
        WorkspaceLockOutcome::Ran(())
    );
    assert_eq!(
        family_thread.join().expect("family thread"),
        WorkspaceLockOutcome::Ran(())
    );
}

#[test]
fn same_workspace_automatic_trigger_coalesces_while_the_runner_is_active() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let paths = Arc::new(brain::workspace::WorkspacePaths::new(
        temporary.path(),
        WorkspaceId::parse(FAMILY_ID).expect("family UUID"),
    ));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let held_paths = Arc::clone(&paths);
    let held = std::thread::spawn(move || {
        run_with_workspace_lock(
            &held_paths,
            true,
            || {
                entered_tx.send(()).expect("report entry");
                release_rx.recv().expect("release held run");
            },
            || panic!("the lock owner must not follow"),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first run entered");

    let coalesced = run_with_workspace_lock(
        &paths,
        true,
        || panic!("same-workspace automatic trigger must not enter"),
        || panic!("same-workspace automatic trigger must not follow"),
    );
    release_tx.send(()).expect("release held run");

    assert_eq!(coalesced, WorkspaceLockOutcome::Coalesced);
    assert_eq!(
        held.join().expect("held thread"),
        WorkspaceLockOutcome::Ran(())
    );
}

#[test]
fn same_workspace_manual_trigger_follows_instead_of_entering() {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let paths = Arc::new(brain::workspace::WorkspacePaths::new(
        temporary.path(),
        WorkspaceId::parse(FAMILY_ID).expect("family UUID"),
    ));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let held_paths = Arc::clone(&paths);
    let held = std::thread::spawn(move || {
        run_with_workspace_lock(
            &held_paths,
            true,
            || {
                entered_tx.send(()).expect("report entry");
                release_rx.recv().expect("release held run");
            },
            || panic!("the lock owner must not follow"),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first run entered");
    let followed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_follow = Arc::clone(&followed);

    let outcome = run_with_workspace_lock(
        &paths,
        false,
        || panic!("same-workspace manual trigger must not enter"),
        move || saw_follow.store(true, std::sync::atomic::Ordering::SeqCst),
    );
    release_tx.send(()).expect("release held run");

    assert_eq!(outcome, WorkspaceLockOutcome::Followed);
    assert!(followed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        held.join().expect("held thread"),
        WorkspaceLockOutcome::Ran(())
    );
}

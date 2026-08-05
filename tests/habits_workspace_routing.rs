use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use brain::server::lifecycle::{ElectionGuard, ServerGeneration, ServerPaths};
use brain::workspace::{
    MachineRegistry, RegistryStore, WorkspaceId, WorkspaceManifest, WorkspaceName, WorkspaceRecord,
};
use serde_json::Map;
use tempfile::TempDir;

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const UNKNOWN_ID: &str = "d451ec8f-067f-4d68-9a52-fcefb79faa70";

struct ServerFixture {
    _home: TempDir,
    personal_root: std::path::PathBuf,
    family_root: std::path::PathBuf,
    port: u16,
    child: Child,
}

impl ServerFixture {
    fn new(family_manifest_id: &str) -> Self {
        let home = tempfile::tempdir().expect("temporary home");
        let personal_root = home.path().join("personal");
        let family_root = home.path().join("family");
        write_workspace(&personal_root, PERSONAL_ID, "Personal habit");
        write_workspace(&family_root, family_manifest_id, "Family habit");

        let personal_name = WorkspaceName::parse("personal").expect("personal name");
        let family_name = WorkspaceName::parse("family").expect("family name");
        let registry = MachineRegistry {
            schema_version: 2,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (
                    personal_name,
                    WorkspaceRecord {
                        workspace_id: workspace_id(PERSONAL_ID),
                        root: personal_root.clone(),
                        aliases: BTreeSet::new(),
                        local_user_id: "pablo".to_owned(),
                        receiver_enabled: false,
                        env: Map::new(),
                    },
                ),
                (
                    family_name,
                    WorkspaceRecord {
                        workspace_id: workspace_id(FAMILY_ID),
                        root: family_root.clone(),
                        aliases: BTreeSet::new(),
                        local_user_id: "pablo".to_owned(),
                        receiver_enabled: false,
                        env: Map::new(),
                    },
                ),
            ]),
        };
        RegistryStore::from_path(home.path().join(".config/brain/env.json"))
            .replace(&registry)
            .expect("write workspace registry");

        let port = available_port();
        let paths = ServerPaths::from_home(home.path());
        let generation = ServerGeneration::new();
        let election = ElectionGuard::try_acquire(&paths, generation)
            .expect("elect test server")
            .expect("test fixture owns the server election");
        let child = Command::new(env!("CARGO_BIN_EXE_brain"))
            .args([
                "server",
                "run",
                "--generation",
                &generation.to_string(),
                "--port",
                &port.to_string(),
            ])
            .env("HOME", home.path())
            .env_remove("XDG_CONFIG_HOME")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start brain server");
        election.handoff();
        wait_for_server(port);

        Self {
            _home: home,
            personal_root,
            family_root,
            port,
            child,
        }
    }

    fn get(&self, path: &str) -> String {
        request(self.port, "GET", path, "")
    }

    fn post(&self, path: &str, body: &str) -> String {
        request(self.port, "POST", path, body)
    }
}

impl Drop for ServerFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn habits_get_renders_only_the_workspace_named_by_uuid() {
    let server = ServerFixture::new(FAMILY_ID);

    let response = server.get(&format!("/habits?workspace_id={FAMILY_ID}"));

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("Family habit"), "{response}");
    assert!(!response.contains("Personal habit"), "{response}");
    assert!(
        response.contains(&format!("/habits/done?workspace_id={FAMILY_ID}")),
        "the rendered page must preserve its UUID in completion requests"
    );
}

#[test]
fn habits_post_mutates_only_the_workspace_named_by_uuid() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);

    let response = server.post(
        &format!("/habits/done?workspace_id={FAMILY_ID}"),
        r#"{"task_id":"H1"}"#,
    );

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
    let family = String::from_utf8(habits_bytes(&server.family_root)).expect("family CSV utf8");
    assert!(family.contains("H1,Family habit,done"), "{family}");
}

#[test]
fn habits_requests_without_a_known_uuid_never_fall_back_to_default() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);

    for path in [
        "/habits".to_owned(),
        format!("/habits?workspace_id={UNKNOWN_ID}"),
    ] {
        let get = server.get(&path);
        let post_path = path.replacen("/habits", "/habits/done", 1);
        let post = server.post(&post_path, r#"{"task_id":"H1"}"#);
        assert!(!get.starts_with("HTTP/1.1 200"), "{path}: {get}");
        assert!(!post.starts_with("HTTP/1.1 200"), "{post_path}: {post}");
        assert!(!get.contains("Personal habit"), "{path}: {get}");
    }
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
}

#[test]
fn habits_requests_reject_a_manifest_identity_mismatch() {
    let server = ServerFixture::new(UNKNOWN_ID);
    let personal_before = habits_bytes(&server.personal_root);
    let family_before = habits_bytes(&server.family_root);

    let get = server.get(&format!("/habits?workspace_id={FAMILY_ID}"));
    let post = server.post(
        &format!("/habits/done?workspace_id={FAMILY_ID}"),
        r#"{"task_id":"H1"}"#,
    );

    assert!(!get.starts_with("HTTP/1.1 200"), "{get}");
    assert!(!post.starts_with("HTTP/1.1 200"), "{post}");
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
    assert_eq!(habits_bytes(&server.family_root), family_before);
}

#[test]
fn habits_requests_reject_an_unavailable_selected_root() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);
    std::fs::remove_dir_all(&server.family_root).expect("remove temporary family root");

    let get = server.get(&format!("/habits?workspace_id={FAMILY_ID}"));
    let post = server.post(
        &format!("/habits/done?workspace_id={FAMILY_ID}"),
        r#"{"task_id":"H1"}"#,
    );

    assert!(!get.starts_with("HTTP/1.1 200"), "{get}");
    assert!(!post.starts_with("HTTP/1.1 200"), "{post}");
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
}

fn write_workspace(root: &std::path::Path, manifest_id: &str, habit_name: &str) {
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).expect("tasks directory");
    WorkspaceManifest::new(workspace_id(manifest_id))
        .write_new(root)
        .expect("workspace manifest");
    std::fs::write(
        tasks.join("tasks.csv"),
        "task_id,task_name,status,completed_date,last_touched\n",
    )
    .expect("tasks CSV");
    std::fs::write(tasks.join(".habits_next_id"), "2\n").expect("habit counter");
    std::fs::write(
        tasks.join("habits.csv"),
        format!(
            "task_id,task_name,status,priority,due_date,hard_deadline,notes,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched\n\
             H1,{habit_name},not_started,p2,2026-07-24,false,,10,9:00 AM,1,days,2026-07-24,,\n"
        ),
    )
    .expect("habits CSV");
}

fn habits_bytes(root: &std::path::Path) -> Vec<u8> {
    std::fs::read(root.join("tasks/habits.csv")).expect("read habits CSV")
}

fn workspace_id(raw: &str) -> WorkspaceId {
    WorkspaceId::parse(raw).expect("valid workspace UUID")
}

fn available_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral test port")
        .local_addr()
        .expect("test port address")
        .port()
}

fn wait_for_server(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("brain server did not start on port {port}");
}

fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to brain server");
    stream
        .write_all(
            format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .expect("write HTTP request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read HTTP response");
    response
}

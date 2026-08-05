use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use brain::server::control::{LeaseRegistration, ServerClient};
use brain::server::lifecycle::{ElectionGuard, IngressId, LeaseId, ServerGeneration, ServerPaths};
use brain::workspace::{
    MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceManifest,
    WorkspaceName, WorkspaceRecord,
};
use serde_json::Map;
use tempfile::TempDir;

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const UNKNOWN_ID: &str = "d451ec8f-067f-4d68-9a52-fcefb79faa70";
const PERSONAL_LEASE: &str = "48a6246e-0c80-4906-99b9-ce66c5fec152";
const FAMILY_LEASE: &str = "24818296-dbad-48ba-a61e-b0fcc98684aa";

struct ServerFixture {
    home: TempDir,
    personal_root: std::path::PathBuf,
    family_root: std::path::PathBuf,
    personal_ingress: IngressId,
    family_ingress: IngressId,
    port: u16,
    client: ServerClient,
    generation: ServerGeneration,
    family_lease: LeaseId,
    _personal_guard: brain::tui::singleton::Guard,
    _family_guard: brain::tui::singleton::Guard,
    _personal_job_socket: brain::tui::singleton::JobSocket,
    _family_job_socket: brain::tui::singleton::JobSocket,
    child: Child,
}

impl ServerFixture {
    fn new(family_manifest_id: &str) -> Self {
        let home = tempfile::tempdir().expect("temporary home");
        let personal_root = home.path().join("personal");
        let family_root = home.path().join("family");
        let personal_ingress = write_workspace(&personal_root, PERSONAL_ID, "Personal habit");
        let family_ingress = write_workspace(&family_root, family_manifest_id, "Family habit");

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
                        receiver_enabled: true,
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
                        receiver_enabled: true,
                        env: Map::new(),
                    },
                ),
            ]),
        };
        RegistryStore::from_path(home.path().join(".config/brain/env.json"))
            .replace(&registry)
            .expect("write workspace registry");

        let personal_workspace = workspace(home.path(), "personal", PERSONAL_ID, &personal_root);
        let family_workspace = workspace(home.path(), "family", FAMILY_ID, &family_root);
        let personal_guard =
            brain::tui::singleton::Guard::acquire(&personal_workspace).expect("personal TUI");
        let family_guard =
            brain::tui::singleton::Guard::acquire(&family_workspace).expect("family TUI");
        let personal_job_socket =
            brain::tui::singleton::JobSocket::bind(&personal_workspace).expect("personal jobs");
        let family_job_socket =
            brain::tui::singleton::JobSocket::bind(&family_workspace).expect("family jobs");

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
                "0",
            ])
            .env("HOME", home.path())
            .env_remove("XDG_CONFIG_HOME")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start brain server");
        let handoff = election.handoff();
        let client = ServerClient::new(paths);
        let personal_lease = LeaseId::parse(PERSONAL_LEASE).expect("personal lease");
        let family_lease = LeaseId::parse(FAMILY_LEASE).expect("family lease");
        wait_for_registration(
            &client,
            &registration(
                &personal_workspace,
                personal_ingress,
                personal_lease,
                generation,
            ),
        );
        let record = client
            .connect_existing()
            .expect("discover registered shared server");
        handoff.cleanup().expect("finish election handoff");
        client
            .register_generation(&registration(
                &family_workspace,
                family_ingress,
                family_lease,
                generation,
            ))
            .expect("register family TUI");

        Self {
            home,
            personal_root,
            family_root,
            personal_ingress,
            family_ingress,
            port: record.port,
            client,
            generation,
            family_lease,
            _personal_guard: personal_guard,
            _family_guard: family_guard,
            _personal_job_socket: personal_job_socket,
            _family_job_socket: family_job_socket,
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
fn two_live_workspace_routes_render_only_their_own_habits() {
    let server = ServerFixture::new(FAMILY_ID);

    let family = server.get(&format!("/w/{}/habits", server.family_ingress));
    let personal = server.get(&format!("/w/{}/habits", server.personal_ingress));

    assert!(family.starts_with("HTTP/1.1 200"), "{family}");
    assert!(family.contains("Family habit"), "{family}");
    assert!(!family.contains("Personal habit"), "{family}");
    assert!(
        family.contains(&format!("/w/{}/habits/done", server.family_ingress)),
        "the rendered page must preserve its opaque ingress in completion requests"
    );
    assert!(personal.starts_with("HTTP/1.1 200"), "{personal}");
    assert!(personal.contains("Personal habit"), "{personal}");
    assert!(!personal.contains("Family habit"), "{personal}");
    assert!(
        personal.contains(&format!("/w/{}/habits/done", server.personal_ingress)),
        "the rendered personal page must preserve only its ingress"
    );
}

#[test]
fn habits_post_mutates_only_the_workspace_named_by_ingress() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);

    let response = server.post(
        &format!("/w/{}/habits/done", server.family_ingress),
        r#"{"task_id":"H1"}"#,
    );

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
    let family = String::from_utf8(habits_bytes(&server.family_root)).expect("family CSV utf8");
    assert!(family.contains("H1,Family habit,done"), "{family}");
}

#[test]
fn triage_completion_is_recorded_only_for_the_ingress_workspace() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_signal =
        brain::workspace::WorkspacePaths::new(server.home.path(), workspace_id(PERSONAL_ID))
            .cache_dir()
            .join("triage-done.json");
    let family_signal =
        brain::workspace::WorkspacePaths::new(server.home.path(), workspace_id(FAMILY_ID))
            .cache_dir()
            .join("triage-done.json");

    let response = server.post(
        &format!("/w/{}/triage/done", server.family_ingress),
        r#"{"token":"family-triage"}"#,
    );

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(
        brain::triage_signal::read_signal(&workspace(
            server.home.path(),
            "family",
            FAMILY_ID,
            &server.family_root,
        ))
        .expect("family completion signal")
        .token,
        "family-triage"
    );
    assert!(family_signal.is_file());
    assert!(!personal_signal.exists());
}

#[test]
fn global_and_unknown_ingress_routes_never_fall_back_to_default() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);

    for path in ["/habits".to_owned(), format!("/w/{UNKNOWN_ID}/habits")] {
        let get = server.get(&path);
        let post_path = format!("{path}/done");
        let post = server.post(&post_path, r#"{"task_id":"H1"}"#);
        assert!(!get.starts_with("HTTP/1.1 200"), "{path}: {get}");
        assert!(!post.starts_with("HTTP/1.1 200"), "{post_path}: {post}");
        assert!(!get.contains("Personal habit"), "{path}: {get}");
    }
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
}

#[test]
fn habits_requests_reject_a_manifest_identity_mismatch() {
    let server = ServerFixture::new(FAMILY_ID);
    let personal_before = habits_bytes(&server.personal_root);
    let family_before = habits_bytes(&server.family_root);
    std::fs::write(
        server.family_root.join(".config/workspace.json"),
        format!(
            "{{\"schema_version\":1,\"workspace_id\":\"{UNKNOWN_ID}\",\"receiver_ingress_id\":\"{}\",\"minimum_brain_version\":\"0.27.2\"}}\n",
            server.family_ingress
        ),
    )
    .expect("replace family manifest identity");

    let get = server.get(&format!("/w/{}/habits", server.family_ingress));
    let post = server.post(
        &format!("/w/{}/habits/done", server.family_ingress),
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

    let get = server.get(&format!("/w/{}/habits", server.family_ingress));
    let post = server.post(
        &format!("/w/{}/habits/done", server.family_ingress),
        r#"{"task_id":"H1"}"#,
    );

    assert!(!get.starts_with("HTTP/1.1 200"), "{get}");
    assert!(!post.starts_with("HTTP/1.1 200"), "{post}");
    assert_eq!(habits_bytes(&server.personal_root), personal_before);
}

#[test]
fn known_ingress_without_its_live_tui_is_unavailable_while_peer_stays_routable() {
    let server = ServerFixture::new(FAMILY_ID);
    server
        .client
        .unregister_generation(server.generation, server.family_lease)
        .expect("unregister family TUI");

    let family = server.get(&format!("/w/{}/habits", server.family_ingress));
    let personal = server.get(&format!("/w/{}/habits", server.personal_ingress));

    assert!(family.starts_with("HTTP/1.1 503"), "{family}");
    assert!(personal.starts_with("HTTP/1.1 200"), "{personal}");
    assert!(personal.contains("Personal habit"), "{personal}");
}

#[test]
fn receiver_disabled_live_ingress_is_unavailable_while_peer_stays_routable() {
    let server = ServerFixture::new(FAMILY_ID);
    server
        .client
        .update_enabled(server.generation, server.family_lease, false)
        .expect("disable family receiver route");

    let family = server.get(&format!("/w/{}/habits", server.family_ingress));
    let family_triage = server.post(
        &format!("/w/{}/triage/done", server.family_ingress),
        r#"{"token":"must-not-land"}"#,
    );
    let personal = server.get(&format!("/w/{}/habits", server.personal_ingress));

    assert!(family.starts_with("HTTP/1.1 503"), "{family}");
    assert!(family_triage.starts_with("HTTP/1.1 503"), "{family_triage}");
    assert!(personal.starts_with("HTTP/1.1 200"), "{personal}");
    assert!(personal.contains("Personal habit"), "{personal}");
}

#[test]
fn unknown_receiver_ingress_returns_plain_not_found_without_provider_acknowledgement() {
    let server = ServerFixture::new(FAMILY_ID);

    for channel in ["sms", "email"] {
        let response = server.post(&format!("/w/{UNKNOWN_ID}/{channel}"), "provider body");
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
        assert!(!response.contains("Received"), "{response}");
        assert!(!response.contains("queued"), "{response}");
    }
}

fn write_workspace(root: &std::path::Path, manifest_id: &str, habit_name: &str) -> IngressId {
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).expect("tasks directory");
    let manifest = WorkspaceManifest::new(workspace_id(manifest_id));
    let ingress = manifest.receiver_ingress_id().into();
    manifest.write_new(root).expect("workspace manifest");
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
    ingress
}

fn habits_bytes(root: &std::path::Path) -> Vec<u8> {
    std::fs::read(root.join("tasks/habits.csv")).expect("read habits CSV")
}

fn workspace_id(raw: &str) -> WorkspaceId {
    WorkspaceId::parse(raw).expect("valid workspace UUID")
}

fn wait_for_registration(client: &ServerClient, registration: &LeaseRegistration) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_error = None;
    while Instant::now() < deadline {
        match client.register_generation(registration) {
            Ok(_) => return,
            Err(error) => last_error = Some(error),
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("brain server did not accept its first registration: {last_error:?}");
}

fn workspace(
    home: &std::path::Path,
    name: &str,
    id: &str,
    root: &std::path::Path,
) -> Arc<WorkspaceContext> {
    Arc::new(
        WorkspaceContext::new(
            home,
            workspace_id(id),
            WorkspaceName::parse(name).expect("workspace name"),
            root,
            "pablo",
            home,
        )
        .expect("workspace context"),
    )
}

fn registration(
    workspace: &WorkspaceContext,
    ingress_id: IngressId,
    lease_id: LeaseId,
    generation: ServerGeneration,
) -> LeaseRegistration {
    LeaseRegistration {
        generation,
        lease_id,
        workspace_id: workspace.id(),
        canonical_name: workspace.name().to_string(),
        ingress_id,
        tui_pid: std::process::id(),
        resolved_root: workspace.root().to_path_buf(),
        job_socket: workspace.paths().job_socket(),
    }
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

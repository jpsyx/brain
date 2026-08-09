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

pub(super) const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
pub(super) const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
pub(super) const UNKNOWN_ID: &str = "d451ec8f-067f-4d68-9a52-fcefb79faa70";
const PERSONAL_LEASE: &str = "48a6246e-0c80-4906-99b9-ce66c5fec152";
const FAMILY_LEASE: &str = "24818296-dbad-48ba-a61e-b0fcc98684aa";

pub(super) struct ServerFixture {
    pub(super) home: TempDir,
    pub(super) personal_root: std::path::PathBuf,
    pub(super) family_root: std::path::PathBuf,
    pub(super) personal_ingress: IngressId,
    pub(super) family_ingress: IngressId,
    pub(super) port: u16,
    pub(super) client: ServerClient,
    pub(super) generation: ServerGeneration,
    pub(super) family_lease: LeaseId,
    pub(super) personal_lease: LeaseId,
    _personal_guard: brain::tui::singleton::Guard,
    _family_guard: brain::tui::singleton::Guard,
    _personal_job_socket: brain::tui::singleton::JobSocket,
    _family_job_socket: brain::tui::singleton::JobSocket,
    _personal_heartbeat: brain::server::control::HeartbeatWorker,
    _family_heartbeat: brain::server::control::HeartbeatWorker,
    child: Child,
}

impl ServerFixture {
    pub(super) fn new(family_manifest_id: &str) -> Self {
        let home = tempfile::tempdir().expect("temporary home");
        let personal_root = home.path().join("personal");
        let family_root = home.path().join("family");
        let personal_ingress = write_workspace(&personal_root, PERSONAL_ID, "Personal habit");
        let family_ingress = write_workspace(&family_root, family_manifest_id, "Family habit");

        let personal_name = WorkspaceName::parse("personal").expect("personal name");
        let family_name = WorkspaceName::parse("family").expect("family name");
        let registry = MachineRegistry {
            schema_version: brain::workspace::REGISTRY_SCHEMA_VERSION,
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
            env: serde_json::Map::new(),
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
        let personal_registration = registration(
            &personal_workspace,
            personal_ingress,
            personal_lease,
            generation,
        );
        wait_for_registration(&client, &personal_registration);
        let personal_heartbeat =
            brain::server::control::HeartbeatWorker::start(client.clone(), personal_registration);
        let record = client
            .connect_existing()
            .expect("discover registered shared server");
        handoff.cleanup().expect("finish election handoff");
        let family_registration =
            registration(&family_workspace, family_ingress, family_lease, generation);
        client
            .register_generation(&family_registration)
            .expect("register family TUI");
        let family_heartbeat =
            brain::server::control::HeartbeatWorker::start(client.clone(), family_registration);

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
            personal_lease,
            _personal_guard: personal_guard,
            _family_guard: family_guard,
            _personal_job_socket: personal_job_socket,
            _family_job_socket: family_job_socket,
            _personal_heartbeat: personal_heartbeat,
            _family_heartbeat: family_heartbeat,
            child,
        }
    }

    pub(super) fn get(&self, path: &str) -> String {
        request(self.port, "GET", path, "")
    }

    pub(super) fn post(&self, path: &str, body: &str) -> String {
        request(self.port, "POST", path, body)
    }

    pub(super) fn pid(&self) -> u32 {
        self.child.id()
    }

    pub(super) fn disable_family_receiver(&self) {
        self.persist_family_receiver_disabled();
        self.client
            .refresh_enabled_generation(self.generation, workspace_id(FAMILY_ID))
            .expect("refresh disabled family receiver intent");
    }

    pub(super) fn persist_family_receiver_disabled(&self) {
        RegistryStore::from_path(self.home.path().join(".config/brain/env.json"))
            .transition_receiver(
                &WorkspaceName::parse("family").expect("family name"),
                workspace_id(FAMILY_ID),
                brain::workspace::ReceiverAction::Stop,
            )
            .expect("persist disabled family receiver intent");
    }
}

impl Drop for ServerFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn write_workspace(
    root: &std::path::Path,
    manifest_id: &str,
    habit_name: &str,
) -> IngressId {
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

pub(super) fn habits_bytes(root: &std::path::Path) -> Vec<u8> {
    std::fs::read(root.join("tasks/habits.csv")).expect("read habits CSV")
}

pub(super) fn workspace_id(raw: &str) -> WorkspaceId {
    WorkspaceId::parse(raw).expect("valid workspace UUID")
}

pub(super) fn wait_for_registration(client: &ServerClient, registration: &LeaseRegistration) {
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

pub(super) fn workspace(
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

pub(super) fn registration(
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

pub(super) fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to brain server");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("bound HTTP response read");
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .expect("bound HTTP request write");
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

pub(super) fn partial_post(port: u16, path: &str, advertised_length: usize) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to brain server");
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .expect("bound partial response read");
    stream
        .write_all(
            format!(
                "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {advertised_length}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .expect("write partial HTTP request");
    stream
}

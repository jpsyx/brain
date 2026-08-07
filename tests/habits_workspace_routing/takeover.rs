//! A habits page opened by `brain habits` must survive a TUI starting after
//! it. The browser holds a URL carrying the browser-only lease's capability
//! and cannot learn that a TUI replaced that lease.

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use brain::server::control::{LeaseRegistration, ServerClient};
use brain::server::lifecycle::{ElectionGuard, IngressId, LeaseId, ServerGeneration, ServerPaths};
use brain::workspace::{MachineRegistry, RegistryStore, WorkspaceName, WorkspaceRecord};
use serde_json::Map;
use tempfile::TempDir;

use super::support::{
    FAMILY_ID, registration, request, wait_for_registration, workspace, workspace_id,
    write_workspace,
};

const BACKGROUND_LEASE: &str = "0b1d2d5c-1a8b-4a2f-9a1e-4f0d5b1c7a11";
const TUI_LEASE: &str = "3c9f6a20-6d64-4f3d-9d4e-2b0a7c5e8d22";
const STRANGER_LEASE: &str = "9e7c1f38-52b3-4c1a-8f6d-1a2b3c4d5e66";

#[test]
fn an_open_habits_page_survives_a_tui_taking_over_the_background_lease() {
    let fixture = BackgroundFixture::start();
    let background = LeaseId::parse(BACKGROUND_LEASE).expect("background lease");

    assert!(
        fixture.get(background, "habits").contains("200 OK"),
        "the page must load under the browser-only lease"
    );

    let _tui = fixture.register_tui();

    let page = fixture.get(background, "habits");
    assert!(
        page.contains("200 OK"),
        "the already-open page must keep loading after the takeover; got:\n{page}"
    );
    let done = fixture.post(background, "habits/done", r#"{"task_id":"H1"}"#);
    assert!(
        done.contains("200 OK"),
        "the already-open page must keep marking habits done; got:\n{done}"
    );

    let stranger = LeaseId::parse(STRANGER_LEASE).expect("stranger lease");
    assert!(
        fixture.get(stranger, "habits").contains("404"),
        "a capability that never owned the ingress must stay unroutable"
    );
    assert!(
        fixture
            .get(LeaseId::parse(TUI_LEASE).unwrap(), "habits")
            .contains("200 OK"),
        "the live lease's own capability still routes"
    );
}

struct BackgroundFixture {
    home: TempDir,
    root: std::path::PathBuf,
    ingress: IngressId,
    port: u16,
    client: ServerClient,
    generation: ServerGeneration,
    child: Child,
}

/// The TUI-side artifacts a live registration is validated against.
struct TuiRegistration {
    _guard: brain::tui::singleton::Guard,
    _job_socket: brain::tui::singleton::JobSocket,
}

impl BackgroundFixture {
    fn start() -> Self {
        let home = tempfile::tempdir().expect("temporary home");
        let root = home.path().join("family");
        let ingress = write_workspace(&root, FAMILY_ID, "Family habit");
        let name = WorkspaceName::parse("family").expect("family name");
        let registry = MachineRegistry {
            schema_version: 2,
            default_workspace: name.clone(),
            workspaces: BTreeMap::from([(
                name,
                WorkspaceRecord {
                    workspace_id: workspace_id(FAMILY_ID),
                    root: root.clone(),
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: true,
                    env: Map::new(),
                },
            )]),
        };
        RegistryStore::from_path(home.path().join(".config/brain/env.json"))
            .replace(&registry)
            .expect("write workspace registry");

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
        let background = LeaseRegistration {
            generation,
            lease_id: LeaseId::parse(BACKGROUND_LEASE).expect("background lease"),
            workspace_id: workspace_id(FAMILY_ID),
            canonical_name: "family".to_owned(),
            ingress_id: ingress,
            tui_pid: 0,
            resolved_root: root.clone(),
            job_socket: std::path::PathBuf::new(),
        };
        wait_for_background_start(&client, &background);
        let record = client
            .connect_existing()
            .expect("discover the background shared server");
        handoff.cleanup().expect("finish election handoff");

        Self {
            home,
            root,
            ingress,
            port: record.port,
            client,
            generation,
            child,
        }
    }

    /// Register a real TUI lease for the same workspace, exactly as starting
    /// `brain` while a habits page is open does.
    fn register_tui(&self) -> TuiRegistration {
        let workspace = workspace(self.home.path(), "family", FAMILY_ID, &self.root);
        let guard = brain::tui::singleton::Guard::acquire(&workspace).expect("family TUI");
        let job_socket = brain::tui::singleton::JobSocket::bind(&workspace).expect("family jobs");
        let tui = registration(
            &workspace,
            self.ingress,
            LeaseId::parse(TUI_LEASE).expect("TUI lease"),
            self.generation,
        );
        wait_for_registration(&self.client, &tui);
        TuiRegistration {
            _guard: guard,
            _job_socket: job_socket,
        }
    }

    fn get(&self, capability: LeaseId, suffix: &str) -> String {
        request(self.port, "GET", &self.path(capability, suffix), "")
    }

    fn post(&self, capability: LeaseId, suffix: &str, body: &str) -> String {
        request(self.port, "POST", &self.path(capability, suffix), body)
    }

    fn path(&self, capability: LeaseId, suffix: &str) -> String {
        format!("/local/{capability}/w/{}/{suffix}", self.ingress)
    }
}

impl Drop for BackgroundFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for_background_start(client: &ServerClient, registration: &LeaseRegistration) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_error = None;
    while Instant::now() < deadline {
        match client.start_background(registration) {
            Ok(_) => return,
            Err(error) => last_error = Some(error),
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("brain server did not accept the background lease: {last_error:?}");
}

use brain::server::control::{LeaseRegistration, ServerClient};
use brain::server::lifecycle::{ElectionGuard, IngressId, LeaseId, ServerGeneration, ServerPaths};
use brain::workspace::{WorkspaceId, WorkspaceName};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PROCESS_FIXTURE_LIMIT: usize = 2;

pub(super) static PROCESS_FIXTURE_PERMITS: ProcessFixturePermits =
    ProcessFixturePermits::new(PROCESS_FIXTURE_LIMIT);

pub(super) struct ProcessFixturePermits {
    limit: usize,
    active: std::sync::Mutex<usize>,
    available: std::sync::Condvar,
}

impl ProcessFixturePermits {
    pub(super) const fn new(limit: usize) -> Self {
        Self {
            limit,
            active: std::sync::Mutex::new(0),
            available: std::sync::Condvar::new(),
        }
    }

    pub(super) fn acquire(&self) -> ProcessFixturePermit<'_> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *active == self.limit {
            active = self
                .available
                .wait(active)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *active += 1;
        drop(active);
        ProcessFixturePermit { permits: self }
    }

    pub(super) fn try_acquire(&self) -> Option<ProcessFixturePermit<'_>> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *active == self.limit {
            drop(active);
            None
        } else {
            *active += 1;
            drop(active);
            Some(ProcessFixturePermit { permits: self })
        }
    }
}

pub(super) struct ProcessFixturePermit<'a> {
    permits: &'a ProcessFixturePermits,
}

impl Drop for ProcessFixturePermit<'_> {
    fn drop(&mut self) {
        let mut active = self
            .permits
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = active.checked_sub(1).expect("fixture permit is active");
        drop(active);
        self.permits.available.notify_one();
    }
}

pub(super) struct LiveTui {
    workspace_id: WorkspaceId,
    canonical_name: WorkspaceName,
    ingress_id: IngressId,
    pub(super) lease_id: LeaseId,
    root: std::path::PathBuf,
    _guard: brain::tui::singleton::Guard,
}

impl LiveTui {
    pub(super) fn new(
        home: &std::path::Path,
        name: &str,
        workspace_id: &str,
        ingress_id: &str,
        lease_id: &str,
    ) -> Self {
        let workspace_id = WorkspaceId::parse(workspace_id).expect("valid workspace ID");
        let canonical_name = WorkspaceName::parse(name).expect("valid workspace name");
        let root = home.join(name);
        let workspace = brain::workspace::WorkspaceContext::new(
            home,
            workspace_id,
            canonical_name.clone(),
            &root,
            "tester",
            home,
        )
        .expect("workspace context");
        let guard = brain::tui::singleton::Guard::acquire(&workspace).expect("TUI singleton");
        Self {
            workspace_id,
            canonical_name,
            ingress_id: IngressId::parse(ingress_id).expect("valid ingress ID"),
            lease_id: LeaseId::parse(lease_id).expect("valid lease ID"),
            root,
            _guard: guard,
        }
    }

    pub(super) fn registration(&self, generation: ServerGeneration) -> LeaseRegistration {
        LeaseRegistration {
            generation,
            lease_id: self.lease_id,
            workspace_id: self.workspace_id,
            canonical_name: self.canonical_name.to_string(),
            ingress_id: self.ingress_id,
            tui_pid: std::process::id(),
            resolved_root: self.root.clone(),
        }
    }
}

pub(super) struct RunningServer {
    pub(super) child: Child,
    _process_permit: ProcessFixturePermit<'static>,
    home: tempfile::TempDir,
    pub(super) paths: ServerPaths,
    pub(super) client: ServerClient,
    pub(super) generation: ServerGeneration,
}

impl RunningServer {
    pub(super) fn start() -> Self {
        let process_permit = PROCESS_FIXTURE_PERMITS.acquire();
        let home = tempfile::tempdir().expect("temporary server home");
        prepare_workspace_registry(home.path());
        let paths = ServerPaths::from_home(home.path());
        let generation = ServerGeneration::new();
        let election = ElectionGuard::try_acquire(&paths, generation)
            .expect("election probe")
            .expect("test process wins election");
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
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn hidden server");
        let handoff = election.handoff();
        let client = ServerClient::with_launch_context(
            paths.clone(),
            std::path::PathBuf::from(env!("CARGO_BIN_EXE_brain")),
            home.path().to_path_buf(),
        );
        wait_for("shared server reachability", || {
            client.connect_existing().is_ok()
        });
        handoff.cleanup().expect("finish election handoff");
        Self {
            child,
            _process_permit: process_permit,
            home,
            paths,
            client,
            generation,
        }
    }

    pub(super) fn home(&self) -> &std::path::Path {
        self.home.path()
    }

    pub(super) fn shutdown_with_two_leases(&mut self) {
        let family = LiveTui::new(
            self.home(),
            "family",
            "e806258e-491a-436d-9db4-a5ca9903e0d4",
            "57b162df-983a-45c3-ac7e-bad94eb27a99",
            "00000000-0000-0000-0000-000000000003",
        );
        let personal = LiveTui::new(
            self.home(),
            "personal",
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
            "00000000-0000-0000-0000-000000000004",
        );
        self.client
            .register_generation(&family.registration(self.generation))
            .expect("register family");
        self.client
            .register_generation(&personal.registration(self.generation))
            .expect("register personal");
        self.client
            .unregister(family.lease_id)
            .expect("unregister family");
        self.client
            .unregister(personal.lease_id)
            .expect("unregister personal");
        wait_for("shared server process exit", || {
            self.child.try_wait().ok().flatten().is_some()
        });
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub(super) fn wait_for(description: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn prepare_workspace_registry(home: &std::path::Path) {
    let config = home.join(".config/brain");
    let family = home.join("family");
    let personal = home.join("personal");
    std::fs::create_dir_all(&config).expect("machine config");
    for (root, workspace_id, ingress_id) in [
        (
            &family,
            "e806258e-491a-436d-9db4-a5ca9903e0d4",
            "57b162df-983a-45c3-ac7e-bad94eb27a99",
        ),
        (
            &personal,
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
        ),
    ] {
        std::fs::create_dir_all(root.join(".config")).expect("workspace config");
        let manifest = serde_json::json!({
            "schema_version": 1,
            "workspace_id": workspace_id,
            "receiver_ingress_id": ingress_id,
            "minimum_brain_version": env!("CARGO_PKG_VERSION")
        });
        std::fs::write(
            root.join(".config/workspace.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON"),
        )
        .expect("workspace manifest");
    }
    let registry = serde_json::json!({
        "schema_version": brain::workspace::REGISTRY_SCHEMA_VERSION,
        "default_workspace": "personal",
        "workspaces": {
            "family": {
                "workspace_id": "e806258e-491a-436d-9db4-a5ca9903e0d4",
                "root": family,
                "aliases": [],
                "local_user_id": "tester",
                "receiver_enabled": true,
                "env": {}
            },
            "personal": {
                "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
                "root": personal,
                "aliases": [],
                "local_user_id": "tester",
                "receiver_enabled": true,
                "env": {}
            }
        }
    });
    std::fs::write(
        config.join("env.json"),
        serde_json::to_vec_pretty(&registry).expect("registry JSON"),
    )
    .expect("machine registry");
}

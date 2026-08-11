use brain::server::control::LeaseRegistration;
use brain::server::lifecycle::{IngressId, LeaseId, ServerGeneration};
use brain::workspace::{RegistryStore, WorkspaceId, WorkspaceManifest};
use std::path::PathBuf;

pub(super) fn generation() -> ServerGeneration {
    ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("generation")
}

pub(super) fn stale_generation() -> ServerGeneration {
    ServerGeneration::parse("b5487d2a-2625-49a4-b5f1-fd929ff5bd80").expect("generation")
}

pub(super) fn lease_id() -> LeaseId {
    LeaseId::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5").expect("lease ID")
}

pub(super) fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse("2174fb9d-ae76-4bde-a526-38ac43ebdf8f").expect("workspace ID")
}

pub(super) fn ingress_id() -> IngressId {
    IngressId::parse("a4f0ec11-d121-4f58-aa44-2448ba427b76").expect("ingress ID")
}

pub(super) struct ControlFixture {
    pub(super) temporary: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) ingress_id: IngressId,
    _guard: brain::tui::singleton::Guard,
    job_socket: Option<brain::tui::singleton::JobSocket>,
}

impl ControlFixture {
    pub(super) fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary control fixture");
        let root = temporary.path().join("workspace");
        let manifest = WorkspaceManifest::new(workspace_id());
        manifest.write_new(&root).expect("workspace manifest");
        let registry = serde_json::json!({
            "schema_version": brain::workspace::REGISTRY_SCHEMA_VERSION,
            "default_workspace": "personal",
            "workspaces": {
                "personal": {
                    "workspace_id": workspace_id(),
                    "root": root,
                    "aliases": [],
                    "local_user_id": "tester",
                    "receiver_enabled": true,
                    "env": {}
                }
            }
        });
        std::fs::write(
            temporary.path().join("env.json"),
            serde_json::to_vec_pretty(&registry).expect("registry JSON"),
        )
        .expect("registry");
        let workspace = brain::workspace::WorkspaceContext::new(
            temporary.path(),
            workspace_id(),
            brain::workspace::WorkspaceName::parse("personal").expect("workspace name"),
            &root,
            "tester",
            temporary.path(),
        )
        .expect("workspace context");
        let guard = brain::tui::singleton::Guard::acquire(&workspace).expect("TUI singleton");
        let job_socket = brain::tui::singleton::JobSocket::bind(&workspace).expect("job socket");
        Self {
            temporary,
            root,
            ingress_id: manifest.receiver_ingress_id().into(),
            _guard: guard,
            job_socket: Some(job_socket),
        }
    }

    pub(super) fn registry_store(&self) -> RegistryStore {
        RegistryStore::from_path(self.temporary.path().join("env.json"))
    }

    pub(super) fn registration(&self) -> LeaseRegistration {
        LeaseRegistration {
            generation: generation(),
            lease_id: lease_id(),
            workspace_id: workspace_id(),
            canonical_name: "personal".to_owned(),
            ingress_id: self.ingress_id,
            tui_pid: std::process::id(),
            resolved_root: self.root.clone(),
            job_socket: brain::workspace::WorkspacePaths::new(
                self.temporary.path(),
                workspace_id(),
            )
            .job_socket(),
        }
    }

    pub(super) fn close_job_socket(&mut self) {
        drop(self.job_socket.take());
    }
}

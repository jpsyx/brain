use std::collections::{BTreeMap, BTreeSet};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use brain::server::receiver::InboundJob;
use brain::tui::singleton::JobSocket;
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

use super::provider_request::{post, signed_sms};
use super::{FAMILY_ID, PERSONAL_ID, poll_until};

pub struct DualWorkspaceReceiverFixture {
    home: tempfile::TempDir,
    pub personal: WorkspaceContext,
    pub family: WorkspaceContext,
    personal_ingress: brain::server::IngressId,
    family_ingress: brain::server::IngressId,
    personal_socket: JobSocket,
    family_socket: JobSocket,
    _personal_guard: brain::tui::singleton::Guard,
    _family_guard: brain::tui::singleton::Guard,
    client: brain::server::control::ServerClient,
    generation: brain::server::lifecycle::ServerGeneration,
    personal_lease: brain::server::lifecycle::LeaseId,
    family_lease: brain::server::lifecycle::LeaseId,
    child: Child,
    port: u16,
    registered: bool,
}

impl DualWorkspaceReceiverFixture {
    pub fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let (personal, personal_record, personal_ingress) = workspace(
            &home,
            PERSONAL_ID,
            "personal",
            "personal-member",
            "personal-token",
        );
        let (family, family_record, family_ingress) =
            workspace(&home, FAMILY_ID, "family", "family-member", "family-token");
        let personal_name = personal.name().clone();
        let family_name = family.name().clone();
        let registry = brain::workspace::MachineRegistry {
            schema_version: 2,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (personal_name, personal_record),
                (family_name, family_record),
            ]),
        };
        let store =
            brain::workspace::RegistryStore::from_path(home.path().join(".config/brain/env.json"));
        store.replace(&registry).unwrap();
        save_user(&personal, "personal-member");
        save_user(&family, "family-member");

        let personal_guard = brain::tui::singleton::Guard::acquire(&personal).unwrap();
        let personal_socket = JobSocket::bind(&personal).unwrap();
        let family_guard = brain::tui::singleton::Guard::acquire(&family).unwrap();
        let family_socket = JobSocket::bind(&family).unwrap();
        let paths = brain::server::lifecycle::ServerPaths::from_home(home.path());
        let generation = brain::server::lifecycle::ServerGeneration::new();
        let election = brain::server::lifecycle::ElectionGuard::try_acquire(&paths, generation)
            .unwrap()
            .unwrap();
        let child = spawn_server(&home, generation);
        let handoff = election.handoff();
        let client = brain::server::control::ServerClient::new(paths);
        let record = poll_value(Instant::now() + Duration::from_secs(3), || {
            client.connect_existing().ok()
        });
        handoff.cleanup().unwrap();
        let personal_lease = register(&client, generation, &personal, personal_ingress);
        let family_lease = register(&client, generation, &family, family_ingress);

        Self {
            home,
            personal,
            family,
            personal_ingress,
            family_ingress,
            personal_socket,
            family_socket,
            _personal_guard: personal_guard,
            _family_guard: family_guard,
            client,
            generation,
            personal_lease,
            family_lease,
            child,
            port: record.port,
            registered: true,
        }
    }

    pub fn post_personal_signed_with_family_credentials(&self) -> String {
        self.post_sms(
            self.personal_ingress,
            "family-token",
            "SM-swapped",
            "must reject swapped route",
        )
    }

    pub fn post_personal_async(
        &self,
        provider_id: &str,
        prompt: &str,
    ) -> std::sync::mpsc::Receiver<String> {
        self.post_sms_async(self.personal_ingress, "personal-token", provider_id, prompt)
    }

    pub fn post_family_async(
        &self,
        provider_id: &str,
        prompt: &str,
    ) -> std::sync::mpsc::Receiver<String> {
        self.post_sms_async(self.family_ingress, "family-token", provider_id, prompt)
    }

    pub fn personal_jobs(&self) -> Vec<InboundJob> {
        let mut jobs = Vec::new();
        self.personal_socket
            .poll_jobs(self.personal.id(), &mut jobs);
        jobs
    }

    pub fn family_jobs(&self) -> Vec<InboundJob> {
        let mut jobs = Vec::new();
        self.family_socket.poll_jobs(self.family.id(), &mut jobs);
        jobs
    }

    pub fn poll_both_jobs(&self) -> (Vec<InboundJob>, Vec<InboundJob>) {
        let mut personal = Vec::new();
        let mut family = Vec::new();
        poll_until(Instant::now() + Duration::from_secs(3), || {
            self.personal_socket
                .poll_jobs(self.personal.id(), &mut personal);
            self.family_socket.poll_jobs(self.family.id(), &mut family);
            !personal.is_empty() && !family.is_empty()
        });
        (personal, family)
    }

    pub fn shutdown(&mut self) {
        if self.registered {
            self.client
                .unregister_generation(self.generation, self.personal_lease)
                .unwrap();
            self.client
                .unregister_generation(self.generation, self.family_lease)
                .unwrap();
            self.registered = false;
        }
        poll_until(Instant::now() + Duration::from_secs(3), || {
            self.child.try_wait().ok().flatten().is_some()
        });
    }

    fn post_sms_async(
        &self,
        ingress: brain::server::IngressId,
        token: &str,
        provider_id: &str,
        prompt: &str,
    ) -> std::sync::mpsc::Receiver<String> {
        let port = self.port;
        let request = signed_sms(ingress, token, provider_id, prompt, "+12125550100");
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || tx.send(post(port, &request)).unwrap());
        rx
    }

    fn post_sms(
        &self,
        ingress: brain::server::IngressId,
        token: &str,
        provider_id: &str,
        prompt: &str,
    ) -> String {
        post(
            self.port,
            &signed_sms(ingress, token, provider_id, prompt, "+12125550100"),
        )
    }
}

impl Drop for DualWorkspaceReceiverFixture {
    fn drop(&mut self) {
        if self.registered {
            let _ = self
                .client
                .unregister_generation(self.generation, self.personal_lease);
            let _ = self
                .client
                .unregister_generation(self.generation, self.family_lease);
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = self.home.path();
    }
}

fn workspace(
    home: &tempfile::TempDir,
    id: &str,
    name: &str,
    user_id: &str,
    token: &str,
) -> (
    WorkspaceContext,
    brain::workspace::WorkspaceRecord,
    brain::server::IngressId,
) {
    let root = home.path().join(name);
    let workspace_id = WorkspaceId::parse(id).unwrap();
    let manifest = brain::workspace::WorkspaceManifest::new(workspace_id);
    let ingress = manifest.receiver_ingress_id().into();
    manifest.write_new(&root).unwrap();
    let workspace_name = WorkspaceName::parse(name).unwrap();
    let context = WorkspaceContext::new(
        home.path(),
        workspace_id,
        workspace_name,
        &root,
        user_id,
        home.path(),
    )
    .unwrap();
    let record = brain::workspace::WorkspaceRecord {
        workspace_id,
        root,
        aliases: BTreeSet::new(),
        local_user_id: user_id.to_owned(),
        receiver_enabled: true,
        env: serde_json::Map::from_iter([
            ("twilio_auth_token".to_owned(), serde_json::json!(token)),
            (
                "brain_receiver_public_url".to_owned(),
                serde_json::json!("https://receiver.example.test"),
            ),
        ]),
    };
    (context, record, ingress)
}

fn save_user(workspace: &WorkspaceContext, user_id: &str) {
    brain::users::UsersStore::save(
        workspace,
        &brain::users::Users {
            schema_version: brain::users::USERS_SCHEMA_VERSION,
            users: vec![brain::users::User {
                id: brain::users::UserId::parse(user_id).unwrap(),
                name: user_id.to_owned(),
                phones: vec![brain::users::PhoneIdentity {
                    value: "+12125550100".to_owned(),
                    inbound_allowed: true,
                }],
                emails: Vec::new(),
                response_email: None,
            }],
        },
    )
    .unwrap();
}

fn spawn_server(
    home: &tempfile::TempDir,
    generation: brain::server::lifecycle::ServerGeneration,
) -> Child {
    Command::new(env!("CARGO_BIN_EXE_brain"))
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
        .unwrap()
}

fn register(
    client: &brain::server::control::ServerClient,
    generation: brain::server::lifecycle::ServerGeneration,
    workspace: &WorkspaceContext,
    ingress_id: brain::server::IngressId,
) -> brain::server::lifecycle::LeaseId {
    let lease_id = brain::server::lifecycle::LeaseId::new();
    client
        .register_generation(&brain::server::control::LeaseRegistration {
            generation,
            lease_id,
            workspace_id: workspace.id(),
            canonical_name: workspace.name().as_str().to_owned(),
            ingress_id,
            tui_pid: std::process::id(),
            resolved_root: workspace.root().to_path_buf(),
            job_socket: workspace.paths().job_socket(),
        })
        .unwrap();
    lease_id
}

fn poll_value<T>(deadline: Instant, mut value: impl FnMut() -> Option<T>) -> T {
    loop {
        if let Some(value) = value() {
            return value;
        }
        assert!(Instant::now() < deadline, "value was not produced");
        std::thread::yield_now();
    }
}

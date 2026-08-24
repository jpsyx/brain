use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use brain::server::receiver::InboundJob;
use brain::tui::singleton::JobSocket;
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

use super::provider_request::{FAMILY_PHONE, PERSONAL_PHONE, PUBLIC_URL, post, signed_sms};
use super::{FAMILY_ID, PERSONAL_ID, ProcessFixtureProcess, durable_jobs, poll_until};

pub struct DualWorkspaceReceiverFixture {
    home: tempfile::TempDir,
    pub personal: WorkspaceContext,
    pub family: WorkspaceContext,
    personal_socket: Option<JobSocket>,
    family_socket: Option<JobSocket>,
    personal_guard: Option<brain::tui::singleton::Guard>,
    family_guard: Option<brain::tui::singleton::Guard>,
    client: brain::server::control::ServerClient,
    personal_heartbeat: Option<brain::server::control::HeartbeatWorker>,
    family_heartbeat: Option<brain::server::control::HeartbeatWorker>,
    process: ProcessFixtureProcess,
    port: u16,
    personal_registered: bool,
    family_registered: bool,
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
            PERSONAL_PHONE,
        );
        let (family, family_record, family_ingress) = workspace(
            &home,
            FAMILY_ID,
            "family",
            "family-member",
            "family-token",
            FAMILY_PHONE,
        );
        let personal_name = personal.name().clone();
        let family_name = family.name().clone();
        let registry = brain::workspace::MachineRegistry {
            schema_version: brain::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (personal_name, personal_record),
                (family_name, family_record),
            ]),
            // One machine, one public origin: the URL is the same for both
            // workspaces, and their numbers are what tell them apart.
            env: serde_json::Map::from_iter([(
                "brain_receiver_public_url".to_owned(),
                serde_json::json!(PUBLIC_URL),
            )]),
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
        let process = ProcessFixtureProcess::spawn(&home, generation);
        let handoff = election.handoff();
        let client = brain::server::control::ServerClient::new(paths);
        let record = poll_value(Instant::now() + Duration::from_secs(3), || {
            client.connect_existing().ok()
        });
        handoff.cleanup().unwrap();
        let personal_heartbeat = register(&client, generation, &personal, personal_ingress);
        let family_heartbeat = register(&client, generation, &family, family_ingress);

        Self {
            home,
            personal,
            family,
            personal_socket: Some(personal_socket),
            family_socket: Some(family_socket),
            personal_guard: Some(personal_guard),
            family_guard: Some(family_guard),
            client,
            personal_heartbeat: Some(personal_heartbeat),
            family_heartbeat: Some(family_heartbeat),
            process,
            port: record.port,
            personal_registered: true,
            family_registered: true,
        }
    }

    #[allow(dead_code)]
    pub fn home(&self) -> &std::path::Path {
        self.home.path()
    }

    pub fn post_personal_signed_with_family_credentials(&self) -> String {
        self.post_sms(
            PERSONAL_PHONE,
            "family-token",
            "SM-swapped",
            "must reject swapped route",
        )
    }

    /// A message addressed to family, signed with personal's Twilio token.
    ///
    /// One URL serves both workspaces, so the only thing that decides whose
    /// credential authenticates a request is the number it names.
    pub fn post_family_signed_with_personal_credentials(&self) -> String {
        self.post_sms(
            FAMILY_PHONE,
            "personal-token",
            "SM-crossed",
            "must reject a peer's credential",
        )
    }

    pub fn post_personal_async(
        &self,
        provider_id: &str,
        prompt: &str,
    ) -> std::sync::mpsc::Receiver<String> {
        self.post_sms_async(PERSONAL_PHONE, "personal-token", provider_id, prompt)
    }

    pub fn post_family_async(
        &self,
        provider_id: &str,
        prompt: &str,
    ) -> std::sync::mpsc::Receiver<String> {
        self.post_sms_async(FAMILY_PHONE, "family-token", provider_id, prompt)
    }

    pub fn post_family(&self, provider_id: &str, prompt: &str) -> String {
        self.post_sms(FAMILY_PHONE, "family-token", provider_id, prompt)
    }

    pub fn personal_jobs(&self) -> Vec<InboundJob> {
        durable_jobs(&self.personal)
    }

    pub fn family_jobs(&self) -> Vec<InboundJob> {
        durable_jobs(&self.family)
    }

    pub fn poll_both_jobs(&self) -> (Vec<InboundJob>, Vec<InboundJob>) {
        poll_until(Instant::now() + Duration::from_secs(3), || {
            !durable_jobs(&self.personal).is_empty() && !durable_jobs(&self.family).is_empty()
        });
        (durable_jobs(&self.personal), durable_jobs(&self.family))
    }

    pub fn poll_personal_jobs(&self, expected: usize) -> Vec<InboundJob> {
        poll_until(Instant::now() + Duration::from_secs(3), || {
            durable_jobs(&self.personal).len() >= expected
        });
        durable_jobs(&self.personal)
    }

    pub fn shutdown(&mut self) {
        self.close_family_tui();
        self.close_personal_tui();
        self.wait_for_server_exit();
    }

    pub fn close_family_tui(&mut self) {
        if self.family_registered {
            self.family_heartbeat
                .as_mut()
                .expect("family heartbeat")
                .shutdown()
                .unwrap();
            self.family_heartbeat = None;
            self.family_registered = false;
        }
        self.family_socket.take();
        self.family_guard.take();
    }

    pub fn close_personal_tui(&mut self) {
        if self.personal_registered {
            self.personal_heartbeat
                .as_mut()
                .expect("personal heartbeat")
                .shutdown()
                .unwrap();
            self.personal_heartbeat = None;
            self.personal_registered = false;
        }
        self.personal_socket.take();
        self.personal_guard.take();
    }

    pub fn server_snapshot(&self) -> brain::server::control::ServerSnapshot {
        self.client.snapshot().unwrap().1
    }

    pub fn server_is_running(&self) -> bool {
        self.client.connect_existing().is_ok()
    }

    pub fn server_state_exists(&self) -> bool {
        self.client.paths().process_record().exists()
            || self.client.paths().control_socket().exists()
    }

    pub fn wait_for_server_exit(&mut self) {
        poll_until(Instant::now() + Duration::from_secs(3), || {
            self.process.has_exited()
        });
    }

    fn post_sms_async(
        &self,
        destination: &str,
        token: &str,
        provider_id: &str,
        prompt: &str,
    ) -> std::sync::mpsc::Receiver<String> {
        let port = self.port;
        let request = signed_sms(destination, token, provider_id, prompt, "+12125550100");
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || tx.send(post(port, &request)).unwrap());
        rx
    }

    fn post_sms(&self, destination: &str, token: &str, provider_id: &str, prompt: &str) -> String {
        post(
            self.port,
            &signed_sms(destination, token, provider_id, prompt, "+12125550100"),
        )
    }
}

impl Drop for DualWorkspaceReceiverFixture {
    fn drop(&mut self) {
        if self.personal_registered {
            if let Some(heartbeat) = self.personal_heartbeat.as_mut() {
                let _ = heartbeat.shutdown();
            }
        }
        if self.family_registered {
            if let Some(heartbeat) = self.family_heartbeat.as_mut() {
                let _ = heartbeat.shutdown();
            }
        }
        self.process.terminate();
        let _ = self.home.path();
    }
}

fn workspace(
    home: &tempfile::TempDir,
    id: &str,
    name: &str,
    user_id: &str,
    token: &str,
    phone: &str,
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
            ("twilio_from_number".to_owned(), serde_json::json!(phone)),
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

mod server;

use server::{poll_value, register};

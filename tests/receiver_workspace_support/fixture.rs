use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use brain::tui::singleton::JobSocket;
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

use super::provider_request::{post, signed_email_event, signed_sms};
use super::{FAMILY_ID, PERSONAL_ID, poll_until};

pub struct SharedReceiverFixture {
    home: tempfile::TempDir,
    pub workspace: WorkspaceContext,
    pub ingress: brain::server::IngressId,
    pub socket: JobSocket,
    _guard: brain::tui::singleton::Guard,
    client: brain::server::control::ServerClient,
    generation: brain::server::lifecycle::ServerGeneration,
    lease_id: brain::server::lifecycle::LeaseId,
    target_registered: bool,
    anchor: Option<AnchorLease>,
    child: Child,
    pub port: u16,
}

struct AnchorLease {
    lease_id: brain::server::lifecycle::LeaseId,
    _guard: brain::tui::singleton::Guard,
    _socket: JobSocket,
}

impl SharedReceiverFixture {
    pub fn start() -> Self {
        Self::start_inner(false)
    }

    pub fn start_with_anchor() -> Self {
        Self::start_inner(true)
    }

    fn start_inner(with_anchor: bool) -> Self {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("personal");
        let workspace_id = WorkspaceId::parse(PERSONAL_ID).unwrap();
        let manifest = brain::workspace::WorkspaceManifest::new(workspace_id);
        let ingress = manifest.receiver_ingress_id().into();
        manifest.write_new(&root).unwrap();
        let workspace = WorkspaceContext::new(
            home.path(),
            workspace_id,
            WorkspaceName::parse("personal").unwrap(),
            &root,
            "personal-member",
            home.path(),
        )
        .unwrap();
        save_personal_user(&workspace);
        let name = WorkspaceName::parse("personal").unwrap();
        let mut workspaces = BTreeMap::from([(
            name.clone(),
            brain::workspace::WorkspaceRecord {
                workspace_id,
                root,
                aliases: BTreeSet::new(),
                local_user_id: "personal-member".to_owned(),
                receiver_enabled: true,
                env: serde_json::Map::from_iter([
                    (
                        "twilio_auth_token".to_owned(),
                        serde_json::json!("personal-token"),
                    ),
                    (
                        "brain_receiver_public_url".to_owned(),
                        serde_json::json!("https://receiver.example.test"),
                    ),
                    (
                        "resend_webhook_signing_secret".to_owned(),
                        serde_json::json!(format!(
                            "whsec_{}",
                            STANDARD.encode(b"personal-resend-secret")
                        )),
                    ),
                    (
                        "resend_api_key".to_owned(),
                        serde_json::json!("personal-resend-key"),
                    ),
                ]),
            },
        )]);
        let anchor_workspace = with_anchor.then(|| make_anchor_workspace(&home, &mut workspaces));
        let registry = brain::workspace::MachineRegistry {
            schema_version: 2,
            default_workspace: name,
            workspaces,
        };
        let store =
            brain::workspace::RegistryStore::from_path(home.path().join(".config/brain/env.json"));
        store.replace(&registry).unwrap();
        let guard = brain::tui::singleton::Guard::acquire(&workspace).unwrap();
        let socket = JobSocket::bind(&workspace).unwrap();
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
        let lease_id = register_workspace(&client, generation, &workspace, ingress);
        let anchor = anchor_workspace.map(|workspace| {
            let guard = brain::tui::singleton::Guard::acquire(&workspace).unwrap();
            let socket = JobSocket::bind(&workspace).unwrap();
            let lease_id = register_workspace(
                &client,
                generation,
                &workspace,
                brain::server::workspace_ingress(&workspace).unwrap(),
            );
            AnchorLease {
                lease_id,
                _guard: guard,
                _socket: socket,
            }
        });
        Self {
            home,
            workspace,
            ingress,
            socket,
            _guard: guard,
            client,
            generation,
            lease_id,
            target_registered: true,
            anchor,
            child,
            port: record.port,
        }
    }

    pub fn disable_target(&self) {
        self.client
            .update_enabled(self.generation, self.lease_id, false)
            .unwrap();
    }

    pub fn unregister_target(&mut self) {
        self.client
            .unregister_generation(self.generation, self.lease_id)
            .unwrap();
        self.target_registered = false;
    }

    pub fn post_sms(&self, provider_id: &str, prompt: &str) -> String {
        post(
            self.port,
            &signed_sms(
                self.ingress,
                "personal-token",
                provider_id,
                prompt,
                "+12125550100",
            ),
        )
    }

    pub fn post_sms_from(&self, provider_id: &str, prompt: &str, sender: &str) -> String {
        post(
            self.port,
            &signed_sms(self.ingress, "personal-token", provider_id, prompt, sender),
        )
    }

    pub fn post_sms_async(
        &self,
        provider_id: &str,
        prompt: &str,
    ) -> std::sync::mpsc::Receiver<String> {
        let request = signed_sms(
            self.ingress,
            "personal-token",
            provider_id,
            prompt,
            "+12125550100",
        );
        let port = self.port;
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            response_tx.send(post(port, &request)).unwrap();
        });
        response_rx
    }

    pub fn post_email_without_credentials(&self) -> String {
        let request = signed_email_event(self.ingress, b"wrong-secret", "unsigned", "invalid");
        post(self.port, &request)
    }

    pub fn post_ignored_email_event(&self) -> String {
        post(
            self.port,
            &signed_email_event(
                self.ingress,
                b"personal-resend-secret",
                "ignored-webhook",
                "email.delivered",
            ),
        )
    }

    pub fn post_oversized_sms(&self) -> String {
        const OVERSIZED: usize = 1024 * 1024 + 1;
        let request = format!(
            "POST /w/{}/sms HTTP/1.1\r\nHost: localhost\r\nContent-Length: {OVERSIZED}\r\nConnection: close\r\n\r\n",
            self.ingress,
        );
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.write_all(&vec![b'x'; OVERSIZED]).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    pub fn shutdown(&mut self) {
        if self.target_registered {
            self.client
                .unregister_generation(self.generation, self.lease_id)
                .unwrap();
            self.target_registered = false;
        }
        if let Some(anchor) = self.anchor.take() {
            self.client
                .unregister_generation(self.generation, anchor.lease_id)
                .unwrap();
        }
        poll_until(Instant::now() + Duration::from_secs(3), || {
            self.child.try_wait().ok().flatten().is_some()
        });
    }
}

impl Drop for SharedReceiverFixture {
    fn drop(&mut self) {
        if self.target_registered {
            let _ = self
                .client
                .unregister_generation(self.generation, self.lease_id);
        }
        if let Some(anchor) = self.anchor.take() {
            let _ = self
                .client
                .unregister_generation(self.generation, anchor.lease_id);
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = self.home.path();
    }
}

fn save_personal_user(workspace: &WorkspaceContext) {
    brain::users::UsersStore::save(
        workspace,
        &brain::users::Users {
            schema_version: brain::users::USERS_SCHEMA_VERSION,
            users: vec![brain::users::User {
                id: brain::users::UserId::parse("personal-member").unwrap(),
                name: "Personal member".to_owned(),
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

fn make_anchor_workspace(
    home: &tempfile::TempDir,
    workspaces: &mut BTreeMap<WorkspaceName, brain::workspace::WorkspaceRecord>,
) -> WorkspaceContext {
    let root = home.path().join("family");
    let id = WorkspaceId::parse(FAMILY_ID).unwrap();
    brain::workspace::WorkspaceManifest::new(id)
        .write_new(&root)
        .unwrap();
    let name = WorkspaceName::parse("family").unwrap();
    workspaces.insert(
        name.clone(),
        brain::workspace::WorkspaceRecord {
            workspace_id: id,
            root: root.clone(),
            aliases: BTreeSet::new(),
            local_user_id: "family-member".to_owned(),
            receiver_enabled: true,
            env: serde_json::Map::new(),
        },
    );
    WorkspaceContext::new(home.path(), id, name, &root, "family-member", home.path()).unwrap()
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

fn register_workspace(
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

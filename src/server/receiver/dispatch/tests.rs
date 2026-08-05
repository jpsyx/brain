use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::{
    ProviderDeliveries, ProviderKey, SharedReceiverPipeline, execute_pipeline,
    forward_provider_delivery,
};
use crate::server::control::{ControlRequest, ControlResponse, ControlServer, LeaseRegistration};
use crate::server::lifecycle::{IngressId, LeaseId, ServerGeneration};
use crate::server::receiver::Channel;
use crate::workspace::{
    MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName, WorkspaceRecord,
};

#[test]
fn shared_pipeline_rechecks_persisted_intent_after_provider_work_before_connect() {
    let fixture = tempfile::tempdir().expect("receiver fixture");
    let workspace_id = WorkspaceId::parse(PERSONAL_ID).expect("workspace ID");
    let workspace_name = WorkspaceName::parse("personal").expect("workspace name");
    let workspace_root = fixture.path().join("personal");
    let manifest = crate::workspace::WorkspaceManifest::new(workspace_id);
    let ingress = IngressId::from(manifest.receiver_ingress_id());
    manifest
        .write_new(&workspace_root)
        .expect("workspace manifest");
    let workspace = WorkspaceContext::new(
        fixture.path(),
        workspace_id,
        workspace_name.clone(),
        &workspace_root,
        "personal-member",
        fixture.path(),
    )
    .expect("workspace context");
    crate::users::UsersStore::save(
        &workspace,
        &crate::users::Users {
            schema_version: crate::users::USERS_SCHEMA_VERSION,
            users: vec![crate::users::User {
                id: crate::users::UserId::parse("personal-member").expect("user ID"),
                name: "Personal member".to_owned(),
                phones: vec![crate::users::PhoneIdentity {
                    value: "+12125550100".to_owned(),
                    inbound_allowed: true,
                }],
                emails: Vec::new(),
                response_email: None,
            }],
        },
    )
    .expect("portable users");
    let store = RegistryStore::from_path(fixture.path().join("env.json"));
    store
        .replace(&MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: workspace_name.clone(),
            workspaces: BTreeMap::from([(
                workspace_name.clone(),
                WorkspaceRecord {
                    workspace_id,
                    root: workspace_root,
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
                    ]),
                },
            )]),
        })
        .expect("machine registry");
    let _guard = crate::tui::singleton::Guard::acquire(&workspace).expect("TUI singleton");
    let socket = crate::tui::singleton::JobSocket::bind(&workspace).expect("job socket");
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let lease_id = LeaseId::new();
    let mut server = ControlServer::new(generation, store.clone(), fixture.path().to_path_buf());
    let registration = LeaseRegistration {
        generation,
        lease_id,
        workspace_id,
        canonical_name: workspace_name.as_str().to_owned(),
        ingress_id: ingress,
        tui_pid: std::process::id(),
        resolved_root: workspace.root().to_path_buf(),
        job_socket: workspace.paths().job_socket(),
    };
    assert!(matches!(
        server.apply(ControlRequest::Register(registration), now),
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
    let (ticket, loader) = server
        .begin_workspace_route(ingress, now)
        .expect("route ticket");
    let context =
        crate::server::workspace_route::WorkspaceContextLoader::load(&loader, ticket.lease())
            .expect("route context");
    let route = server
        .finish_workspace_route(&ticket, context, now)
        .expect("resolved route");
    let control = Arc::new(Mutex::new(server));
    let body = "Body=late+disable&From=%2B12125550100&MessageSid=SM-late-disable";
    let fields = BTreeMap::from([
        ("Body".to_owned(), "late disable".to_owned()),
        ("From".to_owned(), "+12125550100".to_owned()),
        ("MessageSid".to_owned(), "SM-late-disable".to_owned()),
    ]);
    let signature = crate::server::security::twilio_signature(
        "personal-token",
        &format!("https://receiver.example.test/w/{ingress}/sms"),
        &fields,
    );
    let wire = format!(
        "POST /w/{ingress}/sms HTTP/1.1\r\nHost: localhost\r\nX-Twilio-Signature: {signature}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let (mut request_client, request_server) = tcp_pair();
    request_client
        .write_all(wire.as_bytes())
        .expect("write signed request");
    let request = crate::server::http::Request::read(request_server).expect("parse request");
    let provider_finished = Arc::new(Barrier::new(2));
    let release_admission = Arc::new(Barrier::new(2));
    let worker_provider_finished = Arc::clone(&provider_finished);
    let worker_release_admission = Arc::clone(&release_admission);
    let worker_control = Arc::clone(&control);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut request = request;
        let mut pipeline = SharedReceiverPipeline {
            route: Some(route),
            request: &mut request,
            control: &worker_control,
            channel: Channel::Sms,
            handoff_deadline: None,
            before_final_admission: Some(Box::new(move || {
                worker_provider_finished.wait();
                worker_release_admission.wait();
            })),
        };
        result_tx
            .send(execute_pipeline(&mut pipeline))
            .expect("report dispatch result");
    });

    provider_finished.wait();
    store
        .transition_receiver(
            &workspace_name,
            workspace_id,
            crate::workspace::ReceiverAction::Stop,
        )
        .expect("persist disable without live refresh");
    release_admission.wait();

    let mut queue = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(1);
    let result = loop {
        socket.poll_jobs(workspace_id, &mut queue);
        if let Ok(result) = result_rx.try_recv() {
            break result;
        }
        assert!(Instant::now() < deadline, "shared pipeline did not finish");
        std::thread::yield_now();
    };
    result.expect_err("persisted disable must reject before the real job-socket handoff");
    assert!(queue.is_empty(), "disabled work reached the live TUI queue");
}

fn tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test listener");
    let client = TcpStream::connect(listener.local_addr().expect("test address"))
        .expect("connect request client");
    let (server, _) = listener.accept().expect("accept request client");
    (client, server)
}

#[test]
fn failed_handoff_releases_provider_id_for_one_later_success() {
    let deliveries = Mutex::new(ProviderDeliveries::default());
    let key = key(PERSONAL_ID, Channel::Sms, "provider-1");
    let mut attempts = 0;

    assert!(
        forward_provider_delivery(&deliveries, &key, || {
            attempts += 1;
            anyhow::bail!("socket unavailable")
        })
        .is_err()
    );
    forward_provider_delivery(&deliveries, &key, || {
        attempts += 1;
        Ok(())
    })
    .unwrap();
    forward_provider_delivery(&deliveries, &key, || {
        attempts += 1;
        Ok(())
    })
    .unwrap();

    assert_eq!(attempts, 2);
}

#[test]
fn in_flight_duplicate_is_not_acknowledged_before_first_handoff_finishes() {
    let deliveries = Arc::new(Mutex::new(ProviderDeliveries::default()));
    let key = key(PERSONAL_ID, Channel::Email, "provider-2");
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_deliveries = Arc::clone(&deliveries);
    let worker_key = key.clone();
    let worker_entered = Arc::clone(&entered);
    let worker_release = Arc::clone(&release);
    let worker = std::thread::spawn(move || {
        forward_provider_delivery(&worker_deliveries, &worker_key, || {
            worker_entered.wait();
            worker_release.wait();
            Ok(())
        })
    });
    entered.wait();

    assert!(forward_provider_delivery(&deliveries, &key, || Ok(())).is_err());
    release.wait();
    worker.join().unwrap().unwrap();
    forward_provider_delivery(&deliveries, &key, || {
        panic!("accepted duplicate reached the job socket")
    })
    .unwrap();
}

#[test]
fn retained_provider_ids_are_bounded_and_workspace_channel_scoped() {
    let mut deliveries = ProviderDeliveries::default();
    for index in 0..=1024 {
        let key = key(PERSONAL_ID, Channel::Sms, &format!("provider-{index}"));
        assert!(deliveries.begin(key.clone()).started());
        deliveries.finish(&key, true);
    }

    assert!(
        deliveries
            .begin(key(PERSONAL_ID, Channel::Sms, "provider-0"))
            .started()
    );
    assert!(
        deliveries
            .begin(key(FAMILY_ID, Channel::Sms, "provider-1024"))
            .started()
    );
    assert!(
        deliveries
            .begin(key(PERSONAL_ID, Channel::Email, "provider-1024"))
            .started()
    );
}

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

fn key(workspace: &str, channel: Channel, provider_id: &str) -> ProviderKey {
    (
        WorkspaceId::parse(workspace).unwrap(),
        channel,
        provider_id.to_owned(),
    )
}

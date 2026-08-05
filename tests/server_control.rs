use brain::server::control::{
    ControlRequest, ControlResponse, ControlServer, HeartbeatDisposition, LeaseRegistration,
    heartbeat_disposition,
};
use brain::server::lifecycle::{IngressId, LeaseId, ServerGeneration};
use brain::workspace::{RegistryStore, WorkspaceId, WorkspaceManifest};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn generation() -> ServerGeneration {
    ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("generation")
}

fn lease_id() -> LeaseId {
    LeaseId::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5").expect("lease ID")
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse("2174fb9d-ae76-4bde-a526-38ac43ebdf8f").expect("workspace ID")
}

fn ingress_id() -> IngressId {
    IngressId::parse("a4f0ec11-d121-4f58-aa44-2448ba427b76").expect("ingress ID")
}

#[test]
fn register_request_round_trips_as_newline_delimited_json() {
    let request = ControlRequest::Register(LeaseRegistration {
        generation: generation(),
        lease_id: lease_id(),
        workspace_id: workspace_id(),
        canonical_name: "personal".to_owned(),
        ingress_id: ingress_id(),
        tui_pid: 101,
        job_socket: PathBuf::from("/tmp/brain-test/jobs.sock"),
    });

    let encoded = brain::server::control::codec::encode(&request).expect("encode request");
    assert_eq!(encoded.last(), Some(&b'\n'));
    assert!(!String::from_utf8_lossy(&encoded).contains("root"));

    let decoded: ControlRequest =
        brain::server::control::codec::decode(&encoded).expect("decode request");
    assert_eq!(decoded, request);
}

#[test]
fn codec_rejects_malformed_and_oversized_frames() {
    assert!(brain::server::control::codec::decode::<ControlRequest>(b"not-json\n").is_err());
    assert!(brain::server::control::codec::decode::<ControlRequest>(b"{}{}").is_err());
    assert!(brain::server::control::codec::decode::<ControlRequest>(b"{}\n{}\n").is_err());
    assert!(
        brain::server::control::codec::decode::<ControlRequest>(&vec![
            b'x';
            brain::server::control::codec::MAX_FRAME_BYTES
                + 1
        ])
        .is_err()
    );
}

#[test]
fn registration_reopens_the_authoritative_manifest() {
    let fixture = ControlFixture::new();
    let mut server = ControlServer::new(generation(), fixture.registry_store());
    let replacement = serde_json::json!({
        "schema_version": 1,
        "workspace_id": "f825f323-821d-4e96-a16f-2796b1ce5802",
        "receiver_ingress_id": fixture.ingress_id,
        "minimum_brain_version": env!("CARGO_PKG_VERSION")
    });
    std::fs::write(
        fixture.root.join(".config/workspace.json"),
        serde_json::to_vec_pretty(&replacement).expect("manifest JSON"),
    )
    .expect("replace manifest");

    assert!(matches!(
        server.apply(
            ControlRequest::Register(fixture.registration()),
            Instant::now()
        ),
        ControlResponse::Rejected { message }
            if message.contains("manifest UUID does not match")
    ));
}

#[test]
fn heartbeat_recovery_is_required_for_missing_or_stale_generations() {
    assert_eq!(heartbeat_disposition(None), HeartbeatDisposition::Recover);
    assert_eq!(
        heartbeat_disposition(Some(&ControlResponse::StaleGeneration)),
        HeartbeatDisposition::Recover
    );
    assert_eq!(
        heartbeat_disposition(Some(&ControlResponse::Accepted {
            generation: generation(),
            shutdown: false,
        })),
        HeartbeatDisposition::Current
    );
}

#[test]
fn register_heartbeat_update_snapshot_and_unregister_are_generation_guarded() {
    let fixture = ControlFixture::new();
    let mut server = ControlServer::new(generation(), fixture.registry_store());
    let now = Instant::now();

    assert!(matches!(
        server.apply(ControlRequest::Register(fixture.registration()), now),
        ControlResponse::Accepted {
            generation: accepted_generation,
            shutdown: false,
        } if accepted_generation == generation()
    ));
    assert!(matches!(
        server.apply(ControlRequest::Register(fixture.registration()), now),
        ControlResponse::Rejected { .. }
    ));
    assert!(matches!(
        server.apply(
            ControlRequest::Heartbeat {
                generation: stale_generation(),
                lease_id: lease_id(),
            },
            now + Duration::from_secs(1),
        ),
        ControlResponse::StaleGeneration
    ));
    assert!(matches!(
        server.apply(
            ControlRequest::Heartbeat {
                generation: generation(),
                lease_id: lease_id(),
            },
            now + Duration::from_secs(1),
        ),
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
    assert!(matches!(
        server.apply(
            ControlRequest::UpdateEnabled {
                generation: generation(),
                lease_id: lease_id(),
                receiver_enabled: false,
            },
            now + Duration::from_secs(1),
        ),
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
    assert!(matches!(
        server.apply(ControlRequest::Snapshot, now + Duration::from_secs(1)),
        ControlResponse::Snapshot(snapshot)
            if snapshot.generation == generation() && snapshot.live_leases == 1
    ));
    assert!(matches!(
        server.apply(
            ControlRequest::Unregister {
                generation: generation(),
                lease_id: lease_id(),
            },
            now + Duration::from_secs(1),
        ),
        ControlResponse::Accepted { shutdown: true, .. }
    ));
}

fn stale_generation() -> ServerGeneration {
    ServerGeneration::parse("b5487d2a-2625-49a4-b5f1-fd929ff5bd80").expect("generation")
}

struct ControlFixture {
    temporary: tempfile::TempDir,
    root: PathBuf,
    ingress_id: IngressId,
}

impl ControlFixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary control fixture");
        let root = temporary.path().join("workspace");
        let manifest = WorkspaceManifest::new(workspace_id());
        manifest.write_new(&root).expect("workspace manifest");
        let registry = serde_json::json!({
            "schema_version": 2,
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
        Self {
            temporary,
            root,
            ingress_id: manifest.receiver_ingress_id().into(),
        }
    }

    fn registry_store(&self) -> RegistryStore {
        RegistryStore::from_path(self.temporary.path().join("env.json"))
    }

    fn registration(&self) -> LeaseRegistration {
        LeaseRegistration {
            generation: generation(),
            lease_id: lease_id(),
            workspace_id: workspace_id(),
            canonical_name: "personal".to_owned(),
            ingress_id: self.ingress_id,
            tui_pid: 101,
            job_socket: self.temporary.path().join("jobs.sock"),
        }
    }
}

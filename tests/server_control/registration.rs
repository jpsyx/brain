use super::support::{ControlFixture, generation, workspace_id};
use brain::server::control::{ControlRequest, ControlResponse, ControlServer};
use brain::workspace::WorkspaceManifest;
use std::time::Instant;

#[test]
fn registration_reopens_the_authoritative_manifest() {
    let fixture = ControlFixture::new();
    let mut server = ControlServer::new(
        generation(),
        fixture.registry_store(),
        fixture.temporary.path().to_path_buf(),
    );
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
fn registration_rejects_a_root_changed_after_tui_resolution() {
    let fixture = ControlFixture::new();
    let registration = fixture.registration();
    let replacement_root = fixture.temporary.path().join("replacement-workspace");
    let manifest = WorkspaceManifest::new(workspace_id());
    manifest
        .write_new(&replacement_root)
        .expect("replacement manifest");
    let registry = serde_json::json!({
        "schema_version": brain::workspace::REGISTRY_SCHEMA_VERSION,
        "default_workspace": "personal",
        "workspaces": {
            "personal": {
                "workspace_id": workspace_id(),
                "root": replacement_root,
                "aliases": [],
                "local_user_id": "tester",
                "receiver_enabled": true,
                "env": {}
            }
        }
    });
    std::fs::write(
        fixture.temporary.path().join("env.json"),
        serde_json::to_vec_pretty(&registry).expect("registry JSON"),
    )
    .expect("replace registry");
    let mut server = ControlServer::new(
        generation(),
        fixture.registry_store(),
        fixture.temporary.path().to_path_buf(),
    );

    assert!(matches!(
        server.apply(ControlRequest::Register(registration), Instant::now()),
        ControlResponse::Rejected { message }
            if message.contains("root changed after the TUI resolved it")
    ));
}

#[test]
fn registration_accepts_the_live_tui_singleton_without_a_workspace_endpoint() {
    let fixture = ControlFixture::new();
    let mut server = ControlServer::new(
        generation(),
        fixture.registry_store(),
        fixture.temporary.path().to_path_buf(),
    );

    assert!(matches!(
        server.apply(
            ControlRequest::Register(fixture.registration()),
            Instant::now()
        ),
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
}

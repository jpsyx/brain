pub(super) fn save_personal_user(workspace: &WorkspaceContext) {
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

pub(super) fn make_anchor_workspace(
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

pub(super) fn register_workspace(
    client: &brain::server::control::ServerClient,
    generation: brain::server::lifecycle::ServerGeneration,
    workspace: &WorkspaceContext,
    ingress_id: brain::server::IngressId,
) -> brain::server::control::HeartbeatWorker {
    let lease_id = brain::server::lifecycle::LeaseId::new();
    let registration = brain::server::control::LeaseRegistration {
        generation,
        lease_id,
        workspace_id: workspace.id(),
        canonical_name: workspace.name().as_str().to_owned(),
        ingress_id,
        tui_pid: std::process::id(),
        resolved_root: workspace.root().to_path_buf(),
        job_socket: workspace.paths().job_socket(),
    };
    client.register_generation(&registration).unwrap();
    brain::server::control::HeartbeatWorker::start(client.clone(), registration)
}

pub(super) fn poll_value<T>(deadline: Instant, mut value: impl FnMut() -> Option<T>) -> T {
    loop {
        if let Some(value) = value() {
            return value;
        }
        assert!(Instant::now() < deadline, "value was not produced");
        std::thread::yield_now();
    }
}
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::{FAMILY_ID, WorkspaceContext, WorkspaceId, WorkspaceName};

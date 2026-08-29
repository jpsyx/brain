use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

use brain::server::lifecycle::{IngressId, LeaseTable, WorkspaceAvailability, WorkspaceLease};
use brain::server::workspace_route::WorkspaceRouteResolver;
use brain::workspace::{
    MachineRegistry, RegistryStore, WorkspaceId, WorkspaceManifest, WorkspaceName, WorkspaceRecord,
};
use serde_json::Map;

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

struct Fixture {
    home: tempfile::TempDir,
    store: RegistryStore,
    table: LeaseTable,
    personal_ingress: IngressId,
    family_ingress: IngressId,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("temporary home");
        let personal_root = home.path().join("personal");
        let family_root = home.path().join("family");
        let personal_ingress = write_manifest(&personal_root, PERSONAL_ID);
        let family_ingress = write_manifest(&family_root, FAMILY_ID);
        let personal_name = WorkspaceName::parse("personal").expect("personal name");
        let family_name = WorkspaceName::parse("family").expect("family name");
        let registry = MachineRegistry {
            schema_version: brain::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (
                    personal_name.clone(),
                    record(PERSONAL_ID, &personal_root, true),
                ),
                (family_name.clone(), record(FAMILY_ID, &family_root, false)),
            ]),
            env: serde_json::Map::new(),
        };
        let store = RegistryStore::from_path(home.path().join(".config/brain/env.json"));
        store.replace(&registry).expect("write registry");
        let now = Instant::now();
        let mut table = LeaseTable::default();
        table
            .register(
                lease(PERSONAL_ID, personal_name, personal_ingress, true, now),
                now,
            )
            .expect("personal lease");
        table
            .register(
                lease(FAMILY_ID, family_name, family_ingress, false, now),
                now,
            )
            .expect("family lease");
        Self {
            home,
            store,
            table,
            personal_ingress,
            family_ingress,
        }
    }

    fn resolver(&self) -> WorkspaceRouteResolver<'_> {
        WorkspaceRouteResolver::new(&self.table, &self.store, self.home.path(), Instant::now())
    }
}

#[test]
fn only_receiver_enabled_live_ingress_resolves_to_a_verified_context() {
    let fixture = Fixture::new();
    let personal_ingress = fixture.personal_ingress;
    let family_ingress = fixture.family_ingress;

    let personal = fixture
        .resolver()
        .resolve(personal_ingress)
        .expect("personal route");
    assert_eq!(personal.context().id(), workspace_id(PERSONAL_ID));
    assert!(personal.lease().receiver_enabled);

    let family_error = fixture
        .resolver()
        .resolve(family_ingress)
        .expect_err("disabled workspace is unavailable");
    assert_eq!(family_error.status(), 503);
}

#[test]
fn unknown_and_known_without_live_tui_have_distinct_http_statuses() {
    let mut fixture = Fixture::new();
    let unknown = IngressId::new();
    let personal_ingress = fixture.personal_ingress;
    let now = Instant::now();
    let personal_lease = match fixture.table.availability(personal_ingress, now) {
        WorkspaceAvailability::Accepting(lease) => lease,
        state => panic!("expected accepting live lease, got {state:?}"),
    };
    let _ = fixture.table.unregister(personal_lease.lease_id, now);

    let unknown_error = fixture.resolver().resolve(unknown).unwrap_err();
    assert_eq!(unknown_error.status(), 404);
    let unavailable_error = fixture.resolver().resolve(personal_ingress).unwrap_err();
    assert_eq!(unavailable_error.status(), 503);
}

fn write_manifest(root: &Path, id: &str) -> IngressId {
    std::fs::create_dir_all(root).expect("workspace root");
    let manifest = WorkspaceManifest::new(workspace_id(id));
    let ingress = manifest.receiver_ingress_id().into();
    manifest.write_new(root).expect("workspace manifest");
    ingress
}

fn record(id: &str, root: &Path, receiver_enabled: bool) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: workspace_id(id),
        root: root.to_path_buf(),
        aliases: BTreeSet::new(),
        local_user_id: "tester".to_owned(),
        receiver_enabled,
        env: Map::new(),
    }
}

fn lease(
    id: &str,
    name: WorkspaceName,
    ingress_id: IngressId,
    receiver_enabled: bool,
    now: Instant,
) -> WorkspaceLease {
    WorkspaceLease {
        lease_id: brain::server::lifecycle::LeaseId::new(),
        workspace_id: workspace_id(id),
        canonical_name: name,
        ingress_id,
        tui_pid: std::process::id(),
        receiver_enabled,
        expires_at: now + Duration::from_secs(30),
    }
}

fn workspace_id(id: &str) -> WorkspaceId {
    WorkspaceId::parse(id).expect("valid workspace ID")
}

use std::collections::{BTreeMap, BTreeSet};

use brain::workspace::{
    MachineRegistry, ReceiverAction, RegistryStore, WorkspaceId, WorkspaceName, WorkspaceRecord,
    receiver_transition,
};

fn workspace_id(value: &str) -> WorkspaceId {
    WorkspaceId::parse(value).expect("workspace ID")
}

fn record(root: &std::path::Path, workspace_id: WorkspaceId) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id,
        root: root.to_path_buf(),
        aliases: BTreeSet::new(),
        local_user_id: "tester".to_owned(),
        receiver_enabled: false,
        env: serde_json::Map::new(),
    }
}

#[test]
fn every_receiver_surface_shares_one_transition_decision() {
    assert!(receiver_transition(false, ReceiverAction::Start));
    assert!(!receiver_transition(true, ReceiverAction::Stop));
    assert!(receiver_transition(false, ReceiverAction::WithReceiverFlag));
    assert!(receiver_transition(false, ReceiverAction::Toggle));
    assert!(!receiver_transition(true, ReceiverAction::Toggle));
}

#[test]
fn persisted_receiver_transition_changes_only_the_exact_selected_record() {
    let temporary = tempfile::tempdir().expect("temporary registry");
    let personal_name = WorkspaceName::parse("personal").expect("personal name");
    let family_name = WorkspaceName::parse("family").expect("family name");
    let personal_id = workspace_id("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b");
    let family_id = workspace_id("e806258e-491a-436d-9db4-a5ca9903e0d4");
    let store = RegistryStore::from_path(temporary.path().join("env.json"));
    let registry = MachineRegistry {
        schema_version: brain::workspace::REGISTRY_SCHEMA_VERSION,
        default_workspace: personal_name.clone(),
        workspaces: BTreeMap::from([
            (
                personal_name.clone(),
                record(&temporary.path().join("personal"), personal_id),
            ),
            (
                family_name.clone(),
                record(&temporary.path().join("family"), family_id),
            ),
        ]),
        env: serde_json::Map::new(),
    };
    store.replace(&registry).expect("seed registry");

    let enabled = store
        .transition_receiver(&family_name, family_id, ReceiverAction::Start)
        .expect("enable family receiver");

    assert!(enabled);
    let saved = RegistryStore::load_from(store.path()).expect("load saved registry");
    assert!(saved.workspaces[&family_name].receiver_enabled);
    assert!(!saved.workspaces[&personal_name].receiver_enabled);
}

#[test]
fn stale_selected_identity_cannot_mutate_a_replacement_record() {
    let temporary = tempfile::tempdir().expect("temporary registry");
    let family_name = WorkspaceName::parse("family").expect("family name");
    let current_id = workspace_id("e806258e-491a-436d-9db4-a5ca9903e0d4");
    let stale_id = workspace_id("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b");
    let store = RegistryStore::from_path(temporary.path().join("env.json"));
    let registry = MachineRegistry {
        schema_version: brain::workspace::REGISTRY_SCHEMA_VERSION,
        default_workspace: family_name.clone(),
        workspaces: BTreeMap::from([(
            family_name.clone(),
            record(&temporary.path().join("family"), current_id),
        )]),
        env: serde_json::Map::new(),
    };
    store.replace(&registry).expect("seed registry");
    let before = std::fs::read(store.path()).expect("registry bytes");

    let error = store
        .transition_receiver(&family_name, stale_id, ReceiverAction::Start)
        .expect_err("stale selection must fail");

    assert!(error.to_string().contains("identity changed"), "{error:#}");
    assert_eq!(
        std::fs::read(store.path()).expect("registry bytes after failure"),
        before
    );
}

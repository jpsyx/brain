use brain::workspace::{RegistryStore, WorkspaceId, WorkspaceName};
use serde_json::json;

use crate::support::Fixture;

#[test]
fn structural_env_keys_are_read_only_and_rejections_preserve_every_store() {
    let fixture = Fixture::new();
    let root = fixture.family.workspace.root();
    std::fs::create_dir_all(root.join(".config")).expect("portable config dir");
    let config_path = root.join(".config/config.json");
    let personalization_path = root.join(".config/personalization.json");
    std::fs::write(&config_path, b"{\"sentinel\":\"config\"}\n").expect("config fixture");
    std::fs::write(&personalization_path, b"{\"name\":\"Family sentinel\"}\n")
        .expect("personalization fixture");
    let registry_before = std::fs::read(fixture.store.path()).expect("registry bytes");
    let config_before = std::fs::read(&config_path).expect("config bytes");
    let personalization_before =
        std::fs::read(&personalization_path).expect("personalization bytes");

    for key in [
        "root",
        "root.child",
        "workspace_id",
        "workspace_id.value",
        "aliases",
        "aliases.0",
        "local_user_id",
        "local_user_id.value",
        "receiver_enabled",
        "receiver_enabled.value",
        "access_mode",
        "access_mode.value",
        "access_policy",
        "access_policy.value",
        "schema_version",
        "default_workspace",
        "workspaces.family",
        "env.root",
    ] {
        assert!(
            brain::env::set(&fixture.family, key, "forbidden").is_err(),
            "set must reject structural key {key}"
        );
        assert!(
            brain::env::set_raw(&fixture.family, key, json!({"forbidden": true})).is_err(),
            "set_raw must reject structural key {key}"
        );
        assert_eq!(
            std::fs::read(fixture.store.path()).expect("registry after rejection"),
            registry_before,
            "registry bytes changed after rejecting {key}"
        );
        assert_eq!(
            std::fs::read(&config_path).expect("config after rejection"),
            config_before,
            "portable config changed after rejecting {key}"
        );
        assert_eq!(
            std::fs::read(&personalization_path).expect("personalization after rejection"),
            personalization_before,
            "portable personalization changed after rejecting {key}"
        );
    }

    assert_eq!(
        brain::env::resolve_one(&fixture.family, "root"),
        Some(root.display().to_string()),
        "root remains a virtual read-only env row"
    );
}

#[test]
fn env_reads_and_writes_reject_a_same_name_replacement_uuid_without_touching_bytes() {
    let fixture = Fixture::new();
    let family_name = WorkspaceName::parse("family").expect("family name");
    let replacement_id =
        WorkspaceId::parse("c48b0de2-361d-43aa-8e7d-9a60ba6caf39").expect("replacement id");
    let mut registry = RegistryStore::load_from(fixture.store.path()).expect("registry");
    let replacement = registry
        .workspaces
        .get_mut(&family_name)
        .expect("family record");
    replacement.workspace_id = replacement_id;
    replacement
        .env
        .insert("claude_cmd".to_owned(), json!("replacement-secret"));
    fixture.store.replace(&registry).expect("replace identity");
    let registry_before = std::fs::read(fixture.store.path()).expect("replacement bytes");
    let root_sentinel = fixture
        .family
        .workspace
        .root()
        .join("portable-sentinel.txt");
    std::fs::write(&root_sentinel, b"portable\n").expect("portable sentinel");
    let root_before = std::fs::read(&root_sentinel).expect("portable bytes");

    assert_eq!(
        brain::env::get(&fixture.family, "claude_cmd"),
        None,
        "a stale context must not read the replacement record"
    );
    assert!(
        brain::env::set(&fixture.family, "claude_cmd", "stale-write").is_err(),
        "a stale context must not write the replacement record"
    );
    assert_eq!(
        std::fs::read(fixture.store.path()).expect("registry after stale access"),
        registry_before
    );
    assert_eq!(
        std::fs::read(root_sentinel).expect("portable root after stale access"),
        root_before
    );
}

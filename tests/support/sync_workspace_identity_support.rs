use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use brain::sync::config::SyncConfig;
use brain::sync::identity::{
    RemoteIdentityDecision, check_remote_identity, check_remote_manifest_identity,
};
use brain::users::{USERS_SCHEMA_VERSION, User, UserId, Users, UsersStore};
use brain::workspace::{
    CommandContext, MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
    WorkspaceRecord,
};
use serde_json::{Map, json};

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const INGRESS_ID: &str = "c48b0de2-361d-43aa-8e7d-9a60ba6caf39";

struct Fixture {
    home: tempfile::TempDir,
    personal: CommandContext,
    family: CommandContext,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("temporary home");
        let personal_root = home.path().join("personal");
        let family_root = home.path().join("family");
        std::fs::create_dir_all(&personal_root).expect("personal root");
        std::fs::create_dir_all(&family_root).expect("family root");
        let personal_id = workspace_id(PERSONAL_ID);
        let family_id = workspace_id(FAMILY_ID);
        let personal_name = WorkspaceName::parse("personal").expect("personal name");
        let family_name = WorkspaceName::parse("family").expect("family name");
        let registry = MachineRegistry {
            schema_version: brain::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (
                    personal_name.clone(),
                    record(
                        personal_id,
                        personal_root.clone(),
                        "personal-bucket",
                        "personal-key",
                    ),
                ),
                (
                    family_name.clone(),
                    record(
                        family_id,
                        family_root.clone(),
                        "family-bucket",
                        "family-key",
                    ),
                ),
            ]),
            env: serde_json::Map::new(),
        };
        let store = RegistryStore::from_path(home.path().join("config/brain/env.json"));
        store.replace(&registry).expect("registry fixture");
        let personal = context(
            home.path(),
            personal_id,
            personal_name,
            &personal_root,
            store.clone(),
        );
        let family = context(home.path(), family_id, family_name, &family_root, store);
        Self {
            home,
            personal,
            family,
        }
    }
}

fn workspace_id(raw: &str) -> WorkspaceId {
    WorkspaceId::parse(raw).expect("fixed workspace UUID")
}

fn record(
    workspace_id: WorkspaceId,
    root: std::path::PathBuf,
    bucket: &str,
    app_key: &str,
) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id,
        root,
        aliases: BTreeSet::new(),
        local_user_id: "pablo".to_owned(),
        receiver_enabled: false,
        env: Map::from_iter([(
            "sync".to_owned(),
            json!({
                "enabled": true,
                "b2_bucket": bucket,
                "b2_key_id": format!("{bucket}-id"),
                "b2_app_key": app_key,
            }),
        )]),
    }
}

fn context(
    home: &std::path::Path,
    id: WorkspaceId,
    name: WorkspaceName,
    root: &std::path::Path,
    store: RegistryStore,
) -> CommandContext {
    let workspace = Arc::new(
        WorkspaceContext::new(home, id, name, root, "pablo", home).expect("workspace context"),
    );
    brain::workspace::WorkspaceManifest::new(id)
        .write_new(root)
        .expect("workspace manifest");
    UsersStore::save(
        &workspace,
        &Users {
            schema_version: USERS_SCHEMA_VERSION,
            users: vec![User {
                id: UserId::parse("pablo").expect("user ID"),
                name: "Pablo".to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            }],
        },
    )
    .expect("portable users");
    CommandContext::new(workspace, store).expect("command context")
}

fn manifest_bytes(id: &str, minimum_brain_version: &str) -> Vec<u8> {
    format!(
        "{{\n  \"schema_version\": 1,\n  \"workspace_id\": \"{id}\",\n  \"receiver_ingress_id\": \"{INGRESS_ID}\",\n  \"minimum_brain_version\": \"{minimum_brain_version}\"\n}}\n"
    )
    .into_bytes()
}

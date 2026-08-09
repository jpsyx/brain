use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use brain::users::{EmailIdentity, PhoneIdentity, User, UserId, Users, UsersStore};
use brain::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceContext, WorkspaceId,
    WorkspaceManifest, WorkspaceName, WorkspaceRecord,
};
use serde_json::Map;

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

pub(super) struct Fixture {
    pub(super) home: tempfile::TempDir,
    pub(super) config_home: tempfile::TempDir,
    pub(super) cache_home: tempfile::TempDir,
    pub(super) registry_path: PathBuf,
    pub(super) personal: WorkspaceContext,
    pub(super) family: WorkspaceContext,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let config_home = tempfile::tempdir().unwrap();
        let cache_home = tempfile::tempdir().unwrap();
        let personal_id = WorkspaceId::parse(PERSONAL_ID).unwrap();
        let family_id = WorkspaceId::parse(FAMILY_ID).unwrap();
        let personal_name = WorkspaceName::parse("personal").unwrap();
        let family_name = WorkspaceName::parse("family").unwrap();
        let personal_root = home.path().join("personal");
        let family_root = home.path().join("family");
        std::fs::create_dir_all(&personal_root).unwrap();
        std::fs::create_dir_all(&family_root).unwrap();
        WorkspaceManifest::new(personal_id)
            .write_new(&personal_root)
            .unwrap();
        WorkspaceManifest::new(family_id)
            .write_new(&family_root)
            .unwrap();

        let personal = workspace(
            home.path(),
            personal_id,
            personal_name.clone(),
            &personal_root,
        );
        let family = workspace(home.path(), family_id, family_name.clone(), &family_root);
        UsersStore::save(&personal, &users_fixture("pablo", "Pablo")).unwrap();
        UsersStore::save(&family, &users_fixture("casey", "Casey")).unwrap();

        let registry = MachineRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (personal_name, record(personal_id, personal_root, "pablo")),
                (family_name, record(family_id, family_root, "casey")),
            ]),
            env: serde_json::Map::new(),
        };
        let registry_path = config_home.path().join("brain/env.json");
        RegistryStore::from_path(registry_path.clone())
            .replace(&registry)
            .unwrap();

        Self {
            home,
            config_home,
            cache_home,
            registry_path,
            personal,
            family,
        }
    }

    pub(super) fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("XDG_CACHE_HOME", self.cache_home.path())
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    pub(super) fn registry(&self) -> MachineRegistry {
        RegistryStore::load_from(&self.registry_path).unwrap()
    }
}

fn workspace(home: &Path, id: WorkspaceId, name: WorkspaceName, root: &Path) -> WorkspaceContext {
    WorkspaceContext::new(home, id, name, root, "local", home).unwrap()
}

fn record(id: WorkspaceId, root: PathBuf, local_user_id: &str) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: id,
        root,
        aliases: BTreeSet::new(),
        local_user_id: local_user_id.to_owned(),
        receiver_enabled: false,
        env: Map::new(),
    }
}

fn users_fixture(id: &str, name: &str) -> Users {
    Users {
        schema_version: brain::users::USERS_SCHEMA_VERSION,
        users: vec![User {
            id: UserId::parse(id).unwrap(),
            name: name.to_owned(),
            phones: Vec::<PhoneIdentity>::new(),
            emails: Vec::<EmailIdentity>::new(),
            response_email: None,
        }],
    }
}

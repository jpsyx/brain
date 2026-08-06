use std::path::Path;

use brain::users::{UserId, Users, UsersError, UsersStore, propose_legacy_user_migration};
use brain::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceContext, WorkspaceId,
    WorkspaceManifest, WorkspaceName, WorkspaceRecord,
};
use serde_json::Map;
use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Output};

const FIXTURE: &str = r#"{
  "schema_version": 1,
  "users": [
    {
      "id": "pablo",
      "name": "Pablo",
      "phones": [
        {
          "value": "(212) 555-0100",
          "inbound_allowed": true
        },
        {
          "value": "+12125550101",
          "inbound_allowed": false
        }
      ],
      "emails": [
        {
          "value": " Pablo+Brain@Example.COM ",
          "inbound_allowed": false
        }
      ],
      "response_email": "pablo+brain@example.com"
    },
    {
      "id": "wife",
      "name": "Wife",
      "phones": [],
      "emails": [
        {
          "value": "Wife@Example.COM",
          "inbound_allowed": true
        },
        {
          "value": "wife+brain@example.com",
          "inbound_allowed": false
        }
      ],
      "response_email": null
    }
  ]
}"#;

const DUPLICATE_PHONE_FIXTURE: &str = r#"{
  "schema_version": 1,
  "users": [
    {
      "id": "pablo",
      "name": "Pablo",
      "phones": [{"value": "+12125550100", "inbound_allowed": true}]
    },
    {
      "id": "wife",
      "name": "Wife",
      "phones": [{"value": "212-555-0100", "inbound_allowed": true}]
    }
  ]
}"#;

fn workspace(root: &Path) -> WorkspaceContext {
    WorkspaceContext::new(
        root,
        WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        WorkspaceName::parse("family").unwrap(),
        root,
        "pablo",
        root,
    )
    .unwrap()
}

struct CliFixture {
    home: tempfile::TempDir,
    config_home: tempfile::TempDir,
    root: std::path::PathBuf,
    registry_path: std::path::PathBuf,
}

impl CliFixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let config_home = tempfile::tempdir().unwrap();
        let root = home.path().join("family");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
        WorkspaceManifest::new(workspace_id)
            .write_new(&root)
            .unwrap();
        let canonical_name = WorkspaceName::parse("family").unwrap();
        let registry = MachineRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            default_workspace: canonical_name.clone(),
            workspaces: BTreeMap::from([(
                canonical_name,
                WorkspaceRecord {
                    workspace_id,
                    root: root.clone(),
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env: Map::new(),
                },
            )]),
        };
        let registry_path = config_home.path().join("brain/env.json");
        RegistryStore::from_path(registry_path.clone())
            .replace(&registry)
            .unwrap();
        UsersStore::save(
            &workspace(&root),
            &Users::parse(FIXTURE.as_bytes()).unwrap(),
        )
        .unwrap();
        Self {
            home,
            config_home,
            root,
            registry_path,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("XDG_CACHE_HOME", self.home.path().join("cache"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    fn users(&self) -> Users {
        UsersStore::load(&workspace(&self.root)).unwrap()
    }
}

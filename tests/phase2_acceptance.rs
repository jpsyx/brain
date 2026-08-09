use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use brain::actor::{Channel, RequestIdentity};
use brain::server::security::{AuthenticatedActorError, resolve_authenticated_actor};
use brain::users::{UserId, Users, UsersStore};
use brain::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceContext, WorkspaceId,
    WorkspaceManifest, WorkspaceName, WorkspaceRecord,
};
use serde_json::Map;

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

struct Machine {
    home: tempfile::TempDir,
    config: tempfile::TempDir,
    cache: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            config: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    fn registry_path(&self) -> PathBuf {
        self.config.path().join("brain/env.json")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_CACHE_HOME", self.cache.path())
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn workspace(home: &Path, root: &Path, local_user_id: &str) -> WorkspaceContext {
    WorkspaceContext::new(
        home,
        WorkspaceId::parse(WORKSPACE_ID).unwrap(),
        WorkspaceName::parse("family").unwrap(),
        root,
        local_user_id,
        root,
    )
    .unwrap()
}

fn portable_users() -> Users {
    Users::parse(
        br#"{
  "schema_version": 1,
  "users": [
    {
      "id": "local-member",
      "name": "Local Member",
      "phones": [],
      "emails": [],
      "response_email": null
    },
    {
      "id": "remote-member",
      "name": "Remote Member",
      "phones": [{"value": "+12125550123", "inbound_allowed": true}],
      "emails": [{"value": "remote@example.test", "inbound_allowed": true}],
      "response_email": "remote@example.test"
    }
  ]
}"#,
    )
    .unwrap()
}

#[test]
fn two_machine_registries_select_the_same_portable_person() {
    let portable = tempfile::tempdir().unwrap();
    let root = portable.path().join("family");
    std::fs::create_dir_all(&root).unwrap();
    WorkspaceManifest::new(WorkspaceId::parse(WORKSPACE_ID).unwrap())
        .write_new(&root)
        .unwrap();
    let seed_home = tempfile::tempdir().unwrap();
    UsersStore::save(
        &workspace(seed_home.path(), &root, "local-member"),
        &portable_users(),
    )
    .unwrap();
    let users_before = std::fs::read(root.join(".config/users.json")).unwrap();
    let first = Machine::new();
    let second = Machine::new();
    let root_arg = root.to_str().unwrap();

    for machine in [&first, &second] {
        assert_success(&machine.run(&["workspace", "attach", root_arg]));
        assert_success(&machine.run(&["user", "local", "local-member", "-b", "family"]));
    }

    let first_registry = RegistryStore::load_from(&first.registry_path()).unwrap();
    let second_registry = RegistryStore::load_from(&second.registry_path()).unwrap();
    let name = WorkspaceName::parse("family").unwrap();
    assert_eq!(
        first_registry.workspaces[&name].local_user_id,
        "local-member"
    );
    assert_eq!(
        second_registry.workspaces[&name].local_user_id,
        "local-member"
    );
    assert_eq!(
        first_registry.workspaces[&name].workspace_id,
        second_registry.workspaces[&name].workspace_id
    );
    assert_eq!(
        std::fs::read(root.join(".config/users.json")).unwrap(),
        users_before
    );
}

#[test]
fn authenticated_inbound_actor_drives_default_task_assignment() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home");
    let config = temporary.path().join("xdg-config");
    let cache = temporary.path().join("xdg-cache");
    let root = temporary.path().join("family");
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let context = workspace(&home, &root, "local-member");
    let users = portable_users();
    UsersStore::save(&context, &users).unwrap();
    RegistryStore::from_path(config.join("brain/env.json"))
        .replace(&MachineRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            default_workspace: WorkspaceName::parse("family").unwrap(),
            workspaces: BTreeMap::from([(
                WorkspaceName::parse("family").unwrap(),
                WorkspaceRecord {
                    workspace_id: WorkspaceId::parse(WORKSPACE_ID).unwrap(),
                    root: root.clone(),
                    aliases: BTreeSet::new(),
                    local_user_id: "local-member".to_owned(),
                    receiver_enabled: false,
                    env: Map::new(),
                },
            )]),
            env: serde_json::Map::new(),
        })
        .unwrap();
    let requests = [
        RequestIdentity::Email {
            from: " Remote@Example.Test ",
        },
        RequestIdentity::Sms {
            from: "(212) 555-0123",
        },
    ];

    for (index, request) in requests.into_iter().enumerate() {
        let tasks_path = root.join("tasks/tasks.csv");
        let rows_before_rejection = std::fs::read(&tasks_path).ok();
        let rejected = resolve_authenticated_actor(
            false,
            &UserId::parse("local-member").unwrap(),
            request,
            &users,
        );
        assert!(matches!(
            rejected,
            Err(AuthenticatedActorError::ProviderAuthenticationFailed)
        ));
        assert_eq!(std::fs::read(&tasks_path).ok(), rows_before_rejection);
        let actor = resolve_authenticated_actor(
            true,
            &UserId::parse("local-member").unwrap(),
            request,
            &users,
        )
        .unwrap();
        assert_eq!(actor.user_id().as_str(), "remote-member");
        assert!(matches!(actor.channel(), Channel::Email | Channel::Sms));
        let mut command = Command::new("python3");
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/todo/scripts/add_task.py"))
            .args([
                "--name",
                &format!("Inbound task {}", index + 1),
                "--type",
                "personal",
                "--priority",
                "p2",
            ])
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_CACHE_HOME", &cache)
            .env("TMPDIR", temporary.path().join("tmp"))
            .env("PYTHONDONTWRITEBYTECODE", "1");
        for (key, value) in context.integration_env(&actor) {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        assert_success(&output);
    }

    let rows = csv::Reader::from_path(root.join("tasks/tasks.csv"))
        .unwrap()
        .deserialize::<BTreeMap<String, String>>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.get("assigned_to").map(String::as_str) == Some("remote-member"))
    );
}

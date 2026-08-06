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

#[test]
fn phone_and_email_resolve_to_one_portable_user() {
    let users = Users::parse(FIXTURE.as_bytes()).unwrap();

    assert_eq!(
        users.resolve_phone("(212) 555-0100").unwrap().id.as_str(),
        "pablo"
    );
    assert_eq!(
        users
            .resolve_email(" Wife@Example.COM ")
            .unwrap()
            .id
            .as_str(),
        "wife"
    );
    assert!(users.resolve_phone("+12125550101").is_none());
    assert!(users.resolve_email("wife+brain@example.com").is_none());
}

#[test]
fn identities_are_normalized_without_provider_specific_rewriting() {
    let users = Users::parse(FIXTURE.as_bytes()).unwrap();
    let pablo = users.user(&UserId::parse("pablo").unwrap()).unwrap();
    let wife = users.user(&UserId::parse("wife").unwrap()).unwrap();

    assert_eq!(pablo.phones[0].value, "+12125550100");
    assert_eq!(pablo.phones[1].value, "+12125550101");
    assert_eq!(pablo.emails[0].value, "pablo+brain@example.com");
    assert_eq!(wife.emails[0].value, "wife@example.com");
    assert_eq!(wife.emails[1].value, "wife+brain@example.com");
}

#[test]
fn one_enabled_sender_cannot_identify_two_users() {
    let error = Users::parse(DUPLICATE_PHONE_FIXTURE.as_bytes()).unwrap_err();

    assert!(matches!(error, UsersError::DuplicateInboundPhone { .. }));
}

#[test]
fn invalid_user_ids_contacts_and_response_addresses_are_typed_errors() {
    for invalid in ["", "Pablo", "pablo_s", "-pablo", "pablo-"] {
        assert!(UserId::parse(invalid).is_err(), "{invalid:?}");
    }
    let ambiguous_phone = FIXTURE.replace("(212) 555-0100", "555-0100");
    assert!(matches!(
        Users::parse(ambiguous_phone.as_bytes()),
        Err(UsersError::InvalidPhone { .. })
    ));
    let foreign_without_prefix = FIXTURE.replace("(212) 555-0100", "442071838750");
    assert!(matches!(
        Users::parse(foreign_without_prefix.as_bytes()),
        Err(UsersError::InvalidPhone { .. })
    ));
    let missing_email = FIXTURE.replace(
        "\"response_email\": \"pablo+brain@example.com\"",
        "\"response_email\": \"other@example.com\"",
    );
    assert!(matches!(
        Users::parse(missing_email.as_bytes()),
        Err(UsersError::ResponseEmailNotOnUser { .. })
    ));
}

#[test]
fn canonical_users_round_trip_is_byte_stable() {
    let users = Users::parse(FIXTURE.as_bytes()).unwrap();
    let canonical = users.to_bytes().unwrap();
    let reparsed = Users::parse(&canonical).unwrap();

    assert_eq!(reparsed.to_bytes().unwrap(), canonical);
    assert!(canonical.ends_with(b"\n"));
}

#[test]
fn workspace_store_loads_and_atomically_saves_canonical_users() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = workspace(fixture.path());
    let users = Users::parse(FIXTURE.as_bytes()).unwrap();

    UsersStore::save(&workspace, &users).unwrap();
    let stored = std::fs::read(UsersStore::path(&workspace)).unwrap();
    assert_eq!(stored, users.to_bytes().unwrap());
    assert_eq!(UsersStore::load(&workspace).unwrap(), users);
    assert_eq!(
        UsersStore::path(&workspace),
        fixture.path().join(".config/users.json")
    );
    assert_eq!(
        std::fs::read_dir(fixture.path().join(".config"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn selected_workspace_user_cli_adds_updates_lists_and_selects_the_local_person() {
    let fixture = CliFixture::new();

    let add = fixture.run(&[
        "-b",
        "family",
        "user",
        "add",
        "--id",
        "alex-smith",
        "--name",
        "Alex Smith",
        "--phone",
        "646-555-0100",
        "--email",
        "Alex@Example.COM",
        "--response-email",
        "alex@example.com",
    ]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );

    let update = fixture.run(&[
        "user",
        "update",
        "alex-smith",
        "--name",
        "Alex Rivera",
        "--add-phone",
        "+16465550101",
        "--add-email",
        "alex+brain@example.com",
        "-b",
        "family",
    ]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );

    let list = fixture.run(&["user", "list", "-b", "family"]);
    assert!(list.status.success());
    let stdout = String::from_utf8(list.stdout).unwrap();
    assert!(stdout.contains("alex-smith"));
    assert!(stdout.contains("Alex Rivera"));
    assert!(stdout.contains("+16465550100"));
    assert!(stdout.contains("alex@example.com"));

    let before_users = std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap();
    let local = fixture.run(&["-b", "family", "user", "local", "alex-smith"]);
    assert!(
        local.status.success(),
        "{}",
        String::from_utf8_lossy(&local.stderr)
    );
    assert_eq!(
        std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap(),
        before_users
    );
    let registry = RegistryStore::load_from(&fixture.registry_path).unwrap();
    assert_eq!(
        registry
            .select(Some("family"))
            .unwrap()
            .record()
            .local_user_id,
        "alex-smith"
    );
}

#[test]
fn removing_an_assigned_user_refuses_or_reassigns_tasks_without_partial_changes() {
    let fixture = CliFixture::new();
    let tasks_path = fixture.root.join("tasks/tasks.csv");
    std::fs::write(
        &tasks_path,
        "task_id,task_name,assignee,status\nT001,Plan trip,pablo,not_started\n",
    )
    .unwrap();
    let users_before = std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap();
    let tasks_before = std::fs::read(&tasks_path).unwrap();

    let refused = fixture.run(&["user", "remove", "pablo", "-b", "family"]);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("tasks remain assigned to pablo; use --reassign-to <USER_ID>")
    );
    assert_eq!(
        std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap(),
        users_before
    );
    assert_eq!(std::fs::read(&tasks_path).unwrap(), tasks_before);

    let invalid = fixture.run(&[
        "user",
        "remove",
        "pablo",
        "--reassign-to",
        "missing-user",
        "-b",
        "family",
    ]);
    assert!(!invalid.status.success());
    assert_eq!(
        std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap(),
        users_before
    );
    assert_eq!(std::fs::read(&tasks_path).unwrap(), tasks_before);

    let removed = fixture.run(&[
        "-b",
        "family",
        "user",
        "remove",
        "pablo",
        "--reassign-to",
        "wife",
    ]);
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(
        fixture
            .users()
            .user(&UserId::parse("pablo").unwrap())
            .is_none()
    );
    assert!(
        String::from_utf8(std::fs::read(&tasks_path).unwrap())
            .unwrap()
            .starts_with("task_id,task_name,assigned_to,status\nT001,Plan trip,wife,not_started")
    );
}

#[test]
fn reassigning_a_legacy_assignment_value_moves_work_without_inventing_a_person() {
    let fixture = CliFixture::new();
    let tasks_path = fixture.root.join("tasks/tasks.csv");
    let habits_path = fixture.root.join("tasks/habits.csv");
    std::fs::write(
        &tasks_path,
        "task_id,task_name,assigned_to,status\nT001,Plan trip,me,not_started\nT002,Rest,wife,not_started\n",
    )
    .unwrap();
    std::fs::write(&habits_path, "task_id,task_name,assigned_to\nH1,Walk,me\n").unwrap();
    let users_before = std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap();
    let tasks_before = std::fs::read(&tasks_path).unwrap();

    let unknown = fixture.run(&["user", "reassign", "me", "nobody", "-b", "family"]);
    assert!(!unknown.status.success());
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("nobody"),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
    assert_eq!(std::fs::read(&tasks_path).unwrap(), tasks_before);

    let reassigned = fixture.run(&["user", "reassign", "me", "pablo", "-b", "family"]);

    assert!(
        reassigned.status.success(),
        "{}",
        String::from_utf8_lossy(&reassigned.stderr)
    );
    assert_eq!(
        String::from_utf8(std::fs::read(&tasks_path).unwrap()).unwrap(),
        "task_id,task_name,assigned_to,status\nT001,Plan trip,pablo,not_started\nT002,Rest,wife,not_started\n"
    );
    assert_eq!(
        String::from_utf8(std::fs::read(&habits_path).unwrap()).unwrap(),
        "task_id,task_name,assigned_to\nH1,Walk,pablo\n"
    );
    assert_eq!(
        std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap(),
        users_before,
        "reassignment never adds or removes a portable person"
    );
}

#[test]
fn reassigning_an_absent_value_reports_it_and_leaves_every_task_byte_alone() {
    let fixture = CliFixture::new();
    let tasks_path = fixture.root.join("tasks/tasks.csv");
    std::fs::write(
        &tasks_path,
        "task_id,task_name,assigned_to\nT001,Plan trip,pablo\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("tasks/habits.csv"),
        "task_id,task_name,assigned_to\nH1,Walk,pablo\n",
    )
    .unwrap();
    let before = std::fs::read(&tasks_path).unwrap();

    let output = fixture.run(&["user", "reassign", "ghost", "wife", "-b", "family"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ghost"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(std::fs::read(&tasks_path).unwrap(), before);
}

#[test]
fn user_removal_collapses_both_assignment_headers_and_prefers_canonical_values() {
    let fixture = CliFixture::new();
    let tasks_path = fixture.root.join("tasks/tasks.csv");
    std::fs::write(
        &tasks_path,
        "task_id,task_name,assignee,assigned_to,status\nT001,Plan trip,pablo,wife,not_started\n",
    )
    .unwrap();

    let removed = fixture.run(&["user", "remove", "pablo", "-b", "family"]);

    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert_eq!(
        String::from_utf8(std::fs::read(tasks_path).unwrap()).unwrap(),
        "task_id,task_name,assigned_to,status\nT001,Plan trip,wife,not_started\n"
    );
}

#[test]
fn ordinary_commands_reject_a_local_user_not_in_the_portable_registry() {
    let fixture = CliFixture::new();
    let mut registry = RegistryStore::load_from(&fixture.registry_path).unwrap();
    registry
        .workspaces
        .get_mut(&WorkspaceName::parse("family").unwrap())
        .unwrap()
        .local_user_id = "missing-user".to_owned();
    RegistryStore::from_path(fixture.registry_path.clone())
        .replace(&registry)
        .unwrap();

    let output = fixture.run(&["config", "list", "-b", "family"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("local user missing-user is not a portable member"));
    assert!(stderr.contains("brain user local <USER_ID> -b family"));
}

#[test]
fn inactive_legacy_helper_proposes_the_first_user_without_guessing_other_people() {
    let proposal = propose_legacy_user_migration(
        "Alex Smith",
        None,
        " Alex@Example.COM ",
        &["+12125550100".to_owned()],
        &[
            "alex@example.com".to_owned(),
            "relative@example.com".to_owned(),
        ],
    )
    .unwrap();

    assert_eq!(proposal.user.id.as_str(), "alex-smith");
    assert_eq!(proposal.user.name, "Alex Smith");
    assert_eq!(
        proposal.user.response_email.as_deref(),
        Some("alex@example.com")
    );
    assert_eq!(proposal.user.emails.len(), 1);
    assert!(proposal.user.emails[0].inbound_allowed);
    assert_eq!(proposal.unresolved_phones, ["+12125550100"]);
    assert_eq!(proposal.unresolved_emails, ["relative@example.com"]);

    let overridden =
        propose_legacy_user_migration("Alex Smith", Some("alex"), "", &[], &[]).unwrap();
    assert_eq!(overridden.user.id.as_str(), "alex");
}

#[test]
fn legacy_response_email_without_an_allowlist_match_stays_unresolved() {
    let proposal = propose_legacy_user_migration(
        "Alex Smith",
        None,
        "response@example.com",
        &[],
        &["relative@example.com".to_owned()],
    )
    .unwrap();

    assert!(proposal.user.response_email.is_none());
    assert!(proposal.user.emails.is_empty());
    assert_eq!(
        proposal.unresolved_emails,
        ["relative@example.com", "response@example.com"]
    );
}

#[cfg(unix)]
#[test]
fn grouped_removal_preserves_owner_only_users_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = CliFixture::new();
    let users_path = UsersStore::path(&workspace(&fixture.root));
    std::fs::set_permissions(&users_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let removed = fixture.run(&[
        "user",
        "remove",
        "pablo",
        "--reassign-to",
        "wife",
        "-b",
        "family",
    ]);

    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    let mode = std::fs::metadata(users_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

use brain::cli::try_parse_from;
use brain::workspace::{
    BootstrapContext, InteractionMode, MachineRegistry, REGISTRY_SCHEMA_VERSION, ReadinessAction,
    ReadinessField, RegistryStore, WorkspaceName, WorkspaceRecord, bootstrap_with_io,
    readiness_action, readiness_action_with_users,
};
use brain::workspace::{BootstrapPolicy, Invocation, bootstrap_policy, invocation_for};
use brain::workspace::{ManifestError, WorkspaceId, WorkspaceManifest};
use serde_json::Map;
use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

use brain::users::UsersStore;

#[test]
fn legacy_readiness_accepts_exactly_valid_user_ids() {
    enum Expected {
        Invalid,
        Incomplete,
        Ready,
    }

    let temp = tempfile::tempdir().unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);
    let name = WorkspaceName::parse("family").unwrap();
    for (raw, expected) in [
        ("Pablo", Expected::Invalid),
        ("local_user", Expected::Invalid),
        (" pablo ", Expected::Invalid),
        ("", Expected::Incomplete),
        ("valid-kebab", Expected::Ready),
    ] {
        let record = WorkspaceRecord {
            workspace_id,
            root: temp.path().join("family"),
            aliases: BTreeSet::new(),
            local_user_id: raw.to_owned(),
            receiver_enabled: false,
            env: Map::new(),
        };
        let result = readiness_action_with_users(
            &name,
            &record,
            Ok(manifest.clone()),
            UsersStore::load_from(&temp.path().join(format!("missing-{raw:?}.json"))),
            InteractionMode::NonInteractive,
        );

        match expected {
            Expected::Invalid => {
                let error = result.unwrap_err();
                assert!(matches!(
                    error,
                    brain::workspace::ReadinessError::InvalidLegacyLocalUser { .. }
                ));
                assert!(
                    error
                        .to_string()
                        .contains("brain workspace repair -b family --local-user-id <USER_ID>")
                );
            }
            Expected::Incomplete => assert!(matches!(
                result,
                Err(brain::workspace::ReadinessError::Incomplete { .. })
            )),
            Expected::Ready => assert!(matches!(result, Ok(ReadinessAction::Ready(_)))),
        }
    }
}

fn users_named(ids: &[&str]) -> brain::users::Users {
    brain::users::Users {
        schema_version: brain::users::USERS_SCHEMA_VERSION,
        users: ids
            .iter()
            .map(|id| brain::users::User {
                id: brain::users::UserId::parse(id).unwrap(),
                name: (*id).to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            })
            .collect(),
    }
}

fn record_without_local_user(root: PathBuf, workspace_id: WorkspaceId) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id,
        root,
        aliases: BTreeSet::new(),
        local_user_id: String::new(),
        receiver_enabled: false,
        env: Map::new(),
    }
}

#[test]
fn sole_portable_user_is_auto_adopted_as_local_in_every_mode() {
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);
    let name = WorkspaceName::parse("brain").unwrap();
    let record = record_without_local_user(PathBuf::from("/brains/brain"), workspace_id);

    for mode in [
        InteractionMode::NonInteractive,
        InteractionMode::Interactive,
        InteractionMode::Internal,
    ] {
        let action = readiness_action_with_users(
            &name,
            &record,
            Ok(manifest.clone()),
            Ok(users_named(&["pablo"])),
            mode,
        )
        .unwrap();
        assert_eq!(
            action,
            ReadinessAction::AdoptLocalUser(brain::users::UserId::parse("pablo").unwrap()),
            "{mode:?}"
        );
    }
}

#[test]
fn several_portable_users_still_require_an_explicit_local_choice() {
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);
    let name = WorkspaceName::parse("family").unwrap();
    let record = record_without_local_user(PathBuf::from("/brains/family"), workspace_id);

    assert_eq!(
        readiness_action_with_users(
            &name,
            &record,
            Ok(manifest.clone()),
            Ok(users_named(&["pablo", "sam"])),
            InteractionMode::Interactive,
        )
        .unwrap(),
        ReadinessAction::Prompt(vec![ReadinessField::LocalUserId])
    );

    assert!(matches!(
        readiness_action_with_users(
            &name,
            &record,
            Ok(manifest),
            Ok(users_named(&["pablo", "sam"])),
            InteractionMode::NonInteractive,
        )
        .unwrap_err(),
        brain::workspace::ReadinessError::Incomplete { .. }
    ));
}

#[test]
fn an_explicitly_set_but_unknown_local_user_is_never_auto_adopted() {
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);
    let name = WorkspaceName::parse("brain").unwrap();
    let mut record = record_without_local_user(PathBuf::from("/brains/brain"), workspace_id);
    "ghost".clone_into(&mut record.local_user_id);

    let error = readiness_action_with_users(
        &name,
        &record,
        Ok(manifest),
        Ok(users_named(&["pablo"])),
        InteractionMode::NonInteractive,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        brain::workspace::ReadinessError::InvalidLocalUser { .. }
    ));
}

#[test]
fn headless_command_self_heals_a_sole_user_workspace_and_continues() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("brain");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    let canonical_name = WorkspaceName::parse("brain").unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    WorkspaceManifest::new(workspace_id)
        .write_new(&root)
        .unwrap();
    UsersStore::save_to(&root.join(".config/users.json"), &users_named(&["pablo"])).unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            record_without_local_user(root, workspace_id),
        )]),
    };
    let registry_path = config_home.path().join("brain/env.json");
    let store = RegistryStore::from_path(registry_path.clone());
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "brain"]).unwrap();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::NonInteractive,
        &mut std::io::empty(),
        &mut std::io::sink(),
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("a sole-user workspace must self-heal and continue headlessly");
    };
    assert_eq!(context.workspace.local_user_id(), "pablo");
    let healed = RegistryStore::load_from(&registry_path).unwrap();
    assert_eq!(
        healed.select(Some("brain")).unwrap().record().local_user_id,
        "pablo"
    );
}

#[test]
fn every_invocation_has_an_explicit_bootstrap_policy() {
    let cases = [
        (Invocation::Version, BootstrapPolicy::None),
        (Invocation::Help, BootstrapPolicy::None),
        (Invocation::AgentHook, BootstrapPolicy::InternalNoPrompt),
        (
            Invocation::InternalServer,
            BootstrapPolicy::InternalNoPrompt,
        ),
        (Invocation::WorkspaceCreate, BootstrapPolicy::RegistryOnly),
        (Invocation::WorkspaceAttach, BootstrapPolicy::RegistryOnly),
        (Invocation::WorkspaceRemove, BootstrapPolicy::RegistryOnly),
        (Invocation::WorkspaceRepair, BootstrapPolicy::RegistryOnly),
        (Invocation::User, BootstrapPolicy::RegistryOnly),
        (
            Invocation::WorkspaceList,
            BootstrapPolicy::ReadOnlyWorkspace,
        ),
        (Invocation::WorkspaceRename, BootstrapPolicy::ReadyWorkspace),
        (Invocation::WorkspaceAlias, BootstrapPolicy::ReadyWorkspace),
        (
            Invocation::WorkspaceDefault,
            BootstrapPolicy::ReadyWorkspace,
        ),
        (Invocation::Config, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Env, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Sync, BootstrapPolicy::ReadyWorkspace),
        (Invocation::SyncStatus, BootstrapPolicy::ReadOnlyWorkspace),
        (Invocation::Check, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Personalize, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Skills, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Server, BootstrapPolicy::None),
        (Invocation::ServerStatus, BootstrapPolicy::None),
        (Invocation::Receiver, BootstrapPolicy::ReadyWorkspace),
        (
            Invocation::ReceiverStatus,
            BootstrapPolicy::ReadOnlyWorkspace,
        ),
        (Invocation::Habits, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Reindex, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Tasks, BootstrapPolicy::ReadyWorkspace),
        (Invocation::TasksDoctor, BootstrapPolicy::ReadOnlyWorkspace),
        (Invocation::Tui, BootstrapPolicy::ReadyWorkspace),
    ];

    for (invocation, expected) in cases {
        assert_eq!(bootstrap_policy(invocation), expected, "{invocation:?}");
    }
}

#[test]
fn readiness_prompts_interactively_and_errors_actionably_when_headless() {
    let record = WorkspaceRecord {
        workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        root: PathBuf::from("/brains/family"),
        aliases: BTreeSet::new(),
        local_user_id: String::new(),
        receiver_enabled: false,
        env: Map::new(),
    };
    let missing = ManifestError::Io {
        operation: "read workspace manifest",
        path: PathBuf::from("/brains/family/.config/workspace.json"),
        kind: std::io::ErrorKind::NotFound,
        message: "not found".to_owned(),
    };

    assert_eq!(
        readiness_action(
            &WorkspaceName::parse("family").unwrap(),
            &record,
            Err(missing.clone()),
            InteractionMode::Interactive,
        )
        .unwrap(),
        ReadinessAction::Prompt(vec![ReadinessField::Manifest, ReadinessField::LocalUserId])
    );

    let error = readiness_action(
        &WorkspaceName::parse("family").unwrap(),
        &record,
        Err(missing),
        InteractionMode::NonInteractive,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("brain workspace repair -b family --manifest"));
    assert!(message.contains("brain user local <USER_ID> -b family"));
}

#[test]
fn readiness_rejects_a_manifest_for_a_different_registry_uuid() {
    let record = WorkspaceRecord {
        workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        root: PathBuf::from("/brains/family"),
        aliases: BTreeSet::new(),
        local_user_id: "pablo".to_owned(),
        receiver_enabled: false,
        env: Map::new(),
    };
    let body = br#"{"schema_version":1,"workspace_id":"e806258e-491a-436d-9db4-a5ca9903e0d4","receiver_ingress_id":"57b162df-983a-45c3-ac7e-bad94eb27a99","minimum_brain_version":"0.16.0"}"#;
    let manifest = WorkspaceManifest::parse(body, "0.16.0").unwrap();

    let error = readiness_action(
        &WorkspaceName::parse("family").unwrap(),
        &record,
        Ok(manifest),
        InteractionMode::NonInteractive,
    )
    .unwrap_err();

    assert!(error.to_string().contains("does not match registry UUID"));
}

#[test]
fn workspace_repair_creates_the_matching_manifest_and_sets_local_user() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    let create = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["workspace", "create", "--root", root.to_str().unwrap()])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    std::fs::remove_file(WorkspaceManifest::path(&root)).unwrap();

    let repair = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "pablo",
            "-b",
            "family",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        repair.status.success(),
        "{}",
        String::from_utf8_lossy(&repair.stderr)
    );

    let registry = RegistryStore::load_from(&config_home.path().join("brain/env.json")).unwrap();
    let selected = registry.select(Some("family")).unwrap();
    assert_eq!(selected.record().local_user_id, "pablo");
    let manifest = WorkspaceManifest::load(&root, env!("CARGO_PKG_VERSION")).unwrap();
    assert_eq!(manifest.workspace_id(), selected.record().workspace_id);
}

#[test]
fn first_create_is_registry_only_and_the_next_headless_command_names_the_exact_repair() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    let create = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["workspace", "create", "--root", root.to_str().unwrap()])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );

    let registry_path = config_home.path().join("brain/env.json");
    let registry = RegistryStore::load_from(&registry_path).unwrap();
    assert!(
        registry
            .select(None)
            .unwrap()
            .record()
            .local_user_id
            .is_empty()
    );
    assert!(WorkspaceManifest::path(&root).is_file());

    let blocked = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "list"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    let stderr = String::from_utf8(blocked.stderr).unwrap();
    assert!(stderr.contains("brain user add -b family --id <USER_ID> --name <DISPLAY_NAME>"));
    assert!(stderr.contains("brain user local <USER_ID> -b family"));
    assert!(
        !stderr.contains("--manifest"),
        "create already wrote the manifest: {stderr}"
    );

    let add = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "-b", "family", "user", "add", "--id", "pablo", "--name", "Pablo",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let local = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["user", "local", "pablo", "-b", "family"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        local.status.success(),
        "{}",
        String::from_utf8_lossy(&local.stderr)
    );

    let continued = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "list", "-b", "family"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        continued.status.success(),
        "{}",
        String::from_utf8_lossy(&continued.stderr)
    );
}

#[test]
fn workspace_repair_rejects_blank_local_user_without_changing_registry_bytes() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    let create = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["workspace", "create", "--root", root.to_str().unwrap()])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(create.status.success());
    let registry_path = config_home.path().join("brain/env.json");
    let before = std::fs::read(&registry_path).unwrap();

    let repair = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "repair",
            "--local-user-id",
            "   ",
            "-b",
            "family",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(!repair.status.success());
    assert!(String::from_utf8_lossy(&repair.stderr).contains("local user ID cannot be empty"));
    assert_eq!(std::fs::read(registry_path).unwrap(), before);
}

#[test]
fn interactive_bootstrap_repairs_then_continues_the_original_command() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    std::fs::create_dir_all(&root).unwrap();
    let canonical_name = WorkspaceName::parse("family").unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            WorkspaceRecord {
                workspace_id,
                root: root.clone(),
                aliases: BTreeSet::new(),
                local_user_id: String::new(),
                receiver_enabled: false,
                env: Map::new(),
            },
        )]),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "family"]).unwrap();
    let mut input = Cursor::new(b"Pablo\n\n".to_vec());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("ordinary config command must continue with a ready context");
    };
    assert_eq!(context.workspace.local_user_id(), "pablo");
    assert_eq!(context.workspace.id(), workspace_id);
    assert!(WorkspaceManifest::path(&root).is_file());
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "Your display name: User ID [pablo]: "
    );
}

#[test]
fn interactive_first_user_setup_uses_display_name_and_accepts_the_proposed_id() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    std::fs::create_dir_all(&root).unwrap();
    let canonical_name = WorkspaceName::parse("family").unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    WorkspaceManifest::new(workspace_id)
        .write_new(&root)
        .unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            WorkspaceRecord {
                workspace_id,
                root,
                aliases: BTreeSet::new(),
                local_user_id: String::new(),
                receiver_enabled: false,
                env: Map::new(),
            },
        )]),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "family"]).unwrap();
    let mut input = Cursor::new(b"Alex Smith\n\n".to_vec());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("first user setup must continue the command");
    };
    assert_eq!(context.workspace.local_user_id(), "alex-smith");
    let users = UsersStore::load(&context.workspace).unwrap();
    let user = users
        .user(&brain::users::UserId::parse("alex-smith").unwrap())
        .unwrap();
    assert_eq!(user.name, "Alex Smith");
    assert!(user.phones.is_empty());
    assert!(user.emails.is_empty());
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "Your display name: User ID [alex-smith]: "
    );
}

#[test]
fn first_user_setup_asks_for_contacts_only_for_configured_receiver_channels() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    WorkspaceManifest::new(workspace_id)
        .write_new(&root)
        .unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        br#"{"allowed_sms_senders":"+12125550100","allowed_email_senders":"alex@example.com,relative@example.com","response_email":"alex@example.com"}"#,
    )
    .unwrap();
    let canonical_name = WorkspaceName::parse("family").unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            WorkspaceRecord {
                workspace_id,
                root,
                aliases: BTreeSet::new(),
                local_user_id: String::new(),
                receiver_enabled: true,
                env: Map::new(),
            },
        )]),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "family"]).unwrap();
    let mut input = Cursor::new(b"Alex Smith\n\n\n\n".to_vec());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("configured receiver setup must continue the command");
    };
    let users = UsersStore::load(&context.workspace).unwrap();
    let user = &users.users[0];
    assert_eq!(user.phones[0].value, "+12125550100");
    assert_eq!(user.emails[0].value, "alex@example.com");
    assert_eq!(user.response_email.as_deref(), Some("alex@example.com"));
    let prompts = String::from_utf8(output).unwrap();
    assert!(prompts.contains("Phone [+12125550100]:"));
    assert!(prompts.contains("Email [alex@example.com]:"));
    assert!(
        !user
            .emails
            .iter()
            .any(|email| email.value == "relative@example.com")
    );
}

#[test]
fn response_email_alone_does_not_enable_or_prompt_for_an_email_identity() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    WorkspaceManifest::new(workspace_id)
        .write_new(&root)
        .unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        br#"{"response_email":"alex@example.com"}"#,
    )
    .unwrap();
    let canonical_name = WorkspaceName::parse("family").unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            WorkspaceRecord {
                workspace_id,
                root,
                aliases: BTreeSet::new(),
                local_user_id: String::new(),
                receiver_enabled: true,
                env: Map::new(),
            },
        )]),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "family"]).unwrap();
    let mut input = Cursor::new(b"Alex Smith\n\n".to_vec());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("response-only setup must continue without an email prompt");
    };
    let users = UsersStore::load(&context.workspace).unwrap();
    assert!(users.users[0].emails.is_empty());
    assert!(users.users[0].response_email.is_none());
    assert!(!String::from_utf8(output).unwrap().contains("Email"));
}

#[test]
fn manifest_parsing_is_strict_and_checks_compatibility() {
    let workspace_id = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
    let ingress_id = "e806258e-491a-436d-9db4-a5ca9903e0d4";
    let valid = format!(
        r#"{{"schema_version":1,"workspace_id":"{workspace_id}","receiver_ingress_id":"{ingress_id}","minimum_brain_version":"0.16.0"}}"#
    );

    let manifest = WorkspaceManifest::parse(valid.as_bytes(), "0.16.0").expect("valid manifest");
    assert_eq!(
        manifest.workspace_id(),
        WorkspaceId::parse(workspace_id).unwrap()
    );
    assert_eq!(
        manifest.receiver_ingress_id(),
        WorkspaceId::parse(ingress_id).unwrap()
    );

    let unknown = valid.replace('}', ",\"unexpected\":true}");
    assert!(matches!(
        WorkspaceManifest::parse(unknown.as_bytes(), "0.16.0"),
        Err(ManifestError::InvalidJson { .. })
    ));
    let unsupported = valid.replace("\"schema_version\":1", "\"schema_version\":2");
    assert!(matches!(
        WorkspaceManifest::parse(unsupported.as_bytes(), "0.16.0"),
        Err(ManifestError::UnsupportedSchema {
            found: 2,
            supported: 1
        })
    ));
    let incompatible = valid.replace("0.16.0", "0.17.0");
    assert!(matches!(
        WorkspaceManifest::parse(incompatible.as_bytes(), "0.16.0"),
        Err(ManifestError::IncompatibleBrainVersion { .. })
    ));
}

#[test]
fn writing_a_new_manifest_is_create_only_and_round_trips() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);

    manifest.write_new(fixture.path()).expect("first write");
    let original_bytes = std::fs::read(WorkspaceManifest::path(fixture.path())).unwrap();
    let loaded = WorkspaceManifest::load(fixture.path(), env!("CARGO_PKG_VERSION")).unwrap();
    assert_eq!(loaded, manifest);
    let replacement =
        WorkspaceManifest::new(WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap());
    let error = replacement.write_new(fixture.path()).unwrap_err();
    assert!(matches!(
        error,
        ManifestError::Io {
            kind: std::io::ErrorKind::AlreadyExists,
            ..
        }
    ));
    assert_eq!(
        std::fs::read(WorkspaceManifest::path(fixture.path())).unwrap(),
        original_bytes
    );
}

#[test]
fn parsed_routes_map_to_their_explicit_invocations() {
    let cases = [
        (vec!["brain"], Invocation::Tui),
        (
            vec!["brain", "workspace", "create", "--root", "/tmp/new"],
            Invocation::WorkspaceCreate,
        ),
        (
            vec!["brain", "workspace", "attach", "/tmp/existing"],
            Invocation::WorkspaceAttach,
        ),
        (
            vec!["brain", "workspace", "remove", "old"],
            Invocation::WorkspaceRemove,
        ),
        (
            vec![
                "brain",
                "workspace",
                "repair",
                "--manifest",
                "--local-user-id",
                "pablo",
            ],
            Invocation::WorkspaceRepair,
        ),
        (
            vec!["brain", "workspace", "list"],
            Invocation::WorkspaceList,
        ),
        (vec!["brain", "user", "list"], Invocation::User),
        (vec!["brain", "config"], Invocation::Config),
        (vec!["brain", "env"], Invocation::Env),
        (vec!["brain", "sync"], Invocation::Sync),
        (vec!["brain", "sync", "status"], Invocation::SyncStatus),
        (vec!["brain", "check"], Invocation::Check),
        (vec!["brain", "personalize"], Invocation::Personalize),
        (vec!["brain", "skills"], Invocation::Skills),
        (vec!["brain", "server", "status"], Invocation::ServerStatus),
        (vec!["brain", "server", "logs"], Invocation::Server),
        (
            vec![
                "brain",
                "server",
                "run",
                "--generation",
                "57b162df-983a-45c3-ac7e-bad94eb27a99",
                "--port",
                "8765",
            ],
            Invocation::InternalServer,
        ),
        (
            vec!["brain", "receiver", "status"],
            Invocation::ReceiverStatus,
        ),
        (vec!["brain", "receiver", "start"], Invocation::Receiver),
        (vec!["brain", "habits"], Invocation::Habits),
        (vec!["brain", "reindex"], Invocation::Reindex),
        (
            vec!["brain", "tasks", "today", "--no-tui"],
            Invocation::Tasks,
        ),
        (vec!["brain", "tasks", "doctor"], Invocation::TasksDoctor),
        (vec!["brain", "version"], Invocation::Version),
    ];

    for (argv, expected) in cases {
        let cli = try_parse_from(argv.clone()).expect("route parses");
        assert_eq!(invocation_for(&cli), expected, "{argv:?}");
    }
}

use brain::cli::try_parse_from;
use brain::workspace::{
    BootstrapContext, InteractionMode, MachineRegistry, REGISTRY_SCHEMA_VERSION, ReadinessAction,
    ReadinessField, RegistryStore, WorkspaceName, WorkspaceRecord, bootstrap_with_io,
    readiness_action,
};
use brain::workspace::{BootstrapPolicy, Invocation, bootstrap_policy, invocation_for};
use brain::workspace::{ManifestError, WorkspaceId, WorkspaceManifest};
use serde_json::Map;
use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

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
        (Invocation::WorkspaceList, BootstrapPolicy::ReadyWorkspace),
        (Invocation::WorkspaceRename, BootstrapPolicy::ReadyWorkspace),
        (Invocation::WorkspaceAlias, BootstrapPolicy::ReadyWorkspace),
        (
            Invocation::WorkspaceDefault,
            BootstrapPolicy::ReadyWorkspace,
        ),
        (Invocation::Config, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Env, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Sync, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Check, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Personalize, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Skills, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Server, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Receiver, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Habits, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Reindex, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Tasks, BootstrapPolicy::ReadyWorkspace),
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
    assert!(message.contains("brain workspace repair -b family --local-user-id <USER_ID>"));
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
    assert!(stderr.contains("brain workspace repair -b family --local-user-id <USER_ID>"));
    assert!(
        !stderr.contains("--manifest"),
        "create already wrote the manifest: {stderr}"
    );

    let repair = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "repair",
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
    let mut input = Cursor::new(b"\npablo\n".to_vec());
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
        "Local user ID (for example, pablo): A value is required.\nLocal user ID (for example, pablo): "
    );
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
        (vec!["brain", "config"], Invocation::Config),
        (vec!["brain", "env"], Invocation::Env),
        (vec!["brain", "sync"], Invocation::Sync),
        (vec!["brain", "check"], Invocation::Check),
        (vec!["brain", "personalize"], Invocation::Personalize),
        (vec!["brain", "skills"], Invocation::Skills),
        (vec!["brain", "server", "status"], Invocation::Server),
        (
            vec!["brain", "server", "run", "--port", "8765"],
            Invocation::InternalServer,
        ),
        (vec!["brain", "receiver", "status"], Invocation::Receiver),
        (vec!["brain", "habits"], Invocation::Habits),
        (vec!["brain", "reindex"], Invocation::Reindex),
        (
            vec!["brain", "tasks", "today", "--no-tui"],
            Invocation::Tasks,
        ),
        (vec!["brain", "version"], Invocation::Version),
    ];

    for (argv, expected) in cases {
        let cli = try_parse_from(argv.clone()).expect("route parses");
        assert_eq!(invocation_for(&cli), expected, "{argv:?}");
    }
}

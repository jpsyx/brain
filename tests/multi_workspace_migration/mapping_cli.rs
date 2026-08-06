use std::path::Path;
use std::process::Command;

use brain::migration::{
    MappingIssue, MappingResolution, MigrationGate, MigrationGateInput, apply_mapping_resolution,
    headless_mapping_remediation, mapping_issues,
};
use brain::users::{AssignmentRewrites, User, UserId, Users};

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

#[test]
fn mapping_preflight_reports_every_unresolved_sender_and_assignment() {
    let users = Users {
        schema_version: 1,
        users: vec![User {
            id: UserId::parse("alex").unwrap(),
            name: "Alex".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let config: brain::config::Config = serde_json::from_str(
        r#"{
            "allowed_sms_senders":"+12125550100,+12125550100",
            "allowed_email_senders":"relative@example.test,relative@example.test"
        }"#,
    )
    .unwrap();

    assert_eq!(
        mapping_issues(&users, &config, &["alex".to_owned(), "relative".to_owned()]),
        [
            MappingIssue::Phone("+12125550100".to_owned()),
            MappingIssue::Email("relative@example.test".to_owned()),
            MappingIssue::Assignment("relative".to_owned()),
        ]
    );
}

#[test]
fn configured_sync_requires_confirmation_or_explicit_acknowledged_headless_selection() {
    assert_eq!(
        brain::migration::migration_gate(MigrationGateInput {
            sync_configured: true,
            interactive: true,
            explicit_workspace: false,
            acknowledged_all_machines_updated: false,
        })
        .unwrap(),
        MigrationGate::ConfirmAllMachinesUpdated,
    );

    for input in [
        MigrationGateInput {
            sync_configured: true,
            interactive: false,
            explicit_workspace: false,
            acknowledged_all_machines_updated: true,
        },
        MigrationGateInput {
            sync_configured: true,
            interactive: false,
            explicit_workspace: true,
            acknowledged_all_machines_updated: false,
        },
    ] {
        let error = brain::migration::migration_gate(input).unwrap_err();
        assert!(
            error.to_string().contains("--brain <WORKSPACE>"),
            "{error:#}"
        );
        assert!(
            error
                .to_string()
                .contains("--acknowledge-all-machines-updated"),
            "{error:#}"
        );
    }

    assert_eq!(
        brain::migration::migration_gate(MigrationGateInput {
            sync_configured: true,
            interactive: false,
            explicit_workspace: true,
            acknowledged_all_machines_updated: true,
        })
        .unwrap(),
        MigrationGate::Proceed,
    );
}

#[test]
fn headless_mapping_failure_prints_only_shipped_user_command_forms() {
    let commands = headless_mapping_remediation(
        "family",
        &[
            MappingIssue::Phone("+12125550100".to_owned()),
            MappingIssue::Email("relative@example.test".to_owned()),
            MappingIssue::Assignment("relative".to_owned()),
        ],
    );

    assert!(commands.contains("brain user update <USER_ID> -b family --add-phone +12125550100"));
    assert!(
        commands
            .contains("brain user update <USER_ID> -b family --add-email relative@example.test")
    );
    assert!(commands.contains("brain user add -b family --id relative --name <DISPLAY_NAME>"));
    assert!(
        commands.contains("brain user reassign relative <EXISTING_USER_ID> -b family"),
        "an unmapped assignment may belong to someone already in the registry: {commands}"
    );
    assert!(!commands.contains("workspace user"));
}

#[test]
fn interactive_mapping_resolution_can_attach_sender_to_existing_or_create_assignment_user() {
    let mut users = Users {
        schema_version: 1,
        users: vec![User {
            id: UserId::parse("alex").unwrap(),
            name: "Alex".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };

    let mut rewrites = AssignmentRewrites::new();
    apply_mapping_resolution(
        &mut users,
        &mut rewrites,
        &MappingIssue::Phone("+12125550100".to_owned()),
        MappingResolution::Existing(UserId::parse("alex").unwrap()),
    )
    .unwrap();
    apply_mapping_resolution(
        &mut users,
        &mut rewrites,
        &MappingIssue::Assignment("relative".to_owned()),
        MappingResolution::New {
            id: UserId::parse("relative").unwrap(),
            name: "Relative".to_owned(),
        },
    )
    .unwrap();

    assert_eq!(
        users.resolve_phone("+12125550100").unwrap().id,
        UserId::parse("alex").unwrap()
    );
    assert_eq!(
        users
            .user(&UserId::parse("relative").unwrap())
            .unwrap()
            .name,
        "Relative"
    );
    assert!(
        rewrites.is_empty(),
        "an assignment that keeps its own ID needs no task rewrite"
    );
}

#[test]
fn an_assignment_can_adopt_an_existing_member_instead_of_creating_a_duplicate_person() {
    let mut users = Users {
        schema_version: 1,
        users: vec![User {
            id: UserId::parse("pablo").unwrap(),
            name: "Pablo".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let mut rewrites = AssignmentRewrites::new();

    for legacy in ["me", "Pablo S"] {
        apply_mapping_resolution(
            &mut users,
            &mut rewrites,
            &MappingIssue::Assignment(legacy.to_owned()),
            MappingResolution::Existing(UserId::parse("pablo").unwrap()),
        )
        .unwrap();
    }

    assert_eq!(users.users.len(), 1);
    assert_eq!(rewrites.apply("me"), "pablo");
    assert_eq!(rewrites.apply("Pablo S"), "pablo");
}

#[test]
fn adopting_an_unknown_member_for_an_assignment_is_rejected() {
    let mut users = Users {
        schema_version: 1,
        users: vec![User {
            id: UserId::parse("pablo").unwrap(),
            name: "Pablo".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let mut rewrites = AssignmentRewrites::new();

    let error = apply_mapping_resolution(
        &mut users,
        &mut rewrites,
        &MappingIssue::Assignment("me".to_owned()),
        MappingResolution::Existing(UserId::parse("nobody").unwrap()),
    )
    .unwrap_err();

    assert!(error.to_string().contains("nobody"), "{error:#}");
    assert!(rewrites.is_empty());
}

#[test]
fn headless_local_migration_runs_every_step_and_is_byte_idempotent() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home");
    let config_home = temporary.path().join("machine-config");
    let root = temporary.path().join("workspace");
    std::fs::create_dir_all(config_home.join("brain")).unwrap();
    std::fs::create_dir_all(root.join(".config")).unwrap();
    write_legacy_tasks(&root);
    std::fs::write(
        root.join(".config/config.json"),
        b"{\"enable_triage_habits\":false}\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".config/personalization.json"),
        b"{\"name\":\"Pablo\"}\n",
    )
    .unwrap();
    let registry = serde_json::json!({
        "schema_version": 2,
        "default_workspace": "family",
        "workspaces": {
            "family": {
                "workspace_id": WORKSPACE_ID,
                "root": root,
                "aliases": [],
                "local_user_id": "pablo",
                "receiver_enabled": false,
                "env": {}
            }
        }
    });
    std::fs::write(
        config_home.join("brain/env.json"),
        format!("{}\n", serde_json::to_string_pretty(&registry).unwrap()),
    )
    .unwrap();

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(["workspace", "migrate", "-b", "family"])
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &config_home)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let legacy_tasks = std::fs::read(root.join("tasks/tasks.csv")).unwrap();
    let legacy_config = std::fs::read(root.join(".config/config.json")).unwrap();
    let blocked = run();
    assert!(!blocked.status.success());
    assert!(
        String::from_utf8_lossy(&blocked.stderr)
            .contains("brain user add -b family --id alex --name <DISPLAY_NAME>")
    );
    assert_eq!(
        std::fs::read(root.join("tasks/tasks.csv")).unwrap(),
        legacy_tasks
    );
    assert_eq!(
        std::fs::read(root.join(".config/config.json")).unwrap(),
        legacy_config,
        "migration preflight must not seed portable access policy"
    );
    assert!(!root.join(".config/users.json").exists());
    assert!(!root.join(".config/workspace.json").exists());
    let cache = home.join(".cache/brain/workspaces").join(WORKSPACE_ID);
    assert!(!cache.join("migrations/multi-workspace-v1.json").exists());

    std::fs::write(
        root.join("tasks/tasks.csv"),
        b"task_id,task_name,assigned_to\nT1,Plan,pablo\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/habits.csv"),
        b"task_id,task_name,assigned_to\nH1,Walk,pablo\n",
    )
    .unwrap();
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_tasks = std::fs::read(root.join("tasks/tasks.csv")).unwrap();
    assert!(String::from_utf8_lossy(&first_tasks).starts_with("task_uuid,"));
    assert!(root.join(".config/users.json").is_file());
    assert!(root.join(".config/workspace.json").is_file());
    assert!(!cache.join("migrations/multi-workspace-v1.json").exists());
    assert_eq!(
        std::fs::read_dir(cache.join("migration-backups"))
            .unwrap()
            .count(),
        1
    );

    let second = run();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        std::fs::read(root.join("tasks/tasks.csv")).unwrap(),
        first_tasks
    );
    assert_eq!(
        std::fs::read_dir(cache.join("migration-backups"))
            .unwrap()
            .count(),
        1,
        "a completed rerun must not create another backup"
    );
}

fn write_legacy_tasks(root: &Path) {
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join("tasks/tasks.csv"),
        b"task_id,task_name,assigned_to\nT1,Plan,alex\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/habits.csv"),
        b"task_id,task_name,assigned_to\nH1,Walk,alex\n",
    )
    .unwrap();
    std::fs::write(root.join("tasks/SCHEMA.json"), b"{}\n").unwrap();
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use super::{
    ReceiverIntentRefresher, ReceiverStatus, apply_receiver_action_with,
    apply_startup_receiver_flag_with, receiver_status,
};
use crate::workspace::{
    CommandContext, MachineRegistry, ReceiverAction, RegistryStore, WorkspaceContext, WorkspaceId,
    WorkspaceName, WorkspaceRecord,
};

struct FailedRefresh;

impl ReceiverIntentRefresher for FailedRefresh {
    fn refresh_enabled(&self, _workspace_id: WorkspaceId) -> anyhow::Result<()> {
        anyhow::bail!("control socket disappeared")
    }
}

#[derive(Clone)]
struct RecordingRefresh(Arc<std::sync::Mutex<Vec<WorkspaceId>>>);

impl ReceiverIntentRefresher for RecordingRefresh {
    fn refresh_enabled(&self, workspace_id: WorkspaceId) -> anyhow::Result<()> {
        self.0.lock().unwrap().push(workspace_id);
        Ok(())
    }
}

#[test]
fn status_requires_persistent_intent_and_an_enabled_exact_lease_to_accept() {
    assert_eq!(
        receiver_status(true, true, Some(false)),
        ReceiverStatus {
            enabled: true,
            tui_live: true,
            server_running: true,
            accepting: false,
        }
    );
    assert_eq!(
        receiver_status(true, true, Some(true)),
        ReceiverStatus {
            enabled: true,
            tui_live: true,
            server_running: true,
            accepting: true,
        }
    );
    assert_eq!(
        receiver_status(true, false, None),
        ReceiverStatus {
            enabled: true,
            tui_live: false,
            server_running: false,
            accepting: false,
        }
    );
}

#[test]
fn cli_start_stop_and_startup_flag_drive_exact_persistence_and_refresh() {
    let temporary = tempfile::tempdir().expect("receiver fixture");
    let personal_name = WorkspaceName::parse("personal").unwrap();
    let family_name = WorkspaceName::parse("family").unwrap();
    let personal_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let family_id = WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
    let family_root = temporary.path().join("family");
    let store = RegistryStore::from_path(temporary.path().join("env.json"));
    store
        .replace(&MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (
                    personal_name.clone(),
                    WorkspaceRecord {
                        workspace_id: personal_id,
                        root: temporary.path().join("personal"),
                        aliases: BTreeSet::new(),
                        local_user_id: "personal-user".to_owned(),
                        receiver_enabled: false,
                        env: serde_json::Map::new(),
                    },
                ),
                (
                    family_name.clone(),
                    WorkspaceRecord {
                        workspace_id: family_id,
                        root: family_root.clone(),
                        aliases: BTreeSet::new(),
                        local_user_id: "family-user".to_owned(),
                        receiver_enabled: false,
                        env: serde_json::Map::new(),
                    },
                ),
            ]),
        })
        .unwrap();
    let workspace = WorkspaceContext::new(
        temporary.path(),
        family_id,
        family_name.clone(),
        &family_root,
        "family-user",
        temporary.path(),
    )
    .unwrap();
    let context = CommandContext::for_test(Arc::new(workspace), store.clone(), "family-user");
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let refresher = RecordingRefresh(Arc::clone(&calls));

    for (command, expected) in [("start", true), ("stop", false)] {
        let cli = crate::cli::try_parse_from(["brain", "-b", "family", "receiver", command])
            .expect("parse receiver command");
        let Some(crate::cli::Cmd::Receiver(args)) = cli.command else {
            panic!("receiver command");
        };
        super::super::run_receiver_with_refresher(&args, &context, &refresher).unwrap();
        let saved = RegistryStore::load_from(store.path()).unwrap();
        assert_eq!(saved.workspaces[&family_name].receiver_enabled, expected);
        assert!(!saved.workspaces[&personal_name].receiver_enabled);
    }

    let cli = crate::cli::try_parse_from(["brain", "--with-receiver", "-b", "family"])
        .expect("parse startup flag");
    apply_startup_receiver_flag_with(cli.with_receiver, &context, &refresher).unwrap();
    let saved = RegistryStore::load_from(store.path()).unwrap();
    assert!(saved.workspaces[&family_name].receiver_enabled);
    assert!(!saved.workspaces[&personal_name].receiver_enabled);
    assert_eq!(*calls.lock().unwrap(), [family_id, family_id, family_id]);
}

#[test]
fn committed_intent_survives_a_failed_live_refresh() {
    let temporary = tempfile::tempdir().expect("receiver fixture");
    let name = WorkspaceName::parse("personal").expect("workspace name");
    let workspace_id =
        WorkspaceId::parse("2174fb9d-ae76-4bde-a526-38ac43ebdf8f").expect("workspace ID");
    let root = temporary.path().join("personal");
    let store = RegistryStore::from_path(temporary.path().join("env.json"));
    store
        .replace(&MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: name.clone(),
            workspaces: BTreeMap::from([(
                name.clone(),
                WorkspaceRecord {
                    workspace_id,
                    root: root.clone(),
                    aliases: BTreeSet::new(),
                    local_user_id: "tester".to_owned(),
                    receiver_enabled: false,
                    env: serde_json::Map::new(),
                },
            )]),
        })
        .expect("seed registry");
    let workspace = WorkspaceContext::new(
        temporary.path(),
        workspace_id,
        name,
        &root,
        "tester",
        &PathBuf::from("/"),
    )
    .expect("workspace context");
    let context = CommandContext::for_test(Arc::new(workspace), store.clone(), "tester");

    let outcome = apply_receiver_action_with(&context, ReceiverAction::Start, &FailedRefresh)
        .expect("persistence success must remain success");

    assert!(outcome.enabled());
    assert!(outcome.refresh_warning().is_some());
    let saved = RegistryStore::load_from(store.path()).expect("saved registry");
    assert!(saved.workspaces[&WorkspaceName::parse("personal").unwrap()].receiver_enabled);
}

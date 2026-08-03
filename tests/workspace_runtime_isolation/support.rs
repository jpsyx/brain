use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use brain::workspace::{
    CommandContext, MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
    WorkspaceRecord,
};
use serde_json::{Map, json};
use tempfile::TempDir;

pub(crate) struct Fixture {
    _home: TempDir,
    pub(crate) store: RegistryStore,
    pub(crate) personal: CommandContext,
    pub(crate) family: CommandContext,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let home = tempfile::tempdir().expect("temporary home");
        let personal_root = home.path().join("personal");
        let family_root = home.path().join("family");
        std::fs::create_dir_all(&personal_root).expect("personal root");
        std::fs::create_dir_all(&family_root).expect("family root");

        let personal_name = WorkspaceName::parse("personal").expect("valid name");
        let family_name = WorkspaceName::parse("family").expect("valid name");
        let personal_id =
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("valid id");
        let family_id =
            WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").expect("valid id");

        let mut workspaces = BTreeMap::new();
        workspaces.insert(
            personal_name.clone(),
            WorkspaceRecord {
                workspace_id: personal_id,
                root: personal_root.clone(),
                aliases: BTreeSet::new(),
                local_user_id: "pablo".to_owned(),
                receiver_enabled: false,
                env: Map::from_iter([("claude_cmd".to_owned(), json!("personal"))]),
            },
        );
        workspaces.insert(
            family_name.clone(),
            WorkspaceRecord {
                workspace_id: family_id,
                root: family_root.clone(),
                aliases: BTreeSet::new(),
                local_user_id: "pablo".to_owned(),
                receiver_enabled: false,
                env: Map::from_iter([("claude_cmd".to_owned(), json!("family"))]),
            },
        );
        let registry = MachineRegistry {
            schema_version: 2,
            default_workspace: personal_name.clone(),
            workspaces,
        };
        let store = RegistryStore::from_path(home.path().join("config/brain/env.json"));
        store.replace(&registry).expect("write registry");

        let personal_workspace = Arc::new(
            WorkspaceContext::new(
                home.path(),
                personal_id,
                personal_name,
                &personal_root,
                "pablo",
                home.path(),
            )
            .expect("personal context"),
        );
        let family_workspace = Arc::new(
            WorkspaceContext::new(
                home.path(),
                family_id,
                family_name,
                &family_root,
                "pablo",
                home.path(),
            )
            .expect("family context"),
        );
        let users = brain::users::Users {
            schema_version: brain::users::USERS_SCHEMA_VERSION,
            users: vec![brain::users::User {
                id: brain::users::UserId::parse("pablo").expect("fixture user"),
                name: "Workspace member".to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            }],
        };
        brain::users::UsersStore::save(&personal_workspace, &users).expect("personal users");
        brain::users::UsersStore::save(&family_workspace, &users).expect("family users");

        Self {
            _home: home,
            personal: CommandContext::new(personal_workspace, store.clone())
                .expect("personal command actor"),
            family: CommandContext::new(family_workspace, store.clone())
                .expect("family command actor"),
            store,
        }
    }
}

pub(crate) fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, path: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if !path.exists() {
            return;
        }
        if path.is_file() {
            snapshot.insert(
                path.strip_prefix(base)
                    .expect("snapshot descendant")
                    .to_path_buf(),
                std::fs::read(path).expect("snapshot file bytes"),
            );
            return;
        }
        let mut entries = std::fs::read_dir(path)
            .expect("snapshot directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("snapshot entries");
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            visit(base, &entry.path(), snapshot);
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

pub(crate) fn write_session_response(workspace: &WorkspaceContext, session: &str, body: &[u8]) {
    let actor = brain::actor::local_actor(workspace).expect("fixture local actor");
    let env = brain::session::env_for(
        workspace,
        &actor,
        brain::session::AgentKind::Claude,
        session,
        i32::try_from(std::process::id()).expect("test PID fits i32"),
        &workspace.paths().state_db(),
        session,
    );
    let response_dir = env
        .iter()
        .find(|(name, _)| name == "BRAIN_RESPONSE_DIR")
        .map(|(_, value)| PathBuf::from(value))
        .expect("session response dir");
    std::fs::create_dir_all(&response_dir).expect("response dir");
    std::fs::write(response_dir.join(format!("{session}.json")), body).expect("response bytes");
}

pub(crate) fn sync_run(note: &str) -> brain::sync::journal::SyncRun {
    brain::sync::journal::SyncRun {
        started_at: "2026-08-02T00:00:00Z".to_owned(),
        finished_at: "2026-08-02T00:00:01Z".to_owned(),
        direction: "both".to_owned(),
        outcome: "clean".to_owned(),
        transferred: 1,
        deleted: 0,
        conflicts: 0,
        errors: 0,
        note: note.to_owned(),
    }
}

pub(crate) fn write_csv_baseline(workspace: &WorkspaceContext, suffix: &str) {
    let tasks = workspace.root().join("tasks/tasks.csv");
    if let Some(parent) = tasks.parent() {
        std::fs::create_dir_all(parent).expect("tasks dir");
    }
    let local = format!("task_id,status,notes,last_touched\n{suffix},open,local,t1\n");
    std::fs::write(&tasks, local).expect("local tasks CSV");
    let remote = format!("task_id,status,notes,last_touched\n{suffix},open,remote,t2\n");
    let outcome = brain::sync::csv_sync::sync_one(
        workspace.paths(),
        &tasks,
        "tasks/tasks.csv",
        || Some(remote.clone()),
        |_| true,
    );
    assert_eq!(outcome.name, "tasks.csv");
}

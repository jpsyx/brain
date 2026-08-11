use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use brain::workspace::{
    CommandContext, MachineRegistry, RegistryStore, Requirement, RequirementScope,
    RequirementStatus, WorkspaceContext, WorkspaceId, WorkspaceManifest, WorkspaceName,
    WorkspaceRecord,
};
use serde_json::{Map, Value};

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

pub(crate) fn feature_status(
    requirements: &[Requirement],
    scope: &RequirementScope,
) -> brain::workspace::FeatureStatus {
    requirements
        .iter()
        .find(|requirement| requirement.scope() == scope)
        .and_then(|requirement| match requirement.status() {
            RequirementStatus::Feature(status) => Some(status),
            RequirementStatus::Required(_) => None,
        })
        .unwrap_or_else(|| panic!("missing optional requirement for {scope:?}"))
}

pub(crate) fn required_status(
    requirements: &[Requirement],
    scope: &RequirementScope,
) -> brain::workspace::RequiredStatus {
    requirements
        .iter()
        .find(|requirement| requirement.scope() == scope)
        .and_then(|requirement| match requirement.status() {
            RequirementStatus::Required(status) => Some(status),
            RequirementStatus::Feature(_) => None,
        })
        .unwrap_or_else(|| panic!("missing required requirement for {scope:?}"))
}

pub(crate) struct Fixture {
    _temporary: tempfile::TempDir,
    pub(crate) command: CommandContext,
}

impl Fixture {
    pub(crate) fn new(env: Map<String, Value>) -> Self {
        Self::with_receiver(env, false)
    }

    /// Machine-global values are filed where a real `brain env set` would put
    /// them — the registry's top-level map — so a fixture cannot claim a
    /// workspace owns the machine's receiver origin.
    pub(crate) fn with_receiver(env: Map<String, Value>, receiver_enabled: bool) -> Self {
        let (machine_env, env): (Map<String, Value>, Map<String, Value>) = env
            .into_iter()
            .partition(|(name, _)| brain::env::is_machine_global(name));
        let temporary = tempfile::tempdir().expect("temporary workspace home");
        let root = temporary.path().join("brain");
        std::fs::create_dir_all(root.join(".config")).expect("workspace config directory");
        std::fs::create_dir_all(root.join("tasks")).expect("workspace tasks directory");
        let workspace_id = WorkspaceId::parse(WORKSPACE_ID).expect("workspace UUID");
        WorkspaceManifest::new(workspace_id)
            .write_new(&root)
            .expect("workspace manifest");
        std::fs::write(
            root.join(".config/users.json"),
            concat!(
                "{\n",
                "  \"schema_version\": 1,\n",
                "  \"users\": [{\"id\": \"pablo\", \"name\": \"Pablo\", ",
                "\"phones\": [], \"emails\": [], \"response_email\": null}]\n",
                "}\n"
            ),
        )
        .expect("portable users");
        std::fs::write(
            root.join(".config/config.json"),
            "{\"access_mode\":\"unrestricted\",\"enable_triage_habits\":false}\n",
        )
        .expect("portable config");
        let name = WorkspaceName::parse("brain").expect("workspace name");
        let registry = MachineRegistry {
            schema_version: brain::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: name.clone(),
            workspaces: BTreeMap::from([(
                name.clone(),
                WorkspaceRecord {
                    workspace_id,
                    root: root.clone(),
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled,
                    env,
                },
            )]),
            env: machine_env,
        };
        let store = RegistryStore::from_path(temporary.path().join(".config/brain/env.json"));
        store.replace(&registry).expect("machine registry");
        let workspace = WorkspaceContext::new(
            temporary.path(),
            workspace_id,
            name,
            &root,
            "pablo",
            Path::new("/"),
        )
        .expect("workspace context");
        let command = CommandContext::new(Arc::new(workspace), store).expect("command context");
        Self {
            _temporary: temporary,
            command,
        }
    }

    pub(crate) fn write_config(&self, body: &str) {
        std::fs::write(
            self.command.workspace.root().join(".config/config.json"),
            body,
        )
        .expect("portable config");
    }

    pub(crate) fn write_users(&self, body: &str) {
        std::fs::write(
            self.command.workspace.root().join(".config/users.json"),
            body,
        )
        .expect("portable users");
    }
}

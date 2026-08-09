use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use crate::server::lifecycle::{IngressId, LeaseId, WorkspaceLease};
use crate::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, WorkspaceId, WorkspaceName, WorkspaceRecord,
};

pub(super) fn lease(expires_at: Instant) -> WorkspaceLease {
    WorkspaceLease {
        lease_id: LeaseId::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5").unwrap(),
        workspace_id: workspace_id(),
        canonical_name: WorkspaceName::parse("personal").unwrap(),
        ingress_id: ingress(),
        tui_pid: std::process::id(),
        job_socket: PathBuf::from("/tmp/jobs.sock"),
        receiver_enabled: true,
        expires_at,
    }
}

pub(super) fn ingress() -> IngressId {
    IngressId::parse("a4f0ec11-d121-4f58-aa44-2448ba427b76").unwrap()
}

pub(super) fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse("2174fb9d-ae76-4bde-a526-38ac43ebdf8f").unwrap()
}

pub(super) fn registry_with_receiver(receiver_enabled: bool) -> MachineRegistry {
    let name = WorkspaceName::parse("personal").unwrap();
    MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: name.clone(),
        workspaces: BTreeMap::from([(
            name,
            WorkspaceRecord {
                workspace_id: workspace_id(),
                root: PathBuf::from("/tmp/workspace"),
                aliases: BTreeSet::new(),
                local_user_id: "tester".to_owned(),
                receiver_enabled,
                env: serde_json::Map::new(),
            },
        )]),
        env: serde_json::Map::new(),
    }
}

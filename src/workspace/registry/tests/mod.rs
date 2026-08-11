use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::*;
use crate::workspace::{WorkspaceId, WorkspaceName};

mod mutation;
mod serde;
mod store;
mod validation;

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

fn name(value: &str) -> WorkspaceName {
    WorkspaceName::parse(value).expect("valid workspace name")
}

fn id(value: &str) -> WorkspaceId {
    WorkspaceId::parse(value).expect("valid workspace ID")
}

fn record(workspace_id: &str, root: &str, sentinel: &str) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: id(workspace_id),
        root: PathBuf::from(root),
        aliases: BTreeSet::new(),
        local_user_id: format!("{sentinel}-user"),
        receiver_enabled: sentinel == "personal",
        env: Map::from_iter([("sentinel".to_owned(), json!(sentinel))]),
    }
}

fn registry_with_brain_and_family() -> MachineRegistry {
    let mut family = record(FAMILY_ID, "/workspaces/family", "family");
    family.aliases.insert(name("fam"));
    MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: name("brain"),
        workspaces: BTreeMap::from([
            (
                name("brain"),
                record(PERSONAL_ID, "/workspaces/brain", "personal"),
            ),
            (name("family"), family),
        ]),
        env: serde_json::Map::new(),
    }
}

fn valid_registry_json() -> Value {
    json!({
        "schema_version": REGISTRY_SCHEMA_VERSION,
        "default_workspace": "brain",
        "workspaces": {
            "brain": {
                "workspace_id": PERSONAL_ID,
                "root": "/workspaces/brain",
                "aliases": [],
                "local_user_id": "personal"
            },
            "family": {
                "workspace_id": FAMILY_ID,
                "root": "/workspaces/family",
                "aliases": ["fam"],
                "local_user_id": "family"
            }
        }
    })
}

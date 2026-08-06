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

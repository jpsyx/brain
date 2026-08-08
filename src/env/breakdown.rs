//! The `brain env` breakdown: the machine-global values in `env.json` plus one
//! fully resolved env block per registered workspace.
//!
//! "Global" means exactly what it means in the file: every top-level key that is
//! **not** under `workspaces`. Everything else belongs to one workspace record
//! and is resolved against that record's own root, so a non-selected workspace
//! never borrows the selected one's values.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::schema::{REDACTED, is_sensitive};
use crate::settings::Resolved;
use crate::workspace::{CommandContext, MachineRegistry, WorkspaceId};

/// The top-level `env.json` key that holds per-workspace records. Everything
/// beside it is machine-global.
const WORKSPACES_KEY: &str = "workspaces";

/// Descriptions for the machine-global keys brain itself owns.
const GLOBAL_VARS: [(&str, &str); 2] = [
    (
        "schema_version",
        "Workspace-registry schema version of this machine's env.json.",
    ),
    (
        "default_workspace",
        "Canonical workspace brain selects when no --workspace/-w is given.",
    ),
];

/// What an undeclared machine-global key is described as.
const GLOBAL_FALLBACK_DESCRIPTION: &str =
    "Machine-global value from env.json (outside every workspace record).";

/// One documented variable: what a row name means. Nested dotted paths are
/// deliberately absent — their description only says where they came from, which
/// the rendered footnote covers once instead of per path.
pub(crate) struct VarDoc {
    pub name: String,
    pub description: String,
}

/// One workspace's resolved env block.
pub(crate) struct WorkspaceEnv {
    pub name: String,
    pub is_default: bool,
    pub is_selected: bool,
    pub rows: Vec<Resolved>,
}

/// The whole `brain env` view: where the registry lives, its machine-global
/// values, and every workspace's env.
pub(crate) struct Breakdown {
    pub registry_path: PathBuf,
    pub global: Vec<Resolved>,
    pub workspaces: Vec<WorkspaceEnv>,
    pub variables: Vec<VarDoc>,
}

/// Read the machine registry and assemble the breakdown. Thin IO shell over
/// [`assemble`]; an unreadable or non-object registry yields an empty view
/// rather than failing the command.
pub(crate) fn collect(command: &CommandContext) -> Breakdown {
    let path = command.registry_store.path().to_path_buf();
    let raw = crate::settings::load_map_at(&path);
    let registry = crate::workspace::RegistryStore::load_from(&path).ok();
    assemble(path, &raw, registry.as_ref(), command.workspace.id())
}

/// Pure assembly of the breakdown from the raw `env.json` object plus its typed
/// form. `raw` drives the global rows so any top-level key outside `workspaces`
/// shows up, declared or not.
pub(crate) fn assemble(
    registry_path: PathBuf,
    raw: &Map<String, Value>,
    registry: Option<&MachineRegistry>,
    selected: WorkspaceId,
) -> Breakdown {
    let global = global_rows(raw);
    Breakdown {
        registry_path,
        variables: variable_docs(&global),
        global,
        workspaces: registry.map_or_else(Vec::new, |registry| workspace_blocks(registry, selected)),
    }
}

/// The legend: every machine-global row followed by every declared per-workspace
/// variable, each with its description.
fn variable_docs(global: &[Resolved]) -> Vec<VarDoc> {
    global
        .iter()
        .map(|row| VarDoc {
            name: row.name.clone(),
            description: row.description.clone(),
        })
        .chain(
            super::schema::declared_docs().map(|(name, description)| VarDoc {
                name: name.to_owned(),
                description: description.to_owned(),
            }),
        )
        .collect()
}

fn global_rows(raw: &Map<String, Value>) -> Vec<Resolved> {
    let global = raw
        .iter()
        .filter(|(name, _)| name.as_str() != WORKSPACES_KEY)
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<Map<String, Value>>();
    super::vars::flatten_map(&global)
        .into_iter()
        .map(|(name, value)| Resolved {
            value: if is_sensitive(&name) {
                Some(REDACTED.to_owned())
            } else {
                super::vars::value_to_string(&value)
            },
            description: GLOBAL_VARS
                .iter()
                .find(|(declared, _)| *declared == name)
                .map_or(GLOBAL_FALLBACK_DESCRIPTION, |(_, description)| description)
                .to_owned(),
            name,
        })
        .collect()
}

fn workspace_blocks(registry: &MachineRegistry, selected: WorkspaceId) -> Vec<WorkspaceEnv> {
    registry
        .workspaces
        .iter()
        .map(|(name, record)| WorkspaceEnv {
            name: name.to_string(),
            is_default: name == &registry.default_workspace,
            is_selected: record.workspace_id == selected,
            rows: rows_for(&record.root, &record.env),
        })
        .collect()
}

/// Env rows for one workspace record, resolved against its own root. Structural
/// registry fields never leak into the free-form env rows, matching what the
/// selected-workspace store exposes.
fn rows_for(root: &Path, env: &Map<String, Value>) -> Vec<Resolved> {
    let mut env = env.clone();
    env.retain(|name, _| !super::schema::is_structural(name));
    super::vars::resolve_all_at(root, &env)
}

#[cfg(test)]
mod tests;

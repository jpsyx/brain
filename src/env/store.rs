//! Machine-local env access for one already-selected workspace.

use anyhow::Result;
use serde_json::{Map, Value};

/// Read the store as a JSON object. A missing/unreadable/non-object file yields
/// an empty map — a broken env never blocks startup.
#[must_use]
pub(crate) fn load_map(command: &crate::workspace::CommandContext) -> Map<String, Value> {
    let Ok(registry) = crate::workspace::RegistryStore::load_from(command.registry_store.path())
    else {
        return Map::new();
    };
    let Ok(selected) = registry.select(Some(command.workspace.name().as_str())) else {
        return Map::new();
    };
    if selected.record().workspace_id != command.workspace.id() {
        return Map::new();
    }
    let mut map = selected.record().env.clone();
    map.retain(|name, _| !super::schema::is_structural(name));
    map
}

pub(super) fn save_map(
    command: &crate::workspace::CommandContext,
    map: &Map<String, Value>,
) -> Result<()> {
    if let Some(name) = map.keys().find(|name| super::schema::is_structural(name)) {
        anyhow::bail!("env key `{name}` is structural and read-only");
    }
    command
        .registry_store
        .transaction(|transaction| -> Result<()> {
            let mut registry = transaction.load()?;
            let selected = registry.select(Some(command.workspace.name().as_str()))?;
            if selected.record().workspace_id != command.workspace.id() {
                anyhow::bail!("selected workspace identity changed before env update");
            }
            let canonical_name = selected.canonical_name().clone();
            registry
                .workspaces
                .get_mut(&canonical_name)
                .ok_or_else(|| anyhow::anyhow!("selected workspace record disappeared"))?
                .env
                .clone_from(map);
            transaction.save(&registry)?;
            Ok(())
        })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use serde_json::{Map, json};

    use super::save_map;
    use crate::workspace::{
        CommandContext, MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId,
        WorkspaceName, WorkspaceRecord,
    };

    #[test]
    fn save_boundary_rejects_structural_env_leakage_without_rewriting_registry() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("family");
        std::fs::create_dir_all(&root).unwrap();
        let name = WorkspaceName::parse("family").unwrap();
        let id = WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
        let registry = MachineRegistry {
            schema_version: 2,
            default_workspace: name.clone(),
            workspaces: BTreeMap::from([(
                name.clone(),
                WorkspaceRecord {
                    workspace_id: id,
                    root: root.clone(),
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env: Map::new(),
                },
            )]),
        };
        let store = RegistryStore::from_path(home.path().join("config/brain/env.json"));
        store.replace(&registry).unwrap();
        let context = CommandContext::new(
            Arc::new(
                WorkspaceContext::new(home.path(), id, name, &root, "pablo", home.path()).unwrap(),
            ),
            store.clone(),
        )
        .unwrap();
        let before = std::fs::read(store.path()).unwrap();
        let map = Map::from_iter([("root".to_owned(), json!("/escaped"))]);

        assert!(save_map(&context, &map).is_err());
        assert_eq!(std::fs::read(store.path()).unwrap(), before);
    }
}

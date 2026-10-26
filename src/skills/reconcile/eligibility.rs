//! Positive old-generation identity captured before any setup registration.
use crate::workspace::{RegistryStore, WorkspaceRecord};
use anyhow::Result;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Default)]
pub(crate) struct LegacyRoots {
    ids: BTreeSet<String>,
    roots: BTreeSet<PathBuf>,
}

impl LegacyRoots {
    pub(crate) fn capture(home: &Path, store: &RegistryStore) -> Result<Self> {
        if !store.path().exists() {
            let root = crate::workspace::registry::known_legacy_root(
                home,
                store.path().parent().expect("registry parent"),
                &serde_json::json!({}),
            );
            return Ok(Self {
                ids: BTreeSet::new(),
                roots: root.into_iter().collect(),
            });
        }
        match RegistryStore::load_readable(store.path()) {
            Ok(registry) => Ok(Self {
                ids: registry
                    .workspaces
                    .values()
                    .map(|record| record.workspace_id.to_string())
                    .collect(),
                roots: BTreeSet::new(),
            }),
            Err(original) => {
                let value = serde_json::from_slice(&std::fs::read(store.path())?)?;
                let Some(root) = crate::workspace::registry::known_legacy_root(
                    home,
                    store.path().parent().expect("registry parent"),
                    &value,
                ) else {
                    return Err(original.into());
                };
                Ok(Self {
                    ids: BTreeSet::new(),
                    roots: BTreeSet::from([root]),
                })
            }
        }
    }

    pub(crate) fn contains(&self, record: &WorkspaceRecord) -> bool {
        self.ids.contains(&record.workspace_id.to_string()) || self.roots.contains(&record.root)
    }
}

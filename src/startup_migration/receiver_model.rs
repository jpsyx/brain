use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(super) struct WorkspaceState {
    pub(super) path: PathBuf,
    pub(super) workspace_id: String,
    pub(super) local_user_id: String,
}

pub(super) fn up(home: &Path) -> Result<()> {
    for state in workspace_states(home) {
        crate::state::Db::open_path_with_legacy_identity(
            &state.path,
            &state.workspace_id,
            &state.local_user_id,
        )
        .with_context(|| format!("upgrade receiver state {}", state.path.display()))?;
    }
    Ok(())
}

pub(super) fn down(home: &Path) -> Result<()> {
    for state in workspace_states(home) {
        crate::state::receiver_schema_down(&state.path)
            .with_context(|| format!("downgrade receiver state {}", state.path.display()))?;
    }
    Ok(())
}

pub(super) fn workspace_states(home: &Path) -> Vec<WorkspaceState> {
    let store = crate::workspace::RegistryStore::real();
    if !store.path().exists() {
        return Vec::new();
    }
    let Ok(registry) = crate::workspace::RegistryStore::load_readable(store.path()) else {
        // Automatic migrations run before legacy registry bootstrap. Without a
        // validated registry there is no trustworthy workspace UUID to target.
        return Vec::new();
    };
    registry
        .workspaces
        .values()
        .map(|record| WorkspaceState {
            path: crate::workspace::WorkspacePaths::new(home, record.workspace_id).state_db(),
            workspace_id: record.workspace_id.to_string(),
            local_user_id: record.local_user_id.clone(),
        })
        .filter(|state| state.path.is_file())
        .collect()
}

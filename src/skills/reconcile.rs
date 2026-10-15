//! Receipt-aware reconciliation without selected workspace bootstrap.
mod lock;
use super::{
    install,
    install_transaction::{Currentness, InstallIdentity, InstallVersion, LockedInstall},
    receipt::WorkspaceStamp,
};
use crate::workspace::{RegistryStore, WorkspacePaths, WorkspaceRecord};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum WorkspaceRefresh {
    Current {
        workspace_id: String,
    },
    Updated {
        workspace_id: String,
    },
    PendingUnavailable {
        workspace_id: String,
        root: PathBuf,
    },
    Failed {
        workspace_id: String,
        message: String,
    },
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct NamedRefresh {
    pub name: String,
    #[serde(flatten)]
    pub outcome: WorkspaceRefresh,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum RefreshMode {
    Compatibility,
    Explicit,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct RefreshReport {
    pub workspaces: Vec<NamedRefresh>,
    pub refresh_error: Option<String>,
}
impl RefreshReport {
    pub(crate) fn failed(&self) -> bool {
        self.refresh_error.is_some()
            || self
                .workspaces
                .iter()
                .any(|entry| matches!(entry.outcome, WorkspaceRefresh::Failed { .. }))
    }
}

pub(crate) fn all(home: &Path, store: &RegistryStore, mode: RefreshMode) -> RefreshReport {
    match enumerate(home, store, mode) {
        Ok(workspaces) => RefreshReport {
            workspaces,
            refresh_error: None,
        },
        Err(error) => RefreshReport {
            workspaces: Vec::new(),
            refresh_error: Some(format!("{error}")),
        },
    }
}
fn enumerate(home: &Path, store: &RegistryStore, mode: RefreshMode) -> Result<Vec<NamedRefresh>> {
    match std::fs::symlink_metadata(store.path()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        other => {
            other.context("inspect workspace registry")?;
        }
    }
    let registry = RegistryStore::load_readable(store.path()).context("read workspace registry")?;
    Ok(registry
        .workspaces
        .iter()
        .map(|(name, record)| NamedRefresh {
            name: name.to_string(),
            outcome: refresh_record(home, record, mode),
        })
        .collect())
}
fn refresh_record(home: &Path, record: &WorkspaceRecord, mode: RefreshMode) -> WorkspaceRefresh {
    let workspace_id = record.workspace_id.to_string();
    match std::fs::metadata(&record.root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return WorkspaceRefresh::PendingUnavailable {
                workspace_id,
                root: record.root.clone(),
            };
        }
        Err(error) => {
            return WorkspaceRefresh::Failed {
                workspace_id,
                message: format!("inspect workspace root: {error}"),
            };
        }
        Ok(_) => {}
    }
    let paths = WorkspacePaths::new(home, record.workspace_id);
    let result = InstallIdentity::registered(&record.root, record.workspace_id, paths.cache_dir())
        .context("validate registered workspace identity")
        .and_then(|identity| {
            refresh(
                &identity,
                &sources(&record.root),
                &mut WorkspaceStamp::registered(&paths, record.workspace_id, &record.local_user_id),
                mode,
            )
        });
    match result {
        Ok(true) => WorkspaceRefresh::Updated { workspace_id },
        Ok(false) => WorkspaceRefresh::Current { workspace_id },
        Err(error) => WorkspaceRefresh::Failed {
            workspace_id,
            message: format!("{error}"),
        },
    }
}
pub(crate) fn sources(root: &Path) -> install::Sources {
    install::Sources {
        extensions_dir: Some(root.join(".config/extensions")),
        plugins_dir: Some(root.join(".config/plugins")),
    }
}
pub(crate) fn refresh(
    identity: &InstallIdentity,
    sources: &install::Sources,
    stamp: &mut WorkspaceStamp,
    mode: RefreshMode,
) -> Result<bool> {
    let mut locked = match mode {
        RefreshMode::Compatibility => lock::acquire(identity),
        RefreshMode::Explicit => LockedInstall::acquire(identity),
    }
    .context("acquire managed skill install lock")?;
    let recovered = locked
        .recover(stamp)
        .context("recover managed skill installation")?
        .is_some();
    let version = InstallVersion::current();
    let currentness = locked
        .inspect(&version)
        .context("verify managed skill ownership and currentness")?;
    let rendered = if matches!(mode, RefreshMode::Explicit) {
        Some(install::render_sources(sources).context("read and render workspace skill sources")?)
    } else {
        None
    };
    if let Currentness::Current(receipt) = &currentness {
        let source_current = rendered
            .as_ref()
            .map(|rendered| {
                locked
                    .sources_current(rendered, &version)
                    .context("validate requested skill sources against managed ownership")
            })
            .transpose()?
            .unwrap_or(true);
        if stamp.read().context("read managed skill receipt")?.as_ref() == Some(receipt)
            && source_current
        {
            return Ok(recovered);
        }
    }
    let rendered = match rendered {
        Some(rendered) => rendered,
        None => {
            install::render_sources(sources).context("read and render workspace skill sources")?
        }
    };
    locked
        .install(rendered, &version, stamp)
        .context("publish managed skills and persist receipt")?;
    Ok(true)
}

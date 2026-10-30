//! Receipt-aware reconciliation without selected workspace bootstrap.
mod current;
mod eligibility;
mod lock;
pub(crate) mod package;
use super::diagnostic::Diagnostic;
use super::{
    install,
    install_transaction::{Currentness, InstallIdentity, InstallVersion, LockedInstall},
    receipt::WorkspaceStamp,
};
use crate::workspace::{RegistryStore, WorkspacePaths, WorkspaceRecord};
use anyhow::{Context, Result};
pub(crate) use eligibility::LegacyRoots;
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
    /// Another sync or skill update owned the workspace for the whole bounded
    /// wait. The receipt stays stale, so the next invocation retries; the
    /// active sync reconciles anything it receives itself.
    Deferred {
        workspace_id: String,
    },
    Failed {
        workspace_id: String,
        message: String,
        diagnostic: Diagnostic,
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
    all_with_legacy(home, store, mode, &LegacyRoots::default())
}
pub(crate) fn all_with_legacy(
    home: &Path,
    store: &RegistryStore,
    mode: RefreshMode,
    legacy_roots: &LegacyRoots,
) -> RefreshReport {
    match enumerate(home, store, mode, legacy_roots) {
        Ok(workspaces) => RefreshReport {
            workspaces,
            refresh_error: None,
        },
        Err(error) => RefreshReport {
            workspaces: Vec::new(),
            refresh_error: Some(Diagnostic::from_error(&error, "read_registry", None).to_string()),
        },
    }
}
fn enumerate(
    home: &Path,
    store: &RegistryStore,
    mode: RefreshMode,
    legacy_roots: &LegacyRoots,
) -> Result<Vec<NamedRefresh>> {
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
            outcome: refresh_record(home, record, mode, legacy_roots.contains(record)),
        })
        .collect())
}
fn refresh_record(
    home: &Path,
    record: &WorkspaceRecord,
    mode: RefreshMode,
    legacy_machine: bool,
) -> WorkspaceRefresh {
    let workspace_id = record.workspace_id.to_string();
    match std::fs::metadata(&record.root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return WorkspaceRefresh::PendingUnavailable {
                workspace_id,
                root: record.root.clone(),
            };
        }
        Err(error) => {
            return failure(
                workspace_id,
                Diagnostic::from_error(&error.into(), "inspect_workspace", None),
            );
        }
        Ok(_) => {}
    }
    let paths = WorkspacePaths::new(home, record.workspace_id);
    let result = InstallIdentity::registered(&record.root, record.workspace_id, paths.cache_dir())
        .context("validate registered workspace identity")
        .and_then(|identity| {
            let machine = crate::access::MachineCapabilityEnvironment::for_skill_ownership(
                record.workspace_id,
                &record.env,
            )?;
            crate::access::preflight_bundled_skill_ownership(&machine)?;
            refresh_with_legacy(
                &identity,
                &sources(&record.root),
                &mut WorkspaceStamp::registered(&paths, record.workspace_id, &record.local_user_id),
                mode,
                legacy_machine,
            )
        });
    outcome(workspace_id, mode, result)
}

/// Classify one workspace's refresh. Lock contention is transient: automatic
/// compatibility defers it instead of failing the invoking command, while an
/// explicit repair reports it as `sync_busy`.
fn outcome(workspace_id: String, mode: RefreshMode, result: Result<bool>) -> WorkspaceRefresh {
    match result {
        Ok(true) => WorkspaceRefresh::Updated { workspace_id },
        Ok(false) => WorkspaceRefresh::Current { workspace_id },
        Err(error) if matches!(mode, RefreshMode::Compatibility) && is_contention(&error) => {
            WorkspaceRefresh::Deferred { workspace_id }
        }
        Err(error) => failure(
            workspace_id,
            Diagnostic::from_error(&error, "reconcile", None),
        ),
    }
}

/// Whether an error only means another sync or skill update owns the workspace.
pub(crate) fn is_contention(error: &anyhow::Error) -> bool {
    lock::is_contention(error)
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
    refresh_with_legacy(identity, sources, stamp, mode, false)
}
fn refresh_with_legacy(
    identity: &InstallIdentity,
    sources: &install::Sources,
    stamp: &mut WorkspaceStamp,
    mode: RefreshMode,
    legacy_machine: bool,
) -> Result<bool> {
    if matches!(mode, RefreshMode::Compatibility) && current::matches(identity, stamp)? {
        return Ok(false);
    }
    let _sync = match mode {
        RefreshMode::Compatibility => lock::acquire_sync(identity),
        RefreshMode::Explicit => identity.acquire_sync(),
    }?;
    let mut locked = match mode {
        RefreshMode::Compatibility => lock::acquire(identity),
        RefreshMode::Explicit => LockedInstall::acquire(identity),
    }
    .context("acquire managed skill install lock")?;
    Ok(synchronize_locked(&mut locked, sources, stamp, mode, legacy_machine)?.0)
}

pub(crate) fn synchronize_locked(
    locked: &mut LockedInstall,
    sources: &install::Sources,
    stamp: &mut dyn super::install_transaction::StampStore,
    mode: RefreshMode,
    legacy_machine: bool,
) -> Result<(bool, install::Report)> {
    let receive = locked.identity().receive_phase()?;
    anyhow::ensure!(
        receive != Some(crate::sync::journal::ReceivePhase::Receiving),
        Diagnostic::new(super::diagnostic::Category::ReceivePending, "receive", None)
    );
    let mode = if receive.is_some() {
        RefreshMode::Explicit
    } else {
        mode
    };
    let result = publish_locked(
        locked,
        sources,
        stamp,
        mode,
        legacy_machine,
        receive.is_none(),
    )
    .map_err(|error| {
        if receive.is_some() {
            Diagnostic::receive_conflict(&error).into()
        } else {
            error
        }
    })?;
    if receive.is_some() {
        locked.identity().clear_receive()?;
    }
    Ok(result)
}

/// The verified receive caller already owns the UUID sync boundary.
pub(crate) fn after_receive(workspace: &crate::workspace::WorkspaceContext) -> Result<()> {
    let identity = InstallIdentity::workspace(workspace)?;
    let mut locked = lock::acquire(&identity)?;
    synchronize_locked(
        &mut locked,
        &sources(workspace.root()),
        &mut WorkspaceStamp::selected(workspace),
        RefreshMode::Explicit,
        false,
    )?;
    Ok(())
}

fn publish_locked(
    locked: &mut LockedInstall,
    sources: &install::Sources,
    stamp: &mut dyn super::install_transaction::StampStore,
    mode: RefreshMode,
    legacy_machine: bool,
    resume_committed: bool,
) -> Result<(bool, install::Report)> {
    let recovered = locked
        .recover(stamp)
        .map_err(|error| Diagnostic::from_error(&error, "recover_installation", None))?;
    let version = InstallVersion::current();
    let manifest = locked.manifest()?;
    let complete = current::complete(manifest.as_ref());
    if complete
        && let Ok(Currentness::Current(receipt)) = locked.inspect(&version)
        && ((resume_committed
            && recovered
                .as_ref()
                .is_some_and(|restored| restored.receipt == receipt))
            || (matches!(mode, RefreshMode::Compatibility)
                && stamp.read().context("read managed skill receipt")?.as_ref() == Some(&receipt)))
    {
        let repaired = recovered.is_some();
        return Ok((
            repaired,
            recovered
                .filter(|restored| restored.receipt == receipt)
                .map_or(
                    install::Report {
                        installed: Vec::new(),
                        pruned: Vec::new(),
                        retained: Vec::new(),
                    },
                    |restored| restored.report,
                ),
        ));
    }
    let package = package::prepare(locked, sources, legacy_machine)
        .map_err(|error| Diagnostic::from_error(&error, "prepare_package", None))?;
    let report = locked
        .preflight_package(&package, &version)
        .map_err(|error| Diagnostic::from_error(&error, "preflight_package", None))?;
    if complete && let Ok(Currentness::Current(receipt)) = locked.inspect(&version) {
        let prior = manifest
            .as_ref()
            .and_then(|manifest| manifest.package.as_ref())
            .and_then(|package| package.instructions.as_ref());
        let instructions_current = package.instructions.as_ref().is_some_and(|document| {
            !document.changed && prior.is_some_and(|owned| owned.receipt == document.receipt)
        });
        if instructions_current
            && manifest
                .as_ref()
                .and_then(|manifest| manifest.package.as_ref())
                .and_then(|metadata| metadata.extension_snapshot.as_ref())
                == package.extension_snapshot.as_ref()
            && package.config_patches.is_empty()
            && package.source_patches.is_empty()
            && locked.sources_current(&package.skills, &version)?
        {
            let stamp_changed = stamp.read()?.as_ref() != Some(&receipt);
            if stamp_changed {
                stamp
                    .write(&receipt)
                    .map_err(|_| Diagnostic::receipt_write())?;
            }
            return Ok((recovered.is_some() || stamp_changed, report));
        }
    }
    locked
        .install_package(package, &version, stamp)
        .map_err(|error| Diagnostic::from_error(&error, "publish_package", None))?;
    Ok((true, report))
}

fn failure(workspace_id: String, diagnostic: Diagnostic) -> WorkspaceRefresh {
    WorkspaceRefresh::Failed {
        workspace_id,
        message: diagnostic.to_string(),
        diagnostic,
    }
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    use crate::skills::install_transaction::SyncBusy;

    fn busy() -> anyhow::Error {
        anyhow::Error::new(SyncBusy).context("acquire workspace sync")
    }

    #[test]
    fn automatic_refresh_defers_while_another_sync_owns_the_workspace() {
        let outcome = outcome("ws".into(), RefreshMode::Compatibility, Err(busy()));
        assert!(matches!(outcome, WorkspaceRefresh::Deferred { .. }), "{outcome:?}");
        let report = RefreshReport {
            workspaces: vec![NamedRefresh { name: "brain".into(), outcome }],
            refresh_error: None,
        };
        assert!(!report.failed(), "contention is not a refresh failure");
    }

    #[test]
    fn explicit_refresh_reports_contention_as_sync_busy_not_ownership() {
        let outcome = outcome("ws".into(), RefreshMode::Explicit, Err(busy()));
        let WorkspaceRefresh::Failed { diagnostic, .. } = outcome else {
            panic!("explicit contention must still fail: {outcome:?}");
        };
        let text = diagnostic.to_string();
        assert!(text.starts_with("sync_busy"), "{text}");
        assert!(!text.contains("ownership"), "{text}");
    }

    #[test]
    fn other_errors_still_fail_automatic_refresh() {
        let outcome = outcome(
            "ws".into(),
            RefreshMode::Compatibility,
            Err(anyhow::anyhow!("broken")),
        );
        assert!(matches!(outcome, WorkspaceRefresh::Failed { .. }));
    }
}

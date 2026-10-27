//! Installer preparation settles only durable receive evidence, before target preflight.
use crate::{
    skills::install_transaction::InstallIdentity,
    sync::{args::Direction, config::SyncConfig, journal::ReceivePhase},
    workspace::{MachineRegistry, RegistryStore, WorkspaceContext},
};
use anyhow::{Result, ensure};
use std::{fs, path::Path};

pub(super) fn prepare(
    home: &Path,
    store: &RegistryStore,
    registry: &MachineRegistry,
    recovery_binary: Option<&Path>,
) -> Result<()> {
    let mut failed = false;
    for (name, record) in &registry.workspaces {
        if matches!(fs::metadata(&record.root), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        let workspace = WorkspaceContext::new(
            home,
            record.workspace_id,
            name.clone(),
            &record.root,
            &record.local_user_id,
            home,
        )?;
        let result = (|| {
            let identity = InstallIdentity::workspace(&workspace).map_err(|error| {
                crate::skills::diagnostic::Diagnostic::from_error(&error, "receive_identity", None)
            })?;
            let Some(phase) = identity.receive_phase()? else {
                return Ok(());
            };
            eprintln!(
                "{}",
                crate::theme::Theme::active()
                    .info(&format!("{name}: preparing interrupted receive recovery…"))
            );
            let _sync = identity.acquire_sync()?;
            ensure!(
                RegistryStore::load_readable(store.path())? == *registry,
                "workspace registry changed before receive recovery"
            );
            ensure!(
                identity.receive_phase()? == Some(phase),
                "receive state changed before recovery"
            );
            crate::migration::require_no_active_rollout(workspace.paths())?;
            ensure!(
                crate::sync::conflicts::list_conflicts(workspace.root()).is_empty()
                    && crate::sync::conflicts::leftover_markers(workspace.root()) == 0,
                "unresolved sync conflicts; review both copies before retrying installation"
            );
            match phase {
                ReceivePhase::Receiving => {
                    let config = record
                        .env
                        .get("sync")
                        .cloned()
                        .map(serde_json::from_value::<SyncConfig>)
                        .transpose()
                        .map_err(|_| anyhow::anyhow!("invalid workspace sync configuration; correct the workspace sync block before retrying"))?
                        .unwrap_or_default();
                    super::super::receive::transfer(&workspace, &config, Direction::Pull, true)?;
                }
                ReceivePhase::Reconcile => crate::skills::reconcile::after_receive(&workspace)?,
            }
            ensure!(
                identity.receive_phase()?.is_none(),
                "receive recovery is still pending"
            );
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(error) = result {
            failed = true;
            let executable = recovery_binary.map_or_else(
                || "brain".to_owned(),
                |path| crate::session::shell_quote(&path.to_string_lossy()),
            );
            eprintln!("{}", crate::theme::Theme::active().error(&format!("{name} ({}): receive preparation failed: {error:#}\n  Recover with: {executable} sync --pull -w {name}", record.workspace_id)));
        }
    }
    ensure!(
        !failed,
        "workspace receive preparation has pending failures; installed executable retained"
    );
    Ok(())
}

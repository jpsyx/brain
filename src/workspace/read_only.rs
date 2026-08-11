//! Selected-workspace bootstrap for literal read-only status probes.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use super::{
    BootstrapContext, CommandContext, InteractionMode, ReadinessAction, RegistryStore,
    WorkspaceContext, WorkspaceManifest, readiness_action_with_users,
};

pub(super) fn bootstrap(
    cli: &crate::cli::Cli,
    store: &RegistryStore,
    home: &Path,
    current_dir: &Path,
) -> Result<BootstrapContext> {
    // Read-only probes tolerate an older schema in memory: they must not write
    // the upgrade, and refusing to report status until some other command runs
    // would be a worse answer than reporting it.
    let registry = RegistryStore::load_readable(store.path())?;
    let selected = registry.select(cli.workspace_selector.as_deref())?;
    super::selector::remember_selected(selected.canonical_name());
    super::bootstrap::validate_expected_workspace_id(
        std::env::var_os("BRAIN_WORKSPACE_ID").as_deref(),
        selected.record().workspace_id,
    )?;
    if !selected.record().root.is_dir() {
        anyhow::bail!(
            "workspace root {} is unavailable",
            selected.record().root.display()
        );
    }
    let workspace = WorkspaceContext::new(
        home,
        selected.record().workspace_id,
        selected.canonical_name().clone(),
        &selected.record().root,
        selected.record().local_user_id.clone(),
        current_dir,
    )?;
    let readiness = readiness_action_with_users(
        selected.canonical_name(),
        selected.record(),
        WorkspaceManifest::load(&selected.record().root, env!("CARGO_PKG_VERSION")),
        crate::users::UsersStore::load_from(&crate::users::UsersStore::path(&workspace)),
        InteractionMode::NonInteractive,
    )?;
    if !matches!(readiness, ReadinessAction::Ready(_)) {
        anyhow::bail!(
            "workspace {} needs readiness repair before status can be read",
            selected.canonical_name()
        );
    }
    Ok(BootstrapContext::Ready(CommandContext::new_read_only(
        Arc::new(workspace),
        store.clone(),
    )?))
}

/// A read-only context for a registered workspace this command did not select.
///
/// Machine-wide inventories report on every registered workspace, not just the
/// selected one, so they need a context per record. `None` means that record is
/// unreadable on this machine; a half-configured peer must not take the whole
/// inventory down with it.
#[must_use]
pub(crate) fn peer_context(
    name: &super::WorkspaceName,
    record: &super::WorkspaceRecord,
) -> Option<CommandContext> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let current_dir = std::env::current_dir().ok()?;
    let workspace = WorkspaceContext::new(
        &home,
        record.workspace_id,
        name.clone(),
        &record.root,
        record.local_user_id.clone(),
        &current_dir,
    )
    .ok()?;
    CommandContext::new_read_only(Arc::new(workspace), RegistryStore::real()).ok()
}

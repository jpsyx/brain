use std::path::Path;

use anyhow::Result;

use super::RegistryStore;
use crate::workspace::selector;

/// Fill in the workspace an invocation acts on when it named none.
///
/// `-w` wins; then the workspace a Brain-launched process was launched for
/// (`BRAIN_WORKSPACE`); then the workspace whose root contains the current
/// directory, discovered by walking upward the way git finds its repository;
/// then the machine default.
///
/// The launching workspace deliberately outranks the current directory: an agent
/// panel opened for `family` stays on `family` even while it reads files under
/// another root, and `BRAIN_WORKSPACE_ID` validation stays consistent with it.
pub(super) fn resolve_workspace_selector(
    cli: &mut crate::cli::Cli,
    store: &RegistryStore,
    current_dir: &Path,
) {
    let inherited = std::env::var(selector::WORKSPACE_ENV).ok();
    if let Some(selector) =
        selector::effective_selector(cli.workspace_selector.as_deref(), inherited.as_deref())
    {
        cli.workspace_selector = Some(selector);
        return;
    }
    cli.workspace_selector = discover_workspace_from(store, current_dir);
}

/// The registered workspace containing `current_dir`, if any.
///
/// Roots and the current directory are canonicalized before comparison: on macOS
/// a `/tmp` root and a `/private/tmp` current directory are the same place, and a
/// lexical comparison would miss it. An unreadable registry simply discovers
/// nothing, leaving the machine default.
pub(super) fn discover_workspace_from(store: &RegistryStore, current_dir: &Path) -> Option<String> {
    let registry = RegistryStore::load_readable(store.path()).ok()?;
    let roots = registry
        .workspaces
        .iter()
        .map(|(name, record)| {
            (
                name.as_str().to_owned(),
                record
                    .root
                    .canonicalize()
                    .unwrap_or_else(|_| record.root.clone()),
            )
        })
        .collect::<Vec<_>>();
    let here = current_dir
        .canonicalize()
        .unwrap_or_else(|_| current_dir.to_path_buf());
    selector::discover_from_ancestors(&roots, &here)
}

/// Refuse a strict child that names no workspace.
///
/// Brain sets `BRAIN_REQUIRE_WORKSPACE` on the children it spawns for its own
/// work, so any code path that builds a `brain …` command without `-w` fails
/// here instead of quietly operating on whichever workspace is default. An
/// ordinary interactive invocation is unaffected.
pub(super) fn enforce_strict_selector(cli: &crate::cli::Cli) -> Result<()> {
    if selector::violates_strict_selector(
        selector::strict_selector_required(),
        cli.workspace_selector.is_some(),
    ) {
        anyhow::bail!(
            "{} is set, so this command must name its workspace explicitly with -w/--workspace; \
             Brain spawned it and a missing selector would silently target the default workspace",
            selector::STRICT_ENV
        );
    }
    Ok(())
}

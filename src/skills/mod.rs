//! The brain skill pipeline.
//!
//! Renders the bundled (embedded) skills and the user's plugins, injecting each
//! skill's extension, and installs them into the selected workspace's
//! `.agents/skills` directory, fanning out to project-local Claude, Codex, and
//! OpenCode skill directories.
//!
//! Sub-project A shipped `resync_skills()` as a no-op seam. B1 filled in the
//! render/install/fan-out pipeline and the `brain skills sync` command; B2 adds
//! extensions (inject into a built copy at named hooks) and plugins (whole user
//! skills). `resync_skills()` runs the pipeline but is **gated by the routine preference**
//! (`skills_auto_sync`) so a `config`/`personalize` mutation can opt out of
//! workspace skill writes.

pub mod command;
pub(crate) mod compatibility;
pub(crate) mod diagnostic;
pub mod embed;
pub mod extension;
pub(crate) mod generation;
pub mod install;
pub(crate) mod install_transaction;
pub mod layout;
pub mod model;
pub mod plugin;
pub mod prune;
pub(crate) mod receipt;
pub(crate) mod reconcile;
pub mod render;
pub mod repair;

pub use install::WorkspaceCapabilityReport;

/// Render a selected workspace/actor skill view without touching registries.
pub fn render_workspace_capabilities(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &crate::actor::ActorContext,
    plan: &crate::access::CapabilityPlan,
) -> anyhow::Result<WorkspaceCapabilityReport> {
    let generation = generation::prepare_launch(workspace)?;
    generation::render_capabilities(&generation, workspace, actor, plan)
}

/// The brain version currently running, the authority for rendered workspace
/// skills. When the installed skills were rendered by a different version,
/// they are stale (see [`needs_resync`]).
#[must_use]
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether the installed skills must be re-rendered.
///
/// True when the running binary's version differs from the version that last
/// rendered them for this workspace; a missing stamp (never rendered by a
/// version-aware binary) also means yes. Any difference (including a downgrade)
/// re-renders, so workspace skills always match the running binary. Pure.
#[must_use]
pub fn needs_resync(stamped: Option<&str>, current: &str) -> bool {
    stamped != Some(current)
}

/// Re-render and install the brain skills after a config/personalize mutation.
///
/// Gated by the `skills_auto_sync` config flag (default true); a disabled or
/// failed sync must never fail the mutation that triggered it.
pub fn resync_skills(workspace: &crate::workspace::WorkspaceContext) {
    if !auto_sync_enabled(workspace) {
        crate::logging::log("skills auto-sync skipped");
        return;
    }
    let theme = crate::theme::Theme::active();
    eprintln!("{}", format_resync_plan(theme));
    crate::logging::log("skills auto-sync start");
    if let Err(err) = install::sync_workspace(workspace, &real_sources(workspace)) {
        crate::logging::log(format!("skills auto-sync failed: {err:#}"));
    } else {
        crate::logging::log("skills auto-sync complete");
    }
}

/// Reconcile the selected workspace's mandatory compatibility revision before
/// a TUI launch. Current receipts and intact artifacts avoid duplicate writes.
pub fn sync_for_startup(workspace: &crate::workspace::WorkspaceContext) {
    resync_on_version_change(workspace);
}

/// Reconcile registered roots independently, retaining pending and failed roots
/// for later invocations instead of relying on a machine-wide done marker.
pub fn reconcile_registered_skills() {
    let Some(home) = std::env::var_os("HOME") else {
        crate::logging::log("skills compatibility refresh failed: HOME is not set");
        return;
    };
    let report = reconcile::all(
        std::path::Path::new(&home),
        &crate::workspace::RegistryStore::real(),
        reconcile::RefreshMode::Compatibility,
    );
    if let Some(error) = report.refresh_error {
        crate::logging::log(error);
    }
    for row in report.workspaces {
        crate::logging::log(format!(
            "skills compatibility {}: {:?}",
            row.name, row.outcome
        ));
    }
}

/// Mandatory bundled compatibility repair ignores the routine preference.
///
/// This selected startup seam remains best-effort; explicit repair reports
/// failures and the machine migration callback owns mandatory startup failure.
pub fn resync_on_version_change(workspace: &crate::workspace::WorkspaceContext) {
    let result = install_transaction::InstallIdentity::workspace(workspace).and_then(|identity| {
        reconcile::refresh(
            &identity,
            &real_sources(workspace),
            &mut receipt::WorkspaceStamp::selected(workspace),
            reconcile::RefreshMode::Compatibility,
        )
    });
    if let Err(error) = result {
        crate::logging::log(format!("skills compatibility refresh failed: {error}"));
    }
}

/// Narrate a version-triggered resync (to stderr, before any TUI takes the
/// screen). Themed for a dark terminal via [`crate::theme`].
#[must_use]
pub fn format_version_resync_plan(
    previous: Option<&str>,
    current: &str,
    theme: crate::theme::Theme,
) -> String {
    format!(
        "{}\n  {} {}\n  {} {}",
        theme.heading("Brain updated: refreshing installed skills"),
        theme.muted("version:"),
        theme.value(&format!("{} -> {current}", previous.unwrap_or("(none)"))),
        theme.muted("plan:"),
        "render bundled skills, apply extensions/plugins, prune removed skills, and refresh project links",
    )
}

#[must_use]
pub fn format_resync_plan(theme: crate::theme::Theme) -> String {
    format!(
        "{}\n  {} {}\n  {} {}",
        theme.heading("Refreshing installed brain skills"),
        theme.muted("reason:"),
        "config changed",
        theme.muted("plan:"),
        "render bundled skills, apply extensions/plugins, prune removed skills, and refresh project links",
    )
}

/// Extension + plugin sources from the brain config dir.
///
/// They live at `<brain-root>/.config/{extensions,plugins}` — inside the brain
/// root, alongside personalization and config, so they travel with the brain.
#[must_use]
pub fn real_sources(workspace: &crate::workspace::WorkspaceContext) -> install::Sources {
    let config_dir = crate::settings::config_dir(workspace);
    install::Sources {
        extensions_dir: Some(config_dir.join("extensions")),
        plugins_dir: Some(plugin::dir_in_config(&config_dir)),
    }
}

fn auto_sync_enabled(workspace: &crate::workspace::WorkspaceContext) -> bool {
    crate::settings::resolve_one(workspace, "skills_auto_sync").as_deref() == Some("true")
}

#[cfg(test)]
mod tests {
    #[test]
    fn resync_plan_names_the_workspace_skill_refresh() {
        let plan = super::format_resync_plan(crate::theme::Theme::dark(false));

        assert!(plan.contains("Refreshing installed brain skills"), "{plan}");
        assert!(plan.contains("reason: config changed"), "{plan}");
        assert!(plan.contains("plan: render bundled skills"), "{plan}");
    }

    #[test]
    fn needs_resync_when_no_version_has_been_stamped() {
        // Workspace skills never rendered by a version-aware binary must re-render.
        assert!(super::needs_resync(None, "0.18.0"));
    }

    #[test]
    fn needs_resync_only_when_the_stamp_differs_from_the_running_binary() {
        assert!(!super::needs_resync(Some("0.18.0"), "0.18.0"));
        assert!(super::needs_resync(Some("0.17.1"), "0.18.0"));
        // A downgrade also re-renders: the render must match the running binary.
        assert!(super::needs_resync(Some("0.19.0"), "0.18.0"));
    }

    #[test]
    fn version_resync_plan_names_the_update_and_render() {
        let plan = super::format_version_resync_plan(
            Some("0.17.1"),
            "0.18.0",
            crate::theme::Theme::dark(false),
        );
        assert!(plan.contains("Brain updated"), "{plan}");
        assert!(plan.contains("0.17.1"), "{plan}");
        assert!(plan.contains("0.18.0"), "{plan}");
        assert!(plan.contains("render bundled skills"), "{plan}");
    }

    #[test]
    fn version_resync_plan_handles_never_stamped_workspace_skills() {
        let plan =
            super::format_version_resync_plan(None, "0.18.0", crate::theme::Theme::dark(false));
        assert!(plan.contains("0.18.0"), "{plan}");
    }
}

#[cfg(test)]
mod transaction_tests;

#[cfg(test)]
mod transaction_recovery_tests;

#[cfg(test)]
mod transaction_legacy_tests;

#[cfg(test)]
mod install_safety_tests;

#[cfg(test)]
mod transaction_boundary_tests;

#[cfg(test)]
mod transaction_inspection_tests;

#[cfg(test)]
mod transaction_disposal_tests;

#[cfg(test)]
mod reconcile_tests;

#[cfg(test)]
mod compile_tests;

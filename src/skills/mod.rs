//! The brain skill pipeline.
//!
//! Renders the bundled (embedded) skills and the user's plugins, injecting each
//! skill's extension, and installs them into the shared agent registry
//! (`~/.agents/skills`), fanning out to each frontend (Claude, Codex, OpenCode,
//! Cursor).
//!
//! Sub-project A shipped `resync_skills()` as a no-op seam. B1 filled in the
//! render/install/fan-out pipeline and the `brain skills sync` command; B2 adds
//! extensions (inject into a built copy at named hooks) and plugins (whole user
//! skills). `resync_skills()` runs the pipeline but is **gated OFF by default**
//! (`skills_auto_sync`) so a `config`/`personalize` mutation never touches the
//! live registry while the pipeline is rolled out (B1–B3); the B4 cutover flips
//! the gate.

pub mod command;
pub mod embed;
pub mod extension;
pub mod install;
pub mod layout;
pub mod model;
pub mod plugin;
pub mod render;

use std::path::PathBuf;

/// The brain version currently running — the authority for a rendered
/// registry. When the installed skills were rendered by a different version,
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
/// re-renders, so the registry always matches the running binary. Pure.
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
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        crate::logging::log("skills auto-sync skipped: HOME is not set");
        return;
    };
    let theme = crate::theme::Theme::active();
    eprintln!("{}", format_resync_plan(theme));
    crate::logging::log("skills auto-sync start");
    if let Err(err) = install::sync(&layout::Layout::real(&home), &real_sources(workspace)) {
        crate::logging::log(format!("skills auto-sync failed: {err:#}"));
    } else {
        record_synced_version(workspace, current_version());
        crate::logging::log("skills auto-sync complete");
    }
}

/// Re-render + install the bundled skills once, the first time a *new* brain
/// binary runs against this workspace.
///
/// Deterministic and LLM-free: it compares the running version to the
/// per-workspace render stamp and, when they differ, runs the same pipeline
/// `brain skills sync` does, then re-stamps.
///
/// Called from bootstrap for every ready-workspace invocation (i.e. any command
/// that resolves a workspace — not `--help` / `--version`, not the internal
/// hook/server, not registry-only maintenance). Gated by `skills_auto_sync`
/// (default true) so a user can opt out and manage the registry only via
/// explicit `brain skills sync`. Never fails the invocation that triggered it.
pub fn resync_on_version_change(workspace: &crate::workspace::WorkspaceContext) {
    if !auto_sync_enabled(workspace) {
        return;
    }
    let current = current_version();
    let stamped = synced_version(workspace);
    if !needs_resync(stamped.as_deref(), current) {
        return;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        crate::logging::log("skills version-resync skipped: HOME is not set");
        return;
    };
    let theme = crate::theme::Theme::active();
    eprintln!(
        "{}",
        format_version_resync_plan(stamped.as_deref(), current, theme)
    );
    crate::logging::log(format!(
        "skills version-resync start: {} -> {current}",
        stamped.as_deref().unwrap_or("(none)")
    ));
    match install::sync(&layout::Layout::real(&home), &real_sources(workspace)) {
        Ok(report) => {
            record_synced_version(workspace, current);
            crate::logging::log(format!(
                "skills version-resync complete: {} skill(s)",
                report.installed.len()
            ));
        }
        // A failed resync must not block the command; the stamp is left
        // unchanged so the next invocation retries.
        Err(err) => crate::logging::log(format!("skills version-resync failed: {err:#}")),
    }
}

/// Read the per-workspace render stamp from the state DB (`None` on any error —
/// a missing DB simply means "never rendered", which triggers a resync).
fn synced_version(workspace: &crate::workspace::WorkspaceContext) -> Option<String> {
    crate::state::Db::open(workspace)
        .ok()?
        .skills_synced_version()
}

/// Persist the render stamp for `workspace`. Best-effort: a write failure is
/// logged, never fatal (worst case the next invocation re-renders).
pub(crate) fn record_synced_version(
    workspace: &crate::workspace::WorkspaceContext,
    version: &str,
) {
    match crate::state::Db::open(workspace) {
        Ok(db) => {
            if let Err(err) = db.set_skills_synced_version(version) {
                crate::logging::log(format!("skills render stamp write failed: {err:#}"));
            }
        }
        Err(err) => crate::logging::log(format!("skills render stamp skipped: {err:#}")),
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
        "render bundled skills, apply extensions/plugins, and refresh registry links",
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
        "render bundled skills, apply extensions/plugins, and refresh registry links",
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
    fn resync_plan_names_the_skill_registry_refresh() {
        let plan = super::format_resync_plan(crate::theme::Theme::dark(false));

        assert!(plan.contains("Refreshing installed brain skills"), "{plan}");
        assert!(plan.contains("reason: config changed"), "{plan}");
        assert!(plan.contains("plan: render bundled skills"), "{plan}");
    }

    #[test]
    fn needs_resync_when_no_version_has_been_stamped() {
        // A registry never rendered by a version-aware binary must re-render.
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
        let plan =
            super::format_version_resync_plan(Some("0.17.1"), "0.18.0", crate::theme::Theme::dark(false));
        assert!(plan.contains("Brain updated"), "{plan}");
        assert!(plan.contains("0.17.1"), "{plan}");
        assert!(plan.contains("0.18.0"), "{plan}");
        assert!(plan.contains("render bundled skills"), "{plan}");
    }

    #[test]
    fn version_resync_plan_handles_a_never_stamped_registry() {
        let plan = super::format_version_resync_plan(None, "0.18.0", crate::theme::Theme::dark(false));
        assert!(plan.contains("0.18.0"), "{plan}");
    }
}

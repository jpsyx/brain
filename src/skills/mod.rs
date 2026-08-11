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
//! skills). `resync_skills()` runs the pipeline but is **gated OFF by default**
//! (`skills_auto_sync`) so a `config`/`personalize` mutation can opt out of
//! workspace skill writes.

pub mod command;
pub mod embed;
pub mod extension;
pub mod install;
pub mod layout;
pub mod model;
pub mod plugin;
pub mod render;

pub use install::WorkspaceCapabilityReport;

/// Render a selected workspace/actor skill view without touching registries.
pub fn render_workspace_capabilities(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &crate::actor::ActorContext,
    plan: &crate::access::CapabilityPlan,
) -> anyhow::Result<WorkspaceCapabilityReport> {
    let layout = layout::Layout::workspace_capabilities(workspace, actor);
    crate::access::remove_capability_path(workspace, &layout.built_dir)?;
    crate::access::ensure_capability_directory(workspace, &layout.built_dir)?;
    install::render_workspace_capabilities(&layout, &real_sources(workspace), plan)
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
    if let Err(err) = install::sync(
        &layout::Layout::real(workspace.root()),
        &real_sources(workspace),
    ) {
        crate::logging::log(format!("skills auto-sync failed: {err:#}"));
    } else {
        record_synced_version(workspace, current_version());
        crate::logging::log("skills auto-sync complete");
    }
}

/// Synchronize workspace skills once before the interactive TUI can launch an
/// agent panel. This is deliberately best-effort so a malformed user skill
/// cannot prevent the shell from opening.
pub fn sync_for_startup(workspace: &crate::workspace::WorkspaceContext) {
    let layout = layout::Layout::real(workspace.root());
    let sources = real_sources(workspace);
    eprintln!(
        "{}",
        command::format_sync_plan(&layout, &sources, crate::theme::Theme::active())
    );
    crate::logging::log("skills startup sync start");
    match install::sync(&layout, &sources) {
        Ok(report) => {
            record_synced_version(workspace, current_version());
            crate::logging::log(format!(
                "skills startup sync complete: {} skill(s)",
                report.installed.len()
            ));
        }
        Err(error) => crate::logging::log(format!("skills startup sync failed: {error:#}")),
    }
}

/// Migrate the project-scoped core skills for every registered workspace.
///
/// Older Brain releases rendered these skills into machine-global locations.
/// The migration deliberately renders the embedded core set rather than copying
/// global symlinks, then lets the normal installer discover each workspace's
/// user-authored skills. `skip_root` is used by startup callers because the
/// selected workspace is synced immediately afterward (and receives its normal
/// per-workspace version stamp).
pub fn migrate_global_skills_for_all_workspaces(
    skip_root: Option<&std::path::Path>,
) {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        crate::logging::log("skills global migration skipped: HOME is not set");
        return;
    };
    let marker = home.join(".cache/brain/skills-migration-version");
    let stamped = std::fs::read_to_string(&marker).ok();
    if !needs_global_migration(stamped.as_deref().map(str::trim), current_version()) {
        return;
    }
    let store = crate::workspace::RegistryStore::real();
    let registry = match crate::workspace::registry::RegistryStore::load_readable(store.path()) {
        Ok(registry) => registry,
        Err(error) => {
            crate::logging::log(format!("skills global migration skipped: {error}"));
            return;
        }
    };
    let legacy_count = embed::bundled_skills()
        .iter()
        .filter(|skill| legacy_global_skill_exists(&home, &skill.name))
        .count();
    crate::logging::log(format!(
        "skills global migration start: {} legacy core skill(s), {} workspace(s)",
        legacy_count,
        registry.workspaces.len()
    ));

    let mut failed = false;
    for record in registry.workspaces.values() {
        if skip_root.is_some_and(|root| same_path(root, &record.root)) {
            continue;
        }
        let layout = layout::Layout::real(&record.root);
        let sources = install::Sources {
            extensions_dir: Some(record.root.join(".config/extensions")),
            plugins_dir: Some(record.root.join(".config/plugins")),
        };
        if let Err(error) = install::sync(&layout, &sources) {
            failed = true;
            crate::logging::log(format!(
                "skills global migration failed for {}: {error:#}",
                record.root.display()
            ));
        }
    }
    if failed {
        return;
    }
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&marker, current_version()) {
        crate::logging::log(format!("skills global migration marker failed: {error}"));
    } else {
        crate::logging::log("skills global migration complete");
    }
}

#[must_use]
pub fn needs_global_migration(stamped: Option<&str>, current: &str) -> bool {
    stamped != Some(current)
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    let canonical_left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let canonical_right = right
        .canonicalize()
        .unwrap_or_else(|_| right.to_path_buf());
    canonical_left == canonical_right
}

fn legacy_global_skill_exists(home: &std::path::Path, name: &str) -> bool {
    [
        home.join(".agents/skills"),
        home.join(".local/share/brain/skills"),
        home.join(".claude/skills"),
        home.join(".codex/skills"),
        home.join(".config/opencode/skills"),
    ]
    .iter()
    .map(|root| root.join(name).join("SKILL.md"))
    .any(|skill| skill.is_file())
}

/// Re-render + install the bundled skills once, the first time a *new* brain
/// binary runs against this workspace.
///
/// Deterministic and LLM-free: it compares the running version to the
/// per-workspace render stamp and, when they differ, runs the same pipeline
/// `brain skills sync` does, then re-stamps.
///
/// Called from bootstrap for every ready non-TUI invocation (i.e. any command
/// that resolves a workspace — not `--help` / `--version`, not the internal
/// hook/server, not skills-only maintenance). Gated by `skills_auto_sync`
/// (default true) so a user can opt out and manage workspace skills only via
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
    let theme = crate::theme::Theme::active();
    eprintln!(
        "{}",
        format_version_resync_plan(stamped.as_deref(), current, theme)
    );
    crate::logging::log(format!(
        "skills version-resync start: {} -> {current}",
        stamped.as_deref().unwrap_or("(none)")
    ));
    match install::sync(
        &layout::Layout::real(workspace.root()),
        &real_sources(workspace),
    ) {
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
pub(crate) fn record_synced_version(workspace: &crate::workspace::WorkspaceContext, version: &str) {
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
        "render bundled skills, apply extensions/plugins, and refresh project links",
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
        "render bundled skills, apply extensions/plugins, and refresh project links",
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

    #[test]
    fn global_migration_runs_only_when_the_version_marker_is_stale() {
        assert!(super::needs_global_migration(None, "0.58.0"));
        assert!(super::needs_global_migration(Some("0.57.0"), "0.58.0"));
        assert!(!super::needs_global_migration(Some("0.58.0"), "0.58.0"));
    }
}

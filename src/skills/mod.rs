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

/// Re-render and install the brain skills after a config/personalize mutation.
///
/// Gated by the `skills_auto_sync` config flag (default false); a disabled or
/// failed sync must never fail the mutation that triggered it.
pub fn resync_skills() {
    if !auto_sync_enabled() {
        return;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let _ = install::sync(&layout::Layout::real(&home), &real_sources());
}

/// Extension + plugin sources from the brain config dir.
///
/// They live at `<brain-root>/.config/{extensions,plugins}` — inside the brain
/// root, alongside personalization and config, so they travel with the brain.
#[must_use]
pub fn real_sources() -> install::Sources {
    let config_dir = crate::settings::config_dir();
    install::Sources {
        extensions_dir: Some(config_dir.join("extensions")),
        plugins_dir: Some(plugin::dir_in_config(&config_dir)),
    }
}

fn auto_sync_enabled() -> bool {
    crate::settings::resolve_one("skills_auto_sync").as_deref() == Some("true")
}

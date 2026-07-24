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

/// Extension + plugin sources from the configured brain root
/// (`<root>/.config/{extensions,plugins}`). Empty if the root can't be resolved.
#[must_use]
pub fn real_sources() -> install::Sources {
    crate::paths::brain_root().map_or_else(
        |_| install::Sources::default(),
        |root| install::Sources {
            extensions_dir: Some(root.join(".config").join("extensions")),
            plugins_dir: Some(plugin::dir_in_root(&root)),
        },
    )
}

fn auto_sync_enabled() -> bool {
    crate::settings::resolve_one("skills_auto_sync").as_deref() == Some("true")
}

//! The brain skill pipeline.
//!
//! Renders the bundled (embedded) skills against the user's
//! personalization/extensions and installs them into the shared agent registry,
//! fanning out to each frontend (Claude, Codex, OpenCode, Cursor).
//!
//! Sub-project A shipped `resync_skills()` as a no-op seam. B1 fills in the
//! pipeline (embed → render → install) and the `brain skills sync` command, but
//! keeps `resync_skills()` **gated OFF by default** (`skills_auto_sync`) so a
//! `config`/`personalize` mutation never touches the live registry while the
//! pipeline is still being rolled out (B1–B3). The B4 cutover flips the gate.

pub mod command;
pub mod embed;
pub mod install;
pub mod layout;
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
    let _ = install::sync(&layout::Layout::real(&home));
}

fn auto_sync_enabled() -> bool {
    crate::settings::resolve_one("skills_auto_sync").as_deref() == Some("true")
}

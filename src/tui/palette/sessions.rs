//! The per-session rows the catalog splices in after the static session block.
//!
//! These are *data* rows, not declared commands: one per skill session the
//! workspace configures and one per open brain-panel tab. The static session
//! commands (start, rename, close, new conversation, show main) live in the
//! catalog and are always listed.

use crate::skill_session::SkillSessionKey;
use crate::tui::action::GlobalAction;
use crate::tui::state::SessionPaletteEntry;

/// The runnable skill sessions followed by one Show row per open tab, in the
/// tab strip's own order so the palette and the strip agree.
pub(crate) fn session_rows(
    runnable: &[(SkillSessionKey, String)],
    open: &[SessionPaletteEntry],
) -> Vec<(String, GlobalAction)> {
    let mut rows: Vec<(String, GlobalAction)> = runnable
        .iter()
        .map(|(key, label)| (label.clone(), GlobalAction::RunSkillSession(*key)))
        .collect();
    rows.extend(open.iter().map(|entry| {
        (
            format!("Show {} session", entry.title),
            GlobalAction::ShowSessionTab(entry.id),
        )
    }));
    rows
}

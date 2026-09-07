use crate::skill_session::SkillSessionKey;
use crate::tui::action::GlobalAction;
use crate::tui::state::SessionPaletteEntry;

/// The common brain-session group inserted after Message brain in both catalogs.
pub(crate) fn session_actions(
    runnable: &[(SkillSessionKey, String)],
    open: &[SessionPaletteEntry],
) -> Vec<(String, GlobalAction)> {
    let mut actions = vec![
        (
            "Start new brain session".to_owned(),
            GlobalAction::StartManualSession,
        ),
        ("Rename session".to_owned(), GlobalAction::RenameSession),
    ];
    actions.extend(
        runnable
            .iter()
            .map(|(key, label)| (label.clone(), GlobalAction::RunSkillSession(*key))),
    );
    if !open.is_empty() {
        actions.push((
            "Show main brain session".to_owned(),
            GlobalAction::ShowMainBrainSession,
        ));
    }
    for entry in open {
        actions.push((
            format!("Show {} session", entry.title),
            GlobalAction::ShowSessionTab(entry.id),
        ));
        actions.push((
            format!("Close {} session", entry.title),
            GlobalAction::CloseSessionTab(entry.id),
        ));
    }
    actions
}

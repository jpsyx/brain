use super::{RemovalScreen, worker::Request};
use crate::{
    tasks::removal::model::{Dependents, LinkedIssue},
    tui::{
        App,
        overlay::{Overlay, close_overlay},
    },
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub(crate) fn handle_key(app: &mut App, key: &KeyEvent) {
    if key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        close_overlay(&mut app.overlay);
        return;
    }
    let Some(Overlay::TaskRemoval(state)) = &app.overlay else {
        return;
    };
    let mut state = state.clone();
    if matches!(
        key.code,
        KeyCode::Down | KeyCode::PageDown | KeyCode::Up | KeyCode::PageUp
    ) {
        state.scroll = if matches!(key.code, KeyCode::Down | KeyCode::PageDown) {
            state.scroll.saturating_add(1)
        } else {
            state.scroll.saturating_sub(1)
        };
        app.overlay = Some(Overlay::TaskRemoval(state));
        return;
    }
    if state.running() {
        return;
    }
    if state.abandon_confirm {
        if key.code == KeyCode::Char('y') {
            if let Some(id) = state.operation_id.clone() {
                app.removal_request(state, Request::Abandon(id));
            }
        } else {
            state.abandon_confirm = false;
            app.overlay = Some(Overlay::TaskRemoval(state));
        }
        return;
    }
    if state.recovery_available() {
        let id = state.operation_id.clone().expect("recovery operation");
        match key.code {
            KeyCode::Char('s') => app.removal_request(state, Request::Status(id)),
            KeyCode::Char('r') => app.removal_request(state, Request::Resume(id)),
            KeyCode::Char('a') => {
                state.abandon_confirm = true;
                app.overlay = Some(Overlay::TaskRemoval(state));
            }
            _ => {}
        }
        return;
    }
    if !state.preview_actions_available() {
        return;
    }
    match (&state.screen, key.code) {
        (RemovalScreen::Preview(plan), KeyCode::Enter | KeyCode::Char('y'))
            if plan.blockers.is_empty() =>
        {
            let request = Request::Apply {
                plan: Box::new(plan.clone()),
                expected: plan.fingerprint.clone(),
                today: chrono::Local::now().date_naive(),
            };
            app.removal_request(state, request);
        }
        (RemovalScreen::Preview(_), KeyCode::Char('d')) => {
            state.options.dependents = match state.options.dependents {
                Dependents::Reject => Dependents::Unlink,
                Dependents::Unlink => Dependents::Rewire,
                Dependents::Rewire => Dependents::Reject,
            };
            app.removal_preview(state);
        }
        (RemovalScreen::Preview(_), KeyCode::Char('i')) => {
            state.options.linked_issue = match state.options.linked_issue {
                LinkedIssue::Configured => LinkedIssue::Keep,
                LinkedIssue::Keep => LinkedIssue::Cancel,
                LinkedIssue::Cancel => LinkedIssue::Configured,
            };
            app.removal_preview(state);
        }
        (RemovalScreen::Failed(_), KeyCode::Char('p')) => app.removal_preview(state),
        _ => {}
    }
}

use super::{
    RemovalScreen,
    actions::{Action, choices},
    worker::Request,
};
use crate::tui::{
    App,
    overlay::{Overlay, close_overlay},
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
    let rows = choices(&state);
    match key.code {
        KeyCode::PageDown => state.scroll = state.scroll.saturating_add(5),
        KeyCode::PageUp => state.scroll = state.scroll.saturating_sub(5),
        KeyCode::Down | KeyCode::Right | KeyCode::Tab => {
            state.selected = (state.selected + 1) % rows.len();
        }
        KeyCode::Up | KeyCode::Left | KeyCode::BackTab => {
            state.selected = (state.selected + rows.len() - 1) % rows.len();
        }
        KeyCode::Enter => {
            let action = rows[state.selected.min(rows.len() - 1)].0;
            execute(app, state, action);
            return;
        }
        _ => return,
    }
    app.overlay = Some(Overlay::TaskRemoval(state));
}

fn execute(app: &mut App, mut state: super::RemovalState, action: Action) {
    match action {
        Action::Close => {
            close_overlay(&mut app.overlay);
        }
        Action::Delete => {
            if let RemovalScreen::Preview(plan) = &state.screen {
                let request = Request::Apply {
                    plan: Box::new(plan.clone()),
                    expected: plan.fingerprint.clone(),
                    today: chrono::Local::now().date_naive(),
                };
                app.removal_request(state, request);
            }
        }
        Action::Dependencies(value) => {
            state.options.dependents = value;
            app.removal_preview(state);
        }
        Action::Issue(value) => {
            state.options.linked_issue = value;
            app.removal_preview(state);
        }
        Action::Preview => app.removal_preview(state),
        Action::Abandon | Action::Back => {
            state.abandon_confirm = action == Action::Abandon;
            state.selected = 0;
            app.overlay = Some(Overlay::TaskRemoval(state));
        }
        Action::Status | Action::Resume | Action::ConfirmAbandon => {
            if let Some(id) = state.operation_id.clone() {
                let request = match action {
                    Action::Status => Request::Status(id),
                    Action::Resume => Request::Resume(id),
                    _ => Request::Abandon(id),
                };
                app.removal_request(state, request);
            }
        }
    }
}

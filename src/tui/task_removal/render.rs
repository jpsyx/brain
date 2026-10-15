use super::{RemovalScreen, RemovalState};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
pub(crate) fn draw(frame: &mut Frame, state: &RemovalState, area: Rect) {
    let modal =
        crate::tui::draw::layout::centered_rect(area.width.min(90), area.height.min(26), area);
    frame.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(crate::tui::modal_state::ConfirmIntent::Danger.accent()))
        .title(" Remove task ");
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    frame.render_widget(
        Paragraph::new(lines(state).join("\n"))
            .wrap(Wrap { trim: false })
            .scroll((state.scroll, 0)),
        inner,
    );
}
fn lines(state: &RemovalState) -> Vec<String> {
    let mut lines = vec![state.notice.clone()];
    if let Some(id) = &state.operation_id {
        lines.push(format!("Operation: {id}"));
    }
    match &state.screen {
        RemovalScreen::Preview(plan) => {
            lines.push(format!("Consent: {}", plan.fingerprint));
            lines.extend(
                plan.targets
                    .iter()
                    .map(|target| format!("Remove {}: {}", target.task_id, target.task_name)),
            );
            lines.push(format!(
                "Dependents: {:?}   Linked issue: {:?}",
                plan.options.dependents, plan.options.linked_issue
            ));
            lines.extend(
                plan.edits
                    .iter()
                    .map(|edit| format!("Update {}", edit.relative.display())),
            );
            lines.extend(super::effects::relationships(plan));
            lines.extend(
                plan.external
                    .iter()
                    .map(|effect| format!("Cancel {} issue {}", effect.adapter, effect.issue)),
            );
            lines.extend(
                plan.blockers
                    .iter()
                    .map(|p| format!("Blocked ({}): {}", p.code, p.message)),
            );
            lines.extend(
                plan.warnings
                    .iter()
                    .map(|p| format!("Warning ({}): {}", p.code, p.message)),
            );
            if state.preview_actions_available() {
                lines.push("[d] dependency policy: Reject / Keep prerequisites (rewire) / Remove dependency links (unlink)".into());
                lines.push("[i] issue policy  [Esc] cancel".into());
                if plan.blockers.is_empty() {
                    lines.push("[Enter] confirm".into());
                }
            }
        }
        RemovalScreen::Running { operation_id } => {
            lines.push(format!(
                "Working{}...",
                operation_id
                    .as_ref()
                    .map_or(String::new(), |id| format!(" on {id}"))
            ));
            lines.push("Closing this modal does not cancel or roll back this operation.".into());
        }
        RemovalScreen::Failed(problem) => {
            lines.push(format!("{}: {}", problem.code, problem.message));
            if state.preview_actions_available() {
                lines.push("[p] preview again".into());
            }
        }
        RemovalScreen::Finished(receipt) => {
            lines.push(format!(
                "Operation {}: {}",
                receipt.operation_id,
                if receipt.committed {
                    "committed"
                } else {
                    "not committed"
                }
            ));
            lines.extend(
                receipt
                    .removed
                    .iter()
                    .map(|target| format!("Removed {}: {}", target.task_id, target.task_name)),
            );
            lines.extend(
                receipt
                    .warnings
                    .iter()
                    .map(|p| format!("Warning ({}): {}", p.code, p.message)),
            );
        }
    }
    if state.recovery_available() {
        lines.push("[s] saved status  [r] resume  [a] abandon".into());
    }
    if state.abandon_confirm {
        lines.push("Abandon keeps local rows and does not undo external cancellations. Unknown external outcomes must first be reconciled. [y] confirm abandon; any other key cancels".into());
    }
    lines.push("[Esc] close  [Up/Down] scroll".into());
    lines
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;

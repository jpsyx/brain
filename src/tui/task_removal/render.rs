use super::{RemovalScreen, RemovalState, actions::choices};
use crate::render::{SELECTED_BG, TEXT_DIM, TEXT_PRIMARY};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
};

pub(crate) fn draw(frame: &mut Frame, state: &mut RemovalState, area: Rect) {
    let width = area.width.min(78);
    let text_width = usize::from(width.saturating_sub(6)).max(1);
    let body: Vec<_> = lines(state)
        .into_iter()
        .flat_map(|line| crate::tui::draw_modals::wrap_input(&line, text_width))
        .collect();
    let rows = choices(state);
    let items: Vec<_> = rows
        .iter()
        .map(|(_, label)| {
            ListItem::new(
                crate::tui::draw_modals::wrap_input(label, text_width.saturating_sub(2).max(1))
                    .into_iter()
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let menu_height: usize = items.iter().map(ListItem::height).sum();
    let footer_height = if width < 60 { 2 } else { 1 };
    let height = u16::try_from(body.len() + menu_height + 6 + usize::from(footer_height))
        .unwrap_or(u16::MAX)
        .min(area.height)
        .min(30);
    let modal = crate::tui::draw::layout::centered_rect(width, height, area);
    let accent = crate::tui::modal_state::ConfirmIntent::Danger.accent();
    frame.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Line::styled(
            " Delete task ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(modal).inner(Margin {
        horizontal: 2,
        vertical: 0,
    });
    frame.render_widget(block, modal);
    let menu_space = u16::try_from(menu_height)
        .unwrap_or(u16::MAX)
        .min(inner.height.saturating_sub(5 + footer_height).max(1));
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(menu_space),
        Constraint::Length(1),
        Constraint::Length(footer_height),
    ])
    .split(inner);
    let max_scroll = u16::try_from(body.len())
        .unwrap_or(u16::MAX)
        .saturating_sub(chunks[1].height);
    state.scroll = state.scroll.min(max_scroll);
    frame.render_widget(
        Paragraph::new(body.join("\n"))
            .style(Style::default().fg(TEXT_PRIMARY))
            .scroll((state.scroll.min(max_scroll), 0)),
        chunks[1],
    );
    let mut selection =
        ListState::default().with_selected(Some(state.selected.min(rows.len() - 1)));
    frame.render_stateful_widget(
        List::new(items)
            .style(Style::default().fg(TEXT_DIM))
            .highlight_style(
                Style::default()
                    .fg(SELECTED_BG)
                    .bg(accent)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› "),
        chunks[3],
        &mut selection,
    );
    let footer = if width < 60 && max_scroll > 0 {
        "↑↓ choose  Enter select\nEsc close  PgUp/PgDn details"
    } else if width < 60 {
        "↑↓ choose  Enter select\nEsc close"
    } else if max_scroll > 0 {
        "↑↓ choose  Enter select  Esc close  PgUp/PgDn details"
    } else {
        "↑↓ choose   Enter select   Esc close"
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(TEXT_DIM)),
        chunks[5],
    );
}

fn lines(state: &RemovalState) -> Vec<String> {
    let mut lines = Vec::new();
    if !state.notice.is_empty() {
        lines.push(state.notice.clone());
        lines.push(String::new());
    }
    if state.abandon_confirm {
        lines.push("Stop trying to delete this task?".into());
        lines.push(String::new());
        lines.push(
            "The task stays in Brain. Any linked issues already canceled stay canceled.".into(),
        );
        lines.push("If an issue's status is uncertain, check progress before stopping.".into());
        return lines;
    }
    match &state.screen {
        RemovalScreen::Preview(plan) => {
            lines.push("Delete this task?".into());
            lines.extend(
                plan.targets
                    .iter()
                    .map(|target| format!("{}: {}", target.task_id, target.task_name)),
            );
            lines.push(String::new());
            lines.push("This removes the task from Brain. There is no undo.".into());
            let effects = super::effects::relationships(plan);
            if !effects.is_empty() {
                lines.push(String::new());
                lines.push("Other tasks and projects affected".into());
                lines.extend(effects);
            }
            if !plan.external.is_empty() {
                lines.push(String::new());
                lines.extend(
                    plan.external.iter().map(|effect| {
                        format!("Linked issue {} will also be canceled.", effect.issue)
                    }),
                );
            }
            if !plan.blockers.is_empty() {
                lines.push(String::new());
                lines.push("Before you can delete".into());
                for problem in &plan.blockers {
                    if problem.code == "dependents_exist" {
                        lines.push(
                            problem
                                .message
                                .replace("depends on removed", "is waiting for"),
                        );
                        lines.push("Choose what the waiting tasks should do below. They will not be deleted.".into());
                    } else {
                        lines.push(problem.message.clone());
                    }
                }
            }
            lines.extend(plan.warnings.iter().map(|p| format!("Note: {}", p.message)));
        }
        RemovalScreen::Running { operation_id } => {
            lines.push(
                if operation_id.is_some() {
                    "Checking saved deletion..."
                } else {
                    "Checking deletion details..."
                }
                .into(),
            );
            lines.push(String::new());
            lines.push(
                if operation_id.is_some() {
                    "You can close this window. Checking saved progress will continue."
                } else {
                    "Nothing has been deleted. You can cancel this check."
                }
                .into(),
            );
        }
        RemovalScreen::Failed(problem) => {
            lines.push("Deletion needs attention".into());
            lines.push(String::new());
            lines.push(problem.message.clone());
        }
        RemovalScreen::Finished(receipt) => {
            lines.push(
                if receipt.committed {
                    "Task deleted"
                } else {
                    "Task was not deleted"
                }
                .into(),
            );
            lines.extend(
                receipt
                    .removed
                    .iter()
                    .map(|target| format!("{}: {}", target.task_id, target.task_name)),
            );
            lines.extend(
                receipt
                    .warnings
                    .iter()
                    .map(|p| format!("Note: {}", p.message)),
            );
        }
    }
    lines
}

#[cfg(test)]
#[path = "render_tests.rs"]
pub(super) mod tests;

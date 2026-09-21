//! Running a [`TaskCommand`] against one resolved task or habit.

use crate::main_view::MainView;
use crate::tui::App;
use crate::tui::modal_state::{BrainInputState, ConfirmState, FlashKind};
use crate::tui::overlay::{Overlay, open_overlay};
use crate::tui::palette::{TaskChoice, TaskCommand};

use super::commands::{reassign_task_prompt, start_task_prompt};

impl App {
    pub(crate) fn run_task_command(&mut self, command: TaskCommand, target: &TaskChoice) {
        let id = target.id.as_str();
        match command {
            TaskCommand::Start => {
                // Asks the brain agent to (1) gather the task's context
                // (notes / project / see_also / blockers) before proposing
                // anything, (2) give a short list of concrete first steps, and
                // (3) explicitly call out where it can help right now, so the
                // next reply is actionable rather than just advisory.
                let message = start_task_prompt(id, self.context.workspace_root());
                self.send_brain_prompt(&message);
            }
            TaskCommand::MarkComplete => {
                // A confirmation rather than an immediate write: this mutates
                // tasks.csv. The Yes path calls `run_mark_complete`.
                open_overlay(
                    &mut self.overlay,
                    Overlay::TaskConfirmation(ConfirmState::mark_complete(
                        target.id.clone(),
                        target.label.clone(),
                    )),
                );
            }
            TaskCommand::MessageBrainAbout => {
                open_overlay(
                    &mut self.overlay,
                    Overlay::BrainInput(BrainInputState::about(
                        target.id.clone(),
                        target.label.clone(),
                    )),
                );
            }
            TaskCommand::ToggleNotes => self.toggle_notes_for(target),
            TaskCommand::OpenLinks => {
                let plan = self
                    .tasks
                    .links_plan_for(id, &self.context.linear_base_url());
                self.apply_links_plan(plan, Some(id));
            }
            TaskCommand::Remove => {
                // Destructive enough to warrant the extra keystroke; the Yes
                // path calls `run_remove`.
                open_overlay(
                    &mut self.overlay,
                    Overlay::TaskConfirmation(ConfirmState::remove(
                        target.id.clone(),
                        target.label.clone(),
                    )),
                );
            }
            TaskCommand::Reassign => {
                let message = reassign_task_prompt(id);
                self.send_brain_prompt(&message);
            }
            TaskCommand::Defer(days) => {
                // Hand off to the brain agent (which has the /todo skill
                // loaded) rather than calling defer_task.py directly — keeps
                // the user in the loop in case the defer has chunked-task
                // cascade implications worth a glance.
                let day_word = if days == 1 { "day" } else { "days" };
                let message = format!("Defer task {id} by {days} {day_word}");
                self.send_brain_prompt(&message);
            }
        }
    }

    /// Flip one entry's notes. Run from another view the toggle still lands —
    /// expansion is keyed by task ID — so the tasks view comes forward to show
    /// it, and an entry with nothing to expand says so rather than doing
    /// nothing visible.
    fn toggle_notes_for(&mut self, target: &TaskChoice) {
        if self.tasks.toggle_notes_for(&target.id) {
            self.shell.show_main_view(MainView::Tasks);
            self.shell.focus_tasks();
        } else {
            self.status.set_flash(FlashKind::Info(format!(
                "{} has no notes to expand",
                target.id
            )));
        }
    }
}

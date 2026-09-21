//! Resolving a command's target, and asking for it when the context has none.
//!
//! A target counts as "in context" only when the view that owns it is the one
//! showing: a task selection sitting behind the brain-directory view is not
//! something the user is pointing at, so a task command run from there asks
//! which task rather than acting on a stale highlight.

use std::path::PathBuf;

use crate::main_view::MainView;
use crate::tui::App;
use crate::tui::modal_state::FlashKind;
use crate::tui::overlay::{Overlay, open_overlay};
use crate::tui::palette::{
    EntryCommand, EntryTargetPicker, PaletteContext, TaskChoice, TaskCommand, TaskTargetPicker,
};

impl App {
    /// Everything the palette reads when it opens.
    pub(crate) fn palette_context(&mut self) -> PaletteContext {
        self.refresh_receiver_enabled();
        PaletteContext {
            task: self.task_context(),
            entry: self.entry_context(),
            receiver_enabled: self.receiver.is_enabled(),
            daily_triage_alert_disabled: self.status.daily_triage_check_disabled(),
            panel_side: self.shell.panel_side(),
            assignment_mode: self.tasks.assignment_snapshot().mode,
            runnable_skill_sessions: self.runnable_skill_session_rows(),
            user_sessions: self.brain.user_session_rows(),
        }
    }

    /// The highlighted task, but only while the tasks view is showing.
    fn task_context(&self) -> Option<crate::tui::palette::TaskContext> {
        if self.shell.main_view() != MainView::Tasks {
            return None;
        }
        self.tasks
            .selected_task_context(&self.context.linear_base_url())
    }

    /// The highlighted brain-directory entry, but only while that view is
    /// showing.
    fn entry_context(&self) -> Option<crate::tui::palette::EntryContext> {
        if self.shell.main_view() != MainView::BrainSearch {
            return None;
        }
        self.shell.selected_entry_context()
    }

    pub(super) fn resolve_task_target(&self, command: TaskCommand) -> Option<TaskChoice> {
        if self.shell.main_view() != MainView::Tasks {
            return None;
        }
        let (id, label) = self.tasks.selected_identity()?;
        let is_habit = self.tasks.current_is_habit();
        (command.works_on_habits() || !is_habit).then_some(TaskChoice {
            id,
            label,
            is_habit,
        })
    }

    pub(super) fn resolve_entry_target(&self, command: EntryCommand) -> Option<PathBuf> {
        if self.shell.main_view() != MainView::BrainSearch {
            return None;
        }
        let entry = self.shell.selected_entry_context()?;
        entry
            .satisfies(command.requirement())
            .then(|| self.shell.selected_search_path())
            .flatten()
    }

    pub(super) fn open_task_target_picker(&mut self, command: TaskCommand) {
        let choices = self.tasks.target_choices(command.works_on_habits());
        let picker = TaskTargetPicker::new(command, choices);
        if picker.is_empty() {
            self.status.set_flash(FlashKind::Info(
                "no tasks to run that command on".to_owned(),
            ));
            return;
        }
        open_overlay(&mut self.overlay, Overlay::TaskTargetPicker(picker));
    }

    pub(super) fn open_entry_target_picker(&mut self, command: EntryCommand) {
        let root = self.context.workspace_root();
        let entries = crate::entry::collect(root, &crate::tui::search_view::all_bucket_roots(root))
            .unwrap_or_default();
        if entries.is_empty() {
            self.status.set_flash(FlashKind::Info(
                "the brain directory has nothing to act on".to_owned(),
            ));
            return;
        }
        open_overlay(
            &mut self.overlay,
            Overlay::EntryTargetPicker(EntryTargetPicker::new(
                command,
                crate::picker::App::new(&entries, ""),
            )),
        );
    }
}

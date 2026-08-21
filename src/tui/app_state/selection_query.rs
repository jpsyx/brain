//! Queries about the currently-selected entry (id, habit-ness, notes, link
//! kind) and the notes expand/collapse toggles that drive the `l` / arrow keys
//! and the contextual palette rows.

use crate::tui::*;

impl App {
    pub(crate) fn current_task_id(&self) -> Option<String> {
        self.selected_task
            .and_then(|i| self.visible_tasks.get(i))
            .map(|t| t.id.clone())
    }

    pub(crate) fn current_is_habit(&self) -> bool {
        self.selected_task
            .and_then(|i| self.visible_tasks.get(i))
            .is_some_and(Task::is_habit)
    }

    /// Whether the selected entry has any non-blank notes. Drives the
    /// "Expand notes" palette command's visibility and whether `l` does
    /// anything.
    pub(crate) fn current_has_notes(&self) -> bool {
        self.selected_task
            .and_then(|i| self.visible_tasks.get(i))
            .is_some_and(|t| !t.notes.trim().is_empty())
    }

    /// The selected entry's link situation (Linear issue and/or notes URLs),
    /// driving the "open link" palette command's visibility and label.
    pub(crate) fn current_link_kind(&self) -> LinkKind {
        let Some(task) = self.selected_task.and_then(|i| self.visible_tasks.get(i)) else {
            return LinkKind::None;
        };
        let links = task_links(task, &self.config.linear_base_url());
        classify_links(task, &links)
    }

    /// Whether the selected entry's notes are currently rendered expanded
    /// (either globally via `full_notes` or via the per-task toggle).
    pub(crate) fn current_notes_expanded(&self) -> bool {
        self.full_notes
            || self
                .current_task_id()
                .is_some_and(|id| self.expanded_notes.contains(&id))
    }

    /// Toggle expanded notes for the selected entry. No-op when nothing is
    /// selected or the entry has no notes (the action isn't offered in that
    /// case, but guard here too so the bare `l` key stays inert).
    pub(crate) fn toggle_notes(&mut self) {
        if !self.current_has_notes() {
            return;
        }
        let Some(id) = self.current_task_id() else {
            return;
        };
        if !self.expanded_notes.remove(&id) {
            self.expanded_notes.insert(id);
        }
        self.rebuild_body();
    }

    /// Expand the selected entry's notes — the `→` arrow alias. No-op
    /// when the entry has no notes or they're already expanded.
    pub(crate) fn expand_notes(&mut self) {
        if self.current_has_notes() && !self.current_notes_expanded() {
            self.toggle_notes();
        }
    }

    /// Collapse the selected entry's notes — the `←` arrow alias. No-op
    /// when the entry has no notes or they're already collapsed.
    pub(crate) fn collapse_notes(&mut self) {
        if self.current_has_notes() && self.current_notes_expanded() {
            self.toggle_notes();
        }
    }
}

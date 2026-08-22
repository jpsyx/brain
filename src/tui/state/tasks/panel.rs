use ratatui::text::Line;

use super::TasksState;
use crate::tasks::task::AssignmentUser;
use crate::users::UserId;

pub(crate) struct TasksPanelModel<'a> {
    state: &'a TasksState,
}

impl TasksState {
    pub(crate) const fn panel_model(&self) -> TasksPanelModel<'_> {
        TasksPanelModel { state: self }
    }
}

impl<'a> TasksPanelModel<'a> {
    pub(crate) fn title(&self) -> impl ExactSizeIterator<Item = &'a Line<'static>> {
        self.state.header.iter()
    }

    pub(crate) fn assignment_users(&self) -> impl Iterator<Item = &'a AssignmentUser> {
        self.state.assignment.users().iter()
    }

    pub(crate) fn assignment_filter(&self) -> Option<&'a UserId> {
        self.state.assignment_filter.as_ref()
    }

    pub(crate) fn shows_search(&self) -> bool {
        self.state.in_search || !self.state.query.is_empty()
    }

    pub(crate) const fn is_searching(&self) -> bool {
        self.state.in_search
    }

    pub(crate) fn query(&self) -> &'a str {
        &self.state.query
    }

    pub(crate) fn visible_count(&self) -> usize {
        self.state.visible_tasks.len()
    }

    pub(crate) fn unfiltered_count(&self) -> usize {
        self.state.base_tasks.len()
    }

    pub(crate) fn content(&self) -> impl ExactSizeIterator<Item = &'a Line<'static>> {
        self.state.body_lines.iter()
    }

    pub(crate) const fn scroll(&self) -> u16 {
        self.state.scroll
    }

    pub(crate) const fn max_scroll(&self) -> u16 {
        self.state.max_scroll()
    }

    pub(crate) const fn pending_count(&self) -> Option<usize> {
        self.state.pending_count
    }
}

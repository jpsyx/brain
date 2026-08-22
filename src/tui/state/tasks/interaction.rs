use std::ops::Range;

use ratatui::layout::Rect;

use super::TasksState;
use crate::tasks::task::Task;
use crate::users::UserId;

impl TasksState {
    #[cfg(test)]
    pub(crate) const fn scroll_offset(&self) -> u16 {
        self.scroll
    }

    pub(crate) const fn max_scroll(&self) -> u16 {
        self.last_content_rows
            .saturating_sub(self.last_inner_height)
    }

    pub(crate) fn append_query(&mut self, character: char) {
        self.query.push(character);
        self.rebuild_body();
    }

    pub(crate) fn clear_query(&mut self) {
        if !self.query.is_empty() {
            self.query.clear();
            self.rebuild_body();
        }
    }

    pub(crate) fn pop_query(&mut self) {
        if self.query.pop().is_some() {
            self.rebuild_body();
        }
    }

    pub(crate) fn clear_active_filters(&mut self) {
        let had_filter = self.has_active_filter();
        self.query.clear();
        self.assignment_filter = None;
        if had_filter {
            self.rebuild_body();
        }
    }

    pub(crate) fn set_assignment_filter(&mut self, user_id: Option<UserId>) {
        self.assignment_filter = user_id;
        self.rebuild_body();
    }

    pub(crate) fn current_notes_expanded(&self) -> bool {
        self.full_notes
            || self
                .selected_task()
                .is_some_and(|task| self.expanded_notes.contains(&task.id))
    }

    pub(crate) fn current_has_notes(&self) -> bool {
        self.selected_task()
            .is_some_and(|task| !task.notes.trim().is_empty())
    }

    pub(crate) fn current_is_habit(&self) -> bool {
        self.selected_task().is_some_and(Task::is_habit)
    }

    pub(crate) fn current_task_id(&self) -> Option<String> {
        self.selected_task().map(|task| task.id.clone())
    }

    pub(crate) fn toggle_notes(&mut self) {
        let Some(task) = self.selected_task() else {
            return;
        };
        if task.notes.trim().is_empty() {
            return;
        }
        let id = task.id.clone();
        if !self.expanded_notes.remove(&id) {
            self.expanded_notes.insert(id);
        }
        self.rebuild_body();
    }

    pub(crate) fn expand_notes(&mut self) {
        if self.current_has_notes() && !self.current_notes_expanded() {
            self.toggle_notes();
        }
    }

    pub(crate) fn collapse_notes(&mut self) {
        if self.current_has_notes() && self.current_notes_expanded() {
            self.toggle_notes();
        }
    }

    pub(crate) fn select_next(&mut self, amount: usize) {
        let Some(selected) = self.selected_task else {
            return;
        };
        if self.visible_tasks.is_empty() {
            return;
        }
        self.set_selected(
            selected
                .saturating_add(amount)
                .min(self.visible_tasks.len() - 1),
        );
    }

    pub(crate) fn select_prev(&mut self, amount: usize) {
        let Some(selected) = self.selected_task else {
            return;
        };
        self.set_selected(selected.saturating_sub(amount));
    }

    pub(crate) fn select_first(&mut self) {
        self.set_selected(0);
    }

    pub(crate) fn select_last(&mut self) {
        if !self.visible_tasks.is_empty() {
            self.set_selected(self.visible_tasks.len() - 1);
        }
    }

    pub(crate) fn tasks_per_page(&self) -> usize {
        (usize::from(self.last_inner_height.max(1)) / 4).max(1)
    }

    pub(crate) fn push_count_digit(&mut self, digit: u32) -> bool {
        let Some(count) = crate::tui::keymap::accumulate_count(self.pending_count, digit) else {
            return false;
        };
        self.pending_count = Some(count);
        true
    }

    pub(crate) fn take_count(&mut self) -> usize {
        self.pending_count.take().unwrap_or(1)
    }

    pub(crate) fn clear_count(&mut self) {
        self.pending_count = None;
    }

    pub(crate) fn update_body_layout(&mut self, inner_height: u16, heights: &[u16]) {
        self.last_inner_height = inner_height;
        self.visual_row_offsets = visual_row_offsets(heights);
        self.last_content_rows = self.visual_row_offsets.last().copied().unwrap_or(0);
        self.ensure_selected_visible();
        self.clamp_scroll();
    }

    pub(crate) fn selection_band_rect(&self, content_area: Rect) -> Option<Rect> {
        let selected = self.selected_task?;
        let range = self.task_line_ranges.get(selected)?;
        let visible = visual_range(&self.visual_row_offsets, range.clone());
        let bottom = self.scroll.saturating_add(content_area.height);
        if visible.end <= self.scroll || visible.start >= bottom {
            return None;
        }
        let visible_start = visible.start.max(self.scroll);
        let visible_end = visible.end.min(bottom);
        let height = visible_end.saturating_sub(visible_start);
        (height > 0).then_some(Rect {
            x: content_area.x,
            y: content_area.y + (visible_start - self.scroll),
            width: content_area.width,
            height,
        })
    }

    fn set_selected(&mut self, index: usize) {
        if self.visible_tasks.is_empty() {
            self.selected_task = None;
            return;
        }
        let selected = index.min(self.visible_tasks.len() - 1);
        if Some(selected) != self.selected_task {
            self.selected_task = Some(selected);
            self.ensure_selected_visible();
        }
    }

    pub(super) fn ensure_selected_visible(&mut self) {
        let Some(selected) = self.selected_task else {
            return;
        };
        let Some(range) = self.task_line_ranges.get(selected) else {
            return;
        };
        let visible = visual_range(&self.visual_row_offsets, range.clone());
        let inner_height = self.last_inner_height.max(1);
        if visible.start < self.scroll {
            self.scroll = visible.start;
        } else if visible.end > self.scroll.saturating_add(inner_height) {
            self.scroll = visible.end.saturating_sub(inner_height);
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }
}

fn visual_row_offsets(heights: &[u16]) -> Vec<u16> {
    let mut total = 0_u16;
    let mut offsets = Vec::with_capacity(heights.len() + 1);
    offsets.push(0);
    for height in heights {
        total = total.saturating_add(*height);
        offsets.push(total);
    }
    offsets
}

fn visual_range(offsets: &[u16], logical: Range<usize>) -> Range<u16> {
    if offsets.is_empty() {
        return 0..0;
    }
    let last = offsets.len() - 1;
    offsets[logical.start.min(last)]..offsets[logical.end.min(last)]
}

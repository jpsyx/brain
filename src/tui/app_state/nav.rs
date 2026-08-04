//! Query/filter, the body rebuild, scroll bookkeeping, and cursor movement:
//! everything that decides which tasks are visible and where the selection
//! and viewport sit.

use crate::tasks::render::{build_body_lines_with_ranges, no_matches_lines};
use crate::tui::*;

impl App<'_> {
    pub(crate) fn has_active_filter(&self) -> bool {
        !self.query.is_empty() || self.assignment_filter.is_some()
    }

    pub(crate) fn show_search_bar(&self) -> bool {
        self.in_search || !self.query.is_empty()
    }

    pub(crate) const fn max_scroll(&self) -> u16 {
        self.last_content_rows
            .saturating_sub(self.last_inner_height)
    }

    pub(crate) const fn clamp(&mut self) {
        let max = self.max_scroll();
        if self.scroll > max {
            self.scroll = max;
        }
    }

    pub(crate) fn rebuild_body(&mut self) {
        let visible_refs = filter_tasks(
            &self.base_tasks,
            &self.query,
            self.assignment_filter.as_ref(),
            &self.matcher,
        );
        let visible: Vec<Task> = visible_refs.into_iter().cloned().collect();

        if visible.is_empty() && self.has_active_filter() {
            let description = self.assignment_filter.as_ref().map_or_else(
                || self.query.clone(),
                |user_id| {
                    if self.query.is_empty() {
                        format!("assigned to {user_id}")
                    } else {
                        format!("{} assigned to {user_id}", self.query)
                    }
                },
            );
            self.body_lines = no_matches_lines(&description);
            self.task_line_ranges.clear();
            self.visible_tasks.clear();
            self.selected_task = None;
            self.scroll = 0;
            return;
        }

        let full = self.full_notes;
        let expanded = &self.expanded_notes;
        let (lines, ranges) = build_body_lines_with_ranges(
            &visible,
            self.today,
            self.assignment.mode().show_in_detail,
            &self.tag_styles,
            |t| full || expanded.contains(&t.id),
        );
        self.body_lines = lines;
        self.task_line_ranges = ranges;
        self.visible_tasks = visible;

        // Clamp / initialize selection: keep the previous index when it
        // still points at a real task (so the user's position survives a
        // search-narrowing); otherwise start at the top.
        self.selected_task = if self.visible_tasks.is_empty() {
            None
        } else {
            let prev = self.selected_task.unwrap_or(0);
            Some(prev.min(self.visible_tasks.len() - 1))
        };

        self.scroll = 0;
        self.ensure_selected_visible();
    }

    pub(crate) fn append_query(&mut self, c: char) {
        self.query.push(c);
        self.rebuild_body();
    }

    pub(crate) fn pop_query(&mut self) {
        if self.query.pop().is_some() {
            self.rebuild_body();
        }
    }

    pub(crate) fn clear_query(&mut self) {
        if !self.query.is_empty() {
            self.query.clear();
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

    /// Approximate how many tasks fit in the current visible area, used for
    /// page-step navigation. Falls back to 1 on tiny terminals.
    pub(crate) fn tasks_per_page(&self) -> usize {
        let h = usize::from(self.last_inner_height.max(1));
        (h / 4).max(1)
    }

    /// Scroll the currently-focused panel a half-page in `up`'s direction.
    /// The brain panel scrolls its vt100 scrollback by half its visible rows;
    /// the tasks panel moves the selection by the same half-page step as bare
    /// `d`/`u`. Bound to Alt+U / Alt+D, which fire even while the brain panel
    /// is focused or the search filter is active.
    pub(crate) fn scroll_focused_half_page(&mut self, up: bool) {
        match self.focus {
            Panel::Brain => {
                if let Some(pty) = self.brain.as_mut() {
                    let step = half_page_step(pty.terminal_rows());
                    if up {
                        pty.scroll_up(step);
                    } else {
                        pty.scroll_down(step);
                    }
                }
            }
            Panel::Tasks => {
                let step = (self.tasks_per_page() / 2).max(1);
                if up {
                    self.select_prev(step);
                } else {
                    self.select_next(step);
                }
            }
        }
    }

    pub(crate) fn select_next(&mut self, n: usize) {
        let Some(sel) = self.selected_task else {
            return;
        };
        let len = self.visible_tasks.len();
        if len == 0 {
            return;
        }
        self.set_selected(sel.saturating_add(n).min(len - 1));
    }

    pub(crate) fn select_prev(&mut self, n: usize) {
        let Some(sel) = self.selected_task else {
            return;
        };
        self.set_selected(sel.saturating_sub(n));
    }

    pub(crate) fn set_selected(&mut self, idx: usize) {
        if self.visible_tasks.is_empty() {
            self.selected_task = None;
            return;
        }
        let clamped = idx.min(self.visible_tasks.len() - 1);
        if Some(clamped) == self.selected_task {
            return;
        }
        self.selected_task = Some(clamped);
        // Body content is identical regardless of selection — the highlight
        // is a draw-time background block, not a line mutation. So we only
        // need to nudge scroll to keep the new pick visible.
        self.ensure_selected_visible();
    }

    pub(crate) fn select_first(&mut self) {
        self.set_selected(0);
    }

    pub(crate) fn select_last(&mut self) {
        if !self.visible_tasks.is_empty() {
            self.set_selected(self.visible_tasks.len() - 1);
        }
    }

    /// Scroll so the selected task's content is fully on-screen. Called
    /// after every selection change and at the top of `draw_tasks` once
    /// `last_inner_height` is known.
    pub(crate) fn ensure_selected_visible(&mut self) {
        let Some(sel) = self.selected_task else {
            return;
        };
        let Some(range) = self.task_line_ranges.get(sel) else {
            return;
        };
        let vis = visual_range(&self.visual_row_offsets, range.clone());
        let start = vis.start;
        let end = vis.end;
        let inner = self.last_inner_height.max(1);

        if start < self.scroll {
            self.scroll = start;
        } else if end > self.scroll.saturating_add(inner) {
            self.scroll = end.saturating_sub(inner);
        }
        self.clamp();
    }
}

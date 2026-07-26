//! Confirm + brain-input modal behavior (the state structs live in the
//! crate root so `draw` can read their fields).

use super::*;

use std::path::PathBuf;

impl ConfirmState {
    pub(crate) fn mark_complete(task_id: String, task_label: String) -> Self {
        Self {
            kind: ConfirmKind::MarkComplete,
            // Completing a task is constructive — green, not the destructive red.
            intent: ConfirmIntent::Success,
            title: "Confirm".to_owned(),
            prompt: format!("Mark {task_id} as complete?"),
            task_id,
            task_label,
            path: None,
            focus: ConfirmChoice::Yes,
        }
    }

    pub(crate) fn remove(task_id: String, task_label: String) -> Self {
        Self {
            kind: ConfirmKind::Remove,
            intent: ConfirmIntent::Danger,
            title: format!("Remove {task_id}"),
            prompt: format!("Are you sure you want to remove {task_id}?"),
            task_id,
            task_label,
            path: None,
            focus: ConfirmChoice::Yes,
        }
    }

    /// Ctrl+A landed on an empty `/tmp/<today>.md`. Prompts the user to
    /// generate today's agenda before opening it.
    pub(crate) fn generate_agenda() -> Self {
        Self {
            kind: ConfirmKind::GenerateAgenda,
            // Generating an agenda is constructive.
            intent: ConfirmIntent::Success,
            title: "No agenda for today".to_owned(),
            prompt: "Generate today's agenda?".to_owned(),
            task_id: String::new(),
            task_label: String::new(),
            path: None,
            focus: ConfirmChoice::Yes,
        }
    }

    /// Startup nudge: today's triage habit isn't done. Captures the
    /// habit id + name purely for display; the Yes path always sends a
    /// `/triage` message regardless of which habit is wired in.
    pub(crate) fn run_triage(task_id: String, task_label: String) -> Self {
        Self {
            kind: ConfirmKind::RunTriage,
            // Running triage is constructive.
            intent: ConfirmIntent::Success,
            title: "Daily triage".to_owned(),
            prompt: "Today's triage isn't done. Run it now?".to_owned(),
            task_id,
            task_label,
            path: None,
            focus: ConfirmChoice::Yes,
        }
    }

    pub(crate) fn show_logs(path: PathBuf) -> Self {
        Self {
            kind: ConfirmKind::ShowLogs,
            intent: ConfirmIntent::Success,
            title: "Show logs".to_owned(),
            prompt: format!("Would you like to open {}?", path.display()),
            task_id: String::new(),
            task_label: "Yes opens the log directory and the log file.".to_owned(),
            path: Some(path),
            focus: ConfirmChoice::Yes,
        }
    }

    /// The buttons this modal shows, left-to-right. Every modal has
    /// `Yes` / `No`; only the daily-triage nudge adds `Skip`.
    pub(crate) const fn choices(&self) -> &'static [ConfirmChoice] {
        match self.kind {
            ConfirmKind::RunTriage => &[ConfirmChoice::Yes, ConfirmChoice::No, ConfirmChoice::Skip],
            _ => &[ConfirmChoice::Yes, ConfirmChoice::No],
        }
    }

    /// Whether this modal offers the `Skip` button.
    pub(crate) fn has_skip(&self) -> bool {
        self.choices().contains(&ConfirmChoice::Skip)
    }

    /// Move focus one button to the right, clamped at the last choice.
    pub(crate) fn focus_next(&mut self) {
        let choices = self.choices();
        let idx = choices.iter().position(|&c| c == self.focus).unwrap_or(0);
        if idx + 1 < choices.len() {
            self.focus = choices[idx + 1];
        }
    }

    /// Move focus one button to the left, clamped at the first choice.
    pub(crate) fn focus_prev(&mut self) {
        let choices = self.choices();
        let idx = choices.iter().position(|&c| c == self.focus).unwrap_or(0);
        if idx > 0 {
            self.focus = choices[idx - 1];
        }
    }
}
impl BrainInputState {
    pub(crate) const fn about(task_id: String, task_label: String) -> Self {
        Self {
            buffer: String::new(),
            about_task: Some(task_id),
            task_label: Some(task_label),
        }
    }

    /// Assemble the final message to send: the user buffer, optionally
    /// prefixed with the task-context preamble. Returns `None` when
    /// there's nothing meaningful to send. The preamble includes the
    /// task label when available so the brain agent doesn't have to
    /// resolve the ID before answering.
    pub(crate) fn finalize(self) -> Option<String> {
        let buf = self.buffer.trim();
        if buf.is_empty() {
            return None;
        }
        let prefix = match (self.about_task, self.task_label) {
            (Some(id), Some(label)) => {
                format!("This message is about {id} ({label}): {buf}")
            }
            (Some(id), None) => format!("This message is about {id}: {buf}"),
            _ => buf.to_owned(),
        };
        Some(prefix)
    }
}

impl LinkPickerState {
    /// Build a picker over `links` for `task_id`. Callers only open the
    /// modal when there are ≥ 2 links; the selection starts on the first
    /// (the Linear issue, when present).
    pub(crate) fn new(task_id: String, links: Vec<Link>) -> Self {
        Self {
            task_id,
            links,
            selected: 0,
        }
    }

    pub(crate) fn title(&self) -> String {
        format!("Open link · {}", self.task_id)
    }

    pub(crate) fn links(&self) -> &[Link] {
        &self.links
    }

    pub(crate) const fn selected(&self) -> usize {
        self.selected
    }

    pub(crate) const fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub(crate) fn move_down(&mut self) {
        if self.selected + 1 < self.links.len() {
            self.selected += 1;
        }
    }

    /// Jump straight to the 1-based row `n` (the digit the user typed), if it
    /// exists. Returns whether the jump landed on a real row.
    pub(crate) fn select_number(&mut self, n: usize) -> bool {
        if n >= 1 && n <= self.links.len() {
            self.selected = n - 1;
            true
        } else {
            false
        }
    }

    /// The URL of the highlighted row.
    pub(crate) fn selected_url(&self) -> Option<&str> {
        self.links.get(self.selected).map(|l| l.url.as_str())
    }
}

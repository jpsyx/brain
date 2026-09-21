//! Command dispatch plus the `run_*` handlers shared by palette rows and
//! confirm Yes-paths (mark-complete, remove, agenda, habits, links).

use std::path::Path;

use anyhow::Result;

use crate::tasks::complete;
use crate::tui::App;
use crate::tui::logs_view::{LogKind, LogsView};
use crate::tui::modal_state::{ConfirmState, FlashKind, LinkPickerState};
use crate::tui::overlay::{Overlay, close_overlay, open_overlay};
use crate::tui::palette::Command;
use crate::tui::state::TaskLinksPlan;

impl App {
    /// Run one palette command. Commands that need a target they don't have
    /// raise the matching picker instead of failing — the palette lists every
    /// command from every view, so "no task highlighted" is a question to ask,
    /// not a reason to do nothing.
    pub(crate) fn execute_command(&mut self, command: Command) {
        close_overlay(&mut self.overlay);
        match command {
            Command::Global(action) => self.execute_global_action(action),
            Command::Task(task) => match self.resolve_task_target(task) {
                Some(target) => self.run_task_command(task, &target),
                None => self.open_task_target_picker(task),
            },
            Command::Entry(entry) => match self.resolve_entry_target(entry) {
                Some(path) => self.run_entry_command(entry, &path),
                None => self.open_entry_target_picker(entry),
            },
        }
    }

    pub(crate) fn show_logs_view(&mut self, kind: LogKind) {
        crate::logging::log(format!("open logs view kind={kind:?}"));
        self.shell
            .show_logs(LogsView::load(kind, self.context.log_path()));
    }

    /// Wrap `mark_task_complete` with flash-message setting so both the
    /// palette action and the confirm modal route through one place.
    pub(crate) fn run_mark_complete(&mut self, raw_id: &str) {
        let flash = match self.mark_task_complete(raw_id) {
            Ok(()) => FlashKind::Info(format!("✓ {raw_id} marked complete")),
            Err(e) => FlashKind::Error(format!("⚠ {e}")),
        };
        self.status.set_flash(flash);
    }

    /// Hand the remove off to the brain agent. The prompt asks the agent
    /// to auto-delete when nothing links to the task, and only stop for
    /// a decision when there are preservable links — keeps the no-impact
    /// case from costing the user a back-and-forth.
    pub(crate) fn run_remove(&mut self, raw_id: &str) {
        if let Err(error) = self.tasks.validate_removal(raw_id, self.context.config()) {
            self.status
                .set_flash(FlashKind::Error(format!("⚠ {error}")));
            return;
        }
        let message = format!(
            "Remove {raw_id} via the /todo remove path.\n\n\
             If {raw_id} has no links worth preserving (chunked siblings, blockers, project references), delete the row outright and report it in one line.\n\n\
             Otherwise, list the affected links and propose 2-3 options (e.g. hard delete, status=dropped, unlink-then-delete), then stop and wait for me to choose."
        );
        self.send_brain_prompt(&message);
    }

    /// Ctrl+A entry point. Calls the injected `agenda_runner`; on a
    /// non-zero exit (the agenda helper's signal for "no markdown for
    /// today") opens the no-agenda confirm modal. Success goes through
    /// `flash` instead of a modal so the user isn't asked to dismiss a
    /// popup just to look at the agenda window that already opened on
    /// top of the tasks shell.
    pub(super) fn run_open_agenda(&mut self) {
        match self.services.run_agenda() {
            Ok(()) => {
                self.status
                    .set_flash(FlashKind::Info("✓ opened agenda".to_owned()));
            }
            Err(_) => {
                // Don't surface the raw error — the only meaningful
                // failure mode here is "no markdown in /tmp/", which the
                // modal addresses directly.
                open_overlay(
                    &mut self.overlay,
                    Overlay::TaskConfirmation(ConfirmState::generate_agenda()),
                );
            }
        }
    }

    /// "Open habits page" palette entry. Uses the already-attached shared
    /// process, then opens this workspace's ingress-scoped habits page
    /// through the injected `open_runner`, flashing success / error.
    pub(super) fn run_open_habits(&mut self) {
        let flash = match crate::server::lifecycle::ServerClient::default().connect_existing() {
            Ok(record) => {
                let url = self.context.habits_url(record.port);
                self.open_url(&url)
            }
            Err(e) => FlashKind::Error(format!("⚠ habits failed: {e}")),
        };
        self.status.set_flash(flash);
    }

    /// Open the link(s) of `plan`. Zero links flashes which entry had none (a
    /// silent no-op reads as a broken command from a palette row); a single
    /// link opens directly; several raise the picker.
    pub(crate) fn apply_links_plan(&mut self, plan: TaskLinksPlan, named: Option<&str>) {
        match plan {
            TaskLinksPlan::None => {
                if let Some(id) = named {
                    self.status
                        .set_flash(FlashKind::Info(format!("{id} has no links to open")));
                }
            }
            TaskLinksPlan::Open { url } => {
                let flash = self.open_url(&url);
                self.status.set_flash(flash);
            }
            TaskLinksPlan::Choose { task_id, links } => {
                open_overlay(
                    &mut self.overlay,
                    Overlay::LinkPicker(LinkPickerState::new(task_id, links)),
                );
            }
        }
    }

    /// Open the link-picker's highlighted URL and close the modal. No-op
    /// when no picker is open or it somehow has no selection.
    pub(crate) fn open_selected_link(&mut self) {
        let Some(url) = self
            .overlay
            .as_ref()
            .and_then(Overlay::picked_link_url)
            .map(str::to_owned)
        else {
            return;
        };
        let flash = self.open_url(&url);
        self.status.set_flash(flash);
        close_overlay(&mut self.overlay);
    }

    /// Yes-path for the no-agenda confirm modal. Hands off to the brain
    /// agent rather than calling /todo scripts directly because agenda
    /// generation is structured-with-judgement: the agent picks today's
    /// MITs, deduplicates against yesterday's, and writes the markdown.
    pub(crate) fn run_generate_agenda(&mut self) {
        let message = "Generate today's agenda. Use the /todo skill's agenda flow to write \
             /tmp/<today>.md, then let me know it's ready so I can open it with Ctrl+A.";
        self.send_brain_prompt(message);
    }

    /// Complete a task or habit natively, re-sync the day's agenda, then
    /// refresh from disk.
    pub(crate) fn mark_task_complete(&mut self, raw_id: &str) -> Result<()> {
        let id = complete::normalize_id(raw_id)?;
        complete::complete_and_sync_agenda(
            self.context.command(),
            &id,
            chrono::Local::now().date_naive(),
        )?;
        self.reload_tasks()?;
        Ok(())
    }

    pub(super) fn open_url(&self, url: &str) -> FlashKind {
        match self.services.open_url(url) {
            Ok(()) => FlashKind::Info(format!("✓ opened {url}")),
            Err(error) => FlashKind::Error(format!("⚠ open failed: {error}")),
        }
    }
}

/// Build the "start task" brain prompt, interpolating the configured brain root
/// so it never hardcodes `~/brain`. Pure, so the root-authority behavior is
/// unit-testable.
#[must_use]
pub(crate) fn start_task_prompt(id: &str, brain_root: &Path) -> String {
    let tasks_csv = brain_root.join("tasks/tasks.csv");
    let projects_dir = brain_root.join("projects");
    format!(
        "Let's start work on {id}.\n\n\
         Please pull in the task's context first: read the row in {tasks_csv} (notes, project, see_also links, blockers, last_touched), the associated project page in {projects_dir} if a project slug is set, and any supporting URLs.\n\n\
         Then reply with:\n\
         1. The first 2-3 concrete steps I should take to get moving on this.\n\
         2. Where you can help me directly right now — drafting, research, code, planning, summarizing — so we can knock out the first chunk together in this conversation.",
        tasks_csv = tasks_csv.display(),
        projects_dir = projects_dir.display(),
    )
}

#[must_use]
pub(crate) fn add_task_prompt(actor_id: &str) -> String {
    format!(
        "Add a task through the /todo add flow. Default its portable assignment to assigned_to={actor_id} unless I explicitly choose another workspace member. Ask me interactively for any missing task details."
    )
}

#[must_use]
pub(crate) fn reassign_task_prompt(id: &str) -> String {
    format!(
        "Use the /todo assign {id} flow to reassign this task to a portable workspace member. Show me the available members and ask which one should own it."
    )
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod commands_tests;

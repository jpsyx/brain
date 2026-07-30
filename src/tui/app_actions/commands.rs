//! `App` command handlers: the `run_*` actions behind palette rows / confirm
//! Yes-paths (mark-complete, remove, agenda, habits, links), native completion,
//! and the palette-action dispatcher.

use std::path::Path;

use anyhow::Result;

use crate::tasks::complete;
use crate::tui::*;

impl App<'_> {
    pub(crate) fn show_logs_view(&mut self, kind: LogKind) {
        crate::logging::log(format!("open logs view kind={kind:?}"));
        self.logs_view = Some(LogsView::load(kind, self.log_path.as_deref()));
        self.main_view = crate::main_view::MainView::Logs;
    }

    /// Wrap `mark_task_complete` with flash-message setting so both the
    /// palette action and the confirm modal route through one place.
    pub(crate) fn run_mark_complete(&mut self, raw_id: &str) {
        self.flash = Some(match self.mark_task_complete(raw_id) {
            Ok(()) => FlashKind::Info(format!("✓ {raw_id} marked complete")),
            Err(e) => FlashKind::Error(format!("⚠ {e}")),
        });
    }

    /// Hand the remove off to the brain agent. The prompt asks the agent
    /// to auto-delete when nothing links to the task, and only stop for
    /// a decision when there are preservable links — keeps the no-impact
    /// case from costing the user a back-and-forth.
    pub(crate) fn run_remove(&mut self, raw_id: &str) {
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
    pub(crate) fn run_open_agenda(&mut self) {
        match self.agenda_runner.run() {
            Ok(()) => {
                self.flash = Some(FlashKind::Info("✓ opened agenda".to_owned()));
            }
            Err(_) => {
                // Don't surface the raw error — the only meaningful
                // failure mode here is "no markdown in /tmp/", which the
                // modal addresses directly.
                self.confirm = Some(ConfirmState::generate_agenda());
            }
        }
    }

    /// "Open habits page" palette entry. Brings up the bundled brain server
    /// (starting it if needed), then opens its `/habits` page in the browser
    /// through the injected `open_runner`, flashing success / error.
    pub(crate) fn run_open_habits(&mut self) {
        self.flash = Some(match crate::server::lifecycle::ensure_running() {
            Ok(port) => {
                let url = crate::server::url(port, "/habits");
                open_url(self.open_runner.as_ref(), &url)
            }
            Err(e) => FlashKind::Error(format!("⚠ habits failed: {e}")),
        });
    }

    /// Ctrl+O / "open link" entry point. Collects the selected entry's
    /// openable links (Linear issue first, then see_also / notes URLs). Zero links is a
    /// silent no-op; a single link opens directly via the injected
    /// `open_runner`; multiple links raise the picker modal so the user can
    /// choose. The picker is bound to the task's id at open time.
    pub(crate) fn run_open_links(&mut self) {
        let task = self.selected_task.and_then(|i| self.visible_tasks.get(i));
        let Some(task) = task else {
            return;
        };
        let links = task_links(task, &self.config.linear_base_url());
        match links.len() {
            0 => {}
            1 => {
                self.flash = Some(open_url(self.open_runner.as_ref(), &links[0].url));
            }
            _ => {
                let id = task.id.clone();
                self.link_picker = Some(LinkPickerState::new(id, links));
            }
        }
    }

    /// Open the link-picker's highlighted URL and close the modal. No-op
    /// when no picker is open or it somehow has no selection.
    pub(crate) fn open_selected_link(&mut self) {
        let Some(url) = self
            .link_picker
            .as_ref()
            .and_then(LinkPickerState::selected_url)
            .map(str::to_owned)
        else {
            return;
        };
        self.flash = Some(open_url(self.open_runner.as_ref(), &url));
        self.link_picker = None;
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

    /// Complete a task or habit natively, then refresh from disk.
    pub(crate) fn mark_task_complete(&mut self, raw_id: &str) -> Result<()> {
        let id = complete::normalize_id(raw_id)?;
        complete::complete_in_root(&self.brain_root, &id)?;
        self.reload_tasks()?;
        Ok(())
    }

    pub(crate) fn execute_palette_action(&mut self, action: PaletteAction) {
        self.palette = None;
        match action {
            PaletteAction::SendBrainMessage => {
                // Open / focus the persistent brain panel; the user types into it.
                self.open_or_focus_brain(None);
            }
            PaletteAction::CloseBrain => {
                self.close_brain();
            }
            PaletteAction::StartReceiverServer => self.start_receiver_server(),
            PaletteAction::StopReceiverServer => {
                self.receiver_server = None;
                self.receiver_rx = None;
                self.flash = Some(FlashKind::Info("receiver server stopped".to_owned()));
            }
            PaletteAction::RestartReceiverServer => {
                crate::logging::log("palette request receiver server restart");
                self.receiver_server = None;
                self.receiver_rx = None;
                self.start_receiver_server();
            }
            PaletteAction::ShowReceiverServerStatus => {
                crate::logging::log("palette request receiver server status");
                self.flash = Some(FlashKind::ReceiverStatus {
                    running: self.receiver_server_running(),
                });
            }
            PaletteAction::ShowReceiverServerLogs => {
                crate::logging::log("palette request receiver server logs");
                self.show_logs_view(LogKind::Receiver);
            }
            PaletteAction::MessageBrainAboutTask => {
                // Clone (id, name) before mutating self.brain_input so
                // the borrow on visible_tasks ends first.
                let target = self
                    .selected_task
                    .and_then(|i| self.visible_tasks.get(i))
                    .map(|t| (t.id.clone(), t.name.clone()));
                if let Some((id, label)) = target {
                    self.brain_input = Some(BrainInputState::about(id, label));
                }
            }
            PaletteAction::MarkTaskComplete => {
                // Open a Yes/No confirmation rather than completing
                // immediately — same guard as the Ctrl+Enter shortcut,
                // since this mutates tasks.csv. The Yes path calls
                // `run_mark_complete`.
                let target = self
                    .selected_task
                    .and_then(|i| self.visible_tasks.get(i))
                    .map(|t| (t.id.clone(), t.name.clone()));
                if let Some((id, label)) = target {
                    self.confirm = Some(ConfirmState::mark_complete(id, label));
                }
            }
            PaletteAction::DeferTask(days) => {
                let Some(id) = self.current_task_id() else {
                    return;
                };
                // Hand off to the brain agent (which has the /todo skill
                // loaded) rather than calling defer_task.py directly —
                // keeps the user in the loop in case the defer has
                // chunked-task cascade implications worth a glance.
                let day_word = if days == 1 { "day" } else { "days" };
                let message = format!("Defer task {id} by {days} {day_word}");
                self.send_brain_prompt(&message);
            }
            PaletteAction::RemoveTask => {
                // Open a Yes/No confirmation rather than firing off the
                // remove immediately — destructive enough to warrant the
                // extra keystroke. The Yes path calls `run_remove`.
                let target = self
                    .selected_task
                    .and_then(|i| self.visible_tasks.get(i))
                    .map(|t| (t.id.clone(), t.name.clone()));
                if let Some((id, label)) = target {
                    self.confirm = Some(ConfirmState::remove(id, label));
                }
            }
            PaletteAction::OpenHabitsInBrowser => {
                self.run_open_habits();
            }
            PaletteAction::SyncBrainNow => {
                crate::sync::trigger::spawn_detached_sync(crate::sync::args::Direction::Both);
                self.flash = Some(FlashKind::Info("✓ sync started".to_owned()));
            }
            PaletteAction::OpenAgenda => {
                self.run_open_agenda();
            }
            PaletteAction::ShowBrainLogs => {
                crate::logging::log("palette request brain TUI logs");
                self.show_logs_view(LogKind::Brain);
            }
            PaletteAction::ReturnToMainView => {
                crate::logging::log("palette request return to main view");
                self.main_view = crate::main_view::MainView::Tasks;
            }
            PaletteAction::ToggleNotes => {
                self.toggle_notes();
            }
            PaletteAction::OpenLinks => {
                self.run_open_links();
            }
            PaletteAction::StartTask => {
                let Some(id) = self.current_task_id() else {
                    return;
                };
                // Asks the brain agent to (1) gather the task's context
                // (notes / project / see_also / blockers) before
                // proposing anything, (2) give a short list of concrete
                // first steps, and (3) explicitly call out where it can
                // help right now — drafting, research, code, etc. —
                // so the next reply is actionable rather than just
                // advisory.
                let message = start_task_prompt(&id, &self.brain_root);
                self.send_brain_prompt(&message);
            }
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

#[cfg(test)]
mod tests {
    use super::start_task_prompt;
    use std::path::Path;

    #[test]
    fn start_task_prompt_interpolates_the_configured_root() {
        let p = start_task_prompt("T7", Path::new("/srv/brain"));
        assert!(p.contains("T7"));
        assert!(p.contains("/srv/brain/tasks/tasks.csv"));
        assert!(p.contains("/srv/brain/projects"));
    }

    #[test]
    fn start_task_prompt_never_hardcodes_tilde_brain() {
        let p = start_task_prompt("T1", Path::new("/custom/root"));
        assert!(!p.contains("~/brain"));
    }
}

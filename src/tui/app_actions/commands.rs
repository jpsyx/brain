//! `App` command handlers: the `run_*` actions behind palette rows / confirm
//! Yes-paths (mark-complete, remove, agenda, habits, links), native completion,
//! and the palette-action dispatcher.

use std::path::Path;

use anyhow::Result;

use crate::tasks::complete;
use crate::tui::*;

use super::triage::{TriageAlertEvent, should_check_daily_triage};

fn task_for_id<'a>(tasks: &'a [Task], habits: &'a [Task], raw_id: &str) -> Option<&'a Task> {
    tasks
        .iter()
        .chain(habits)
        .find(|task| task.id.eq_ignore_ascii_case(raw_id))
}

fn protect_completion(
    tasks: &[Task],
    habits: &[Task],
    raw_id: &str,
    config: &Config,
) -> Result<(), crate::tasks::triage_habits::ManagedTaskError> {
    task_for_id(tasks, habits, raw_id).map_or(Ok(()), |task| {
        crate::tasks::triage_habits::can_complete(task, config)
    })
}

fn protect_removal(
    tasks: &[Task],
    habits: &[Task],
    raw_id: &str,
    config: &Config,
) -> Result<(), crate::tasks::triage_habits::ManagedTaskError> {
    task_for_id(tasks, habits, raw_id).map_or(Ok(()), |task| {
        crate::tasks::triage_habits::can_remove(task, config)
    })
}

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
        if let Err(error) = protect_removal(&self.all_tasks, &self.all_habits, raw_id, &self.config)
        {
            self.flash = Some(FlashKind::Error(format!("⚠ {error}")));
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

    /// "Open habits page" palette entry. Uses the already-attached shared
    /// process, then opens this workspace's ingress-scoped habits page
    /// through the injected `open_runner`, flashing success / error.
    pub(crate) fn run_open_habits(&mut self) {
        self.flash = Some(
            match crate::server::lifecycle::ServerClient::default().connect_existing() {
                Ok(record) => {
                    let url = self.habits_url_for_port(record.port);
                    open_url(self.open_runner.as_ref(), &url)
                }
                Err(e) => FlashKind::Error(format!("⚠ habits failed: {e}")),
            },
        );
    }

    pub(crate) fn habits_url_for_port(&self, port: u16) -> String {
        crate::server::habits_url(port, self.server_ingress, self.server_local_capability)
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
        protect_completion(&self.all_tasks, &self.all_habits, &id, &self.config)?;
        complete::complete_in_workspace_for_actor_with_today(
            &self.command_context.workspace,
            &id,
            chrono::Local::now().date_naive(),
            &self.command_context.actor,
        )?;
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
            PaletteAction::AddTask => {
                let message = add_task_prompt(self.assignment.actor_id().as_str());
                self.send_brain_prompt(&message);
            }
            PaletteAction::CloseBrain => {
                self.close_brain();
            }
            PaletteAction::ToggleReceiver => self.toggle_receiver(),
            PaletteAction::ShowReceiverServerStatus => self.show_receiver_status(),
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
            PaletteAction::ReassignTask => {
                let Some(id) = self.current_task_id() else {
                    return;
                };
                let message = reassign_task_prompt(&id);
                self.send_brain_prompt(&message);
            }
            PaletteAction::ChooseAssigneeFilter => {
                self.assignee_filter = Some(AssigneeFilterState::new(
                    self.assignment.users(),
                    self.assignment_filter.as_ref(),
                ));
            }
            PaletteAction::OpenHabitsInBrowser => {
                self.run_open_habits();
            }
            PaletteAction::SyncBrainNow => {
                if crate::sync::trigger::spawn_detached_sync(
                    &self.command_context.workspace,
                    crate::sync::args::Direction::Both,
                )
                .is_some()
                {
                    self.flash = Some(FlashKind::Info("✓ sync started".to_owned()));
                } else {
                    self.flash = Some(FlashKind::Error("sync could not start".to_owned()));
                }
            }
            PaletteAction::ShowSyncStatus => {
                crate::logging::log("palette request sync status");
                let workspace_paths = self.command_context.workspace.paths();
                self.flash = Some(FlashKind::Info(
                    crate::sync::current::read_state(workspace_paths)
                        .filter(|state| crate::server::lifecycle::pid_alive(state.pid))
                        .map_or_else(
                            || "no sync is currently running".to_owned(),
                            |state| format!("syncing now ({})", state.direction),
                        ),
                ));
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
            PaletteAction::ToggleDailyTriageAlert => {
                self.skip_daily_triage_check = !self.skip_daily_triage_check;
                if self.skip_daily_triage_check {
                    crate::logging::log("palette disabled daily triage alert for session");
                    self.flash = Some(FlashKind::Info(
                        "daily triage alert disabled for this session".to_owned(),
                    ));
                } else {
                    crate::logging::log("palette enabled daily triage alert for session");
                    self.flash = Some(FlashKind::Info(
                        "daily triage alert enabled for this session".to_owned(),
                    ));
                    // Re-enabling re-arms the nudge. While startup refresh is
                    // pending, wait for synced config and habits instead of
                    // evaluating stale local state.
                    if should_check_daily_triage(
                        TriageAlertEvent::PaletteEnabled,
                        self.triage_gate.is_some(),
                        self.skip_daily_triage_check,
                    ) {
                        self.check_daily_triage();
                    }
                }
            }
            PaletteAction::ShowMainBrainSession => {
                self.select_brain_tab(BrainTab::Main);
            }
            PaletteAction::ShowDailyTriageSession => {
                self.select_brain_tab(BrainTab::Triage);
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

#[cfg(test)]
mod managed_triage_tests {
    use super::{protect_completion, protect_removal};

    fn managed() -> crate::tasks::task::Task {
        let mut task = crate::tasks::task::test_task("H7", "not_started");
        task.system_key = crate::tasks::triage_habits::DAILY_SYSTEM_KEY.to_owned();
        task
    }

    #[test]
    fn actual_tui_mutation_guards_reject_managed_rows() {
        let task = managed();
        let config = crate::config::Config::default();

        assert!(protect_completion(&[], std::slice::from_ref(&task), "H7", &config).is_err());
        assert!(protect_removal(&[], &[task], "H7", &config).is_err());
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
mod tests {
    use super::{add_task_prompt, reassign_task_prompt, start_task_prompt};
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

    #[test]
    fn add_task_prompt_defaults_assignment_to_the_current_actor() {
        let prompt = add_task_prompt("wife");

        assert!(prompt.contains("/todo add"));
        assert!(prompt.contains("assigned_to=wife"));
        assert!(prompt.contains("unless I explicitly choose another workspace member"));
    }

    #[test]
    fn reassign_task_prompt_targets_the_selected_task() {
        let prompt = reassign_task_prompt("T7");

        assert!(prompt.contains("/todo assign T7"));
        assert!(prompt.contains("workspace member"));
    }
}

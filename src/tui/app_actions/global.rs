//! Running a [`GlobalAction`] — the commands that need no target.

use crate::entry::Bucket;
use crate::main_view::MainView;
use crate::tui::App;
use crate::tui::action::GlobalAction;
use crate::tui::logs_view::LogKind;
use crate::tui::modal_state::{AssigneeFilterState, FlashKind, HelpState, SyncLogState};
use crate::tui::model::BrainTab;
use crate::tui::overlay::{Overlay, open_overlay};
use crate::tui::search_view::{all_bucket_roots, single_bucket_root};

use super::triage::{TriageAlertEvent, should_check_daily_triage};

impl App {
    pub(crate) fn execute_global_action(&mut self, action: GlobalAction) {
        match action {
            GlobalAction::MessageBrain => {
                self.open_or_focus_brain(None);
            }
            GlobalAction::StartManualSession => self.start_default_manual_session(),
            GlobalAction::RenameSession => self.open_session_rename_picker(),
            GlobalAction::CloseSession => self.open_session_close_picker(),
            GlobalAction::NewConversation => self.start_new_conversation(),
            GlobalAction::ShowSessionTab(id) => {
                self.select_brain_tab(BrainTab::Session(id));
            }
            GlobalAction::ShowMainBrainSession => {
                self.select_brain_tab(BrainTab::Main);
            }
            GlobalAction::CycleBrainTab(forward) => self.cycle_brain_tab(forward),
            GlobalAction::FocusBrainPanel => self.focus_brain(),
            GlobalAction::FocusMainPanel => self.focus_tasks(),
            GlobalAction::ToggleReceiver => self.toggle_receiver(),
            GlobalAction::ToggleLayout => {
                self.shell.toggle_panel_side();
                let _ = self.services.save_panel_side(self.shell.panel_side());
            }
            GlobalAction::ShowTasks => self.shell.show_main_view(MainView::Tasks),
            GlobalAction::ShowBrainSearch => self.shell.show_brain_search(),
            GlobalAction::OpenFileExplorer => self.open_file_explorer(),
            GlobalAction::ToggleHiddenFiles => self.toggle_hidden_files(),
            GlobalAction::ShowReceiverServerStatus => self.show_receiver_status(),
            GlobalAction::ShowReceiverServerLogs => {
                crate::logging::log("palette request receiver server logs");
                self.show_logs_view(LogKind::Receiver);
            }
            GlobalAction::ShowBrainLogs => {
                crate::logging::log("palette request brain TUI logs");
                self.show_logs_view(LogKind::Brain);
            }
            GlobalAction::OpenHabits => self.run_open_habits(),
            GlobalAction::SyncBrainNow => self.run_sync_now(),
            GlobalAction::ShowSyncStatus => {
                crate::logging::log("palette request sync status");
                open_overlay(
                    &mut self.overlay,
                    Overlay::SyncLog(SyncLogState { scroll: u16::MAX }),
                );
            }
            GlobalAction::OpenAgenda => self.run_open_agenda(),
            GlobalAction::ToggleDailyTriageAlert => self.toggle_daily_triage_alert(),
            GlobalAction::RunSkillSession(key) => self.run_skill_session(key),
            GlobalAction::AddTask => {
                let message =
                    super::commands::add_task_prompt(self.tasks.assignment_snapshot().actor_id.as_str());
                self.send_brain_prompt(&message);
            }
            GlobalAction::ChooseAssigneeFilter => {
                self.show_tasks_view();
                open_overlay(
                    &mut self.overlay,
                    Overlay::AssigneeFilter(AssigneeFilterState::new(
                        self.tasks.assignment_snapshot().users,
                        self.tasks.assignment_snapshot().filter,
                    )),
                );
            }
            GlobalAction::ClearTaskFilters => {
                self.show_tasks_view();
                self.tasks.clear_active_filters();
            }
            GlobalAction::SearchTasks => {
                self.show_tasks_view();
                self.tasks.enter_search();
            }
            GlobalAction::ReloadTasks => self.refresh(),
            GlobalAction::ShowTaskView(view) => {
                self.show_tasks_view();
                self.tasks.set_view(view);
            }
            GlobalAction::RefreshBrainDirectory => self.search_refresh(),
            GlobalAction::SearchBucket(bucket) => self.rescope_search(Some(bucket)),
            GlobalAction::SearchEverything => self.rescope_search(None),
            GlobalAction::CreateCaptureNote => self.open_capture_note_input(),
            GlobalAction::ShowShortcuts => {
                open_overlay(&mut self.overlay, Overlay::Help(HelpState { scroll: 0 }));
            }
            GlobalAction::Quit => self.shell.request_quit(),
        }
    }

    /// Bring the tasks view forward and give it focus, so a task command run
    /// from the brain panel or another main view lands somewhere visible.
    fn show_tasks_view(&mut self) {
        self.shell.show_main_view(MainView::Tasks);
        self.shell.focus_tasks();
    }

    /// Rescope the brain-directory search to one bucket, or to all of them,
    /// and show the view so the change is visible.
    fn rescope_search(&mut self, bucket: Option<Bucket>) {
        let root = self.context.workspace_root().to_path_buf();
        let roots = bucket.map_or_else(|| all_bucket_roots(&root), |b| single_bucket_root(&root, b));
        self.search_rescope(&roots);
        self.shell.show_main_view(MainView::BrainSearch);
        self.shell.focus_tasks();
    }

    fn run_sync_now(&mut self) {
        if crate::sync::trigger::spawn_detached_sync(
            self.context.workspace(),
            crate::sync::args::Direction::Both,
        )
        .is_some()
        {
            self.status
                .set_flash(FlashKind::Info("✓ sync started".to_owned()));
        } else {
            self.status
                .set_flash(FlashKind::Error("sync could not start".to_owned()));
        }
    }

    fn toggle_daily_triage_alert(&mut self) {
        let disabled = self.status.toggle_daily_triage_check();
        let persisted = self.persist_daily_triage_check();
        if disabled {
            crate::logging::log("palette disabled daily triage alert");
            self.status.set_flash(FlashKind::Info(persisted.map_or_else(
                |error| {
                    format!(
                        "daily triage alert disabled for this session only; saving it failed: {error:#}"
                    )
                },
                |()| "daily triage alert disabled (saved to config)".to_owned(),
            )));
            return;
        }
        crate::logging::log("palette enabled daily triage alert");
        self.status.set_flash(FlashKind::Info(persisted.map_or_else(
            |error| {
                format!(
                    "daily triage alert enabled for this session only; saving it failed: {error:#}"
                )
            },
            |()| "daily triage alert enabled (saved to config)".to_owned(),
        )));
        if should_check_daily_triage(
            TriageAlertEvent::PaletteEnabled,
            self.status.triage_gate_is_armed(),
            self.status.daily_triage_check_disabled(),
        ) {
            self.check_daily_triage();
        }
    }
}

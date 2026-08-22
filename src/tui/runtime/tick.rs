use crate::server::control::{HeartbeatEvent, HeartbeatWorker};
use crate::tui::{App, FlashKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecurringStage {
    CloseExitedPanelAndRefreshTasks,
    DrainServerHealthEvents,
    TickSkillSessions,
    TickReceiver,
    TickSyncStatusAndRefreshTasks,
    TickTriageGateAndRefreshTasks,
}

pub(super) const fn recurring_stages() -> [RecurringStage; 6] {
    [
        RecurringStage::CloseExitedPanelAndRefreshTasks,
        RecurringStage::DrainServerHealthEvents,
        RecurringStage::TickSkillSessions,
        RecurringStage::TickReceiver,
        RecurringStage::TickSyncStatusAndRefreshTasks,
        RecurringStage::TickTriageGateAndRefreshTasks,
    ]
}

pub(super) fn tick(app: &mut App, server_lease: &HeartbeatWorker) {
    for stage in recurring_stages() {
        match stage {
            RecurringStage::CloseExitedPanelAndRefreshTasks => {
                app.close_exited_brain_panel();
            }
            RecurringStage::DrainServerHealthEvents => {
                for event in server_lease.poll() {
                    match event {
                        HeartbeatEvent::Recovered(generation) => {
                            crate::logging::log(format!(
                                "shared server recovered at generation {generation}"
                            ));
                        }
                        HeartbeatEvent::RecoveryFailed(error) => {
                            crate::logging::log(format!("shared server recovery failed: {error}"));
                            app.status.set_flash(FlashKind::Error(
                                "shared server unavailable; reconnecting".to_owned(),
                            ));
                        }
                    }
                }
            }
            RecurringStage::TickSkillSessions => app.tick_skill_sessions(),
            RecurringStage::TickReceiver => app.tick_receiver(),
            RecurringStage::TickSyncStatusAndRefreshTasks => app.tick_sync_status(),
            RecurringStage::TickTriageGateAndRefreshTasks => app.tick_triage_gate(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RefreshStage {
    AdvanceLogicalDay,
    ReloadTasks,
    CheckDailyTriageAfterRollover,
    ReportRefresh,
}

pub(super) const fn refresh_stages() -> [RefreshStage; 4] {
    [
        RefreshStage::AdvanceLogicalDay,
        RefreshStage::ReloadTasks,
        RefreshStage::CheckDailyTriageAfterRollover,
        RefreshStage::ReportRefresh,
    ]
}

pub(in crate::tui) fn refresh(app: &mut App) {
    let mut rolled = false;
    let mut reload = None;
    for stage in refresh_stages() {
        match stage {
            RefreshStage::AdvanceLogicalDay => {
                rolled = app.advance_triage_day(chrono::Local::now().naive_local());
            }
            RefreshStage::ReloadTasks => reload = Some(app.reload_tasks()),
            RefreshStage::CheckDailyTriageAfterRollover => {
                if rolled {
                    app.check_daily_triage();
                }
            }
            RefreshStage::ReportRefresh => {
                let flash = match reload
                    .take()
                    .expect("the refresh coordinator reloads tasks before reporting")
                {
                    Ok(()) => FlashKind::Info("✓ refreshed".to_owned()),
                    Err(error) => FlashKind::Error(format!("⚠ reload failed: {error}")),
                };
                app.status.set_flash(flash);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RecurringStage, RefreshStage, recurring_stages, refresh_stages};

    #[test]
    fn recurring_coordinator_preserves_current_feature_order() {
        assert_eq!(
            recurring_stages(),
            [
                RecurringStage::CloseExitedPanelAndRefreshTasks,
                RecurringStage::DrainServerHealthEvents,
                RecurringStage::TickSkillSessions,
                RecurringStage::TickReceiver,
                RecurringStage::TickSyncStatusAndRefreshTasks,
                RecurringStage::TickTriageGateAndRefreshTasks,
            ]
        );
    }

    #[test]
    fn manual_refresh_preserves_logical_day_then_task_refresh_order() {
        assert_eq!(
            refresh_stages(),
            [
                RefreshStage::AdvanceLogicalDay,
                RefreshStage::ReloadTasks,
                RefreshStage::CheckDailyTriageAfterRollover,
                RefreshStage::ReportRefresh,
            ]
        );
    }
}

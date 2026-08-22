use std::time::Instant;

use chrono::NaiveDate;

use crate::tui::modal_state::FlashKind;

pub(crate) struct StatusStateInit {
    pub(crate) triage_day: NaiveDate,
    pub(crate) skip_daily_triage_check: bool,
    pub(crate) persistent_warning: Option<String>,
    pub(crate) sync_status_next_poll: Instant,
    pub(crate) last_seen_downstream_id: Option<i64>,
}

pub(crate) struct StatusState {
    triage_day: NaiveDate,
    triage_gate: Option<TriageGate>,
    skip_daily_triage_check: bool,
    flash: Option<FlashKind>,
    persistent_warning: Option<String>,
    alert: Option<String>,
    sync_status: Option<String>,
    sync_status_next_poll: Instant,
    last_seen_downstream_id: Option<i64>,
}

struct TriageGate {
    pub(crate) seen_journal_id: Option<i64>,
    pub(crate) next_poll: Instant,
    pub(crate) refresh_complete: bool,
}

impl StatusState {
    pub(crate) fn new(init: StatusStateInit) -> Self {
        Self {
            triage_day: init.triage_day,
            triage_gate: None,
            skip_daily_triage_check: init.skip_daily_triage_check,
            flash: None,
            persistent_warning: init.persistent_warning,
            alert: None,
            sync_status: None,
            sync_status_next_poll: init.sync_status_next_poll,
            last_seen_downstream_id: init.last_seen_downstream_id,
        }
    }

    #[must_use]
    pub(crate) fn triage_day(&self) -> NaiveDate {
        self.triage_day
    }

    pub(crate) fn set_triage_day(&mut self, day: NaiveDate) {
        self.triage_day = day;
    }

    pub(crate) fn arm_triage_gate(&mut self, seen_journal_id: Option<i64>, now: Instant) {
        self.triage_gate = Some(TriageGate {
            seen_journal_id,
            next_poll: now,
            refresh_complete: false,
        });
    }

    #[must_use]
    pub(crate) fn triage_gate_observation(&self) -> Option<(Option<i64>, Instant, bool)> {
        self.triage_gate
            .as_ref()
            .map(|gate| (gate.seen_journal_id, gate.next_poll, gate.refresh_complete))
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn triage_seen_journal_id(&self) -> Option<i64> {
        self.triage_gate
            .as_ref()
            .and_then(|gate| gate.seen_journal_id)
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn triage_refresh_complete(&self) -> bool {
        self.triage_gate
            .as_ref()
            .is_some_and(|gate| gate.refresh_complete)
    }

    #[must_use]
    pub(crate) fn triage_gate_is_armed(&self) -> bool {
        self.triage_gate.is_some()
    }

    pub(crate) fn mark_triage_refresh_complete(&mut self) {
        if let Some(gate) = self.triage_gate.as_mut() {
            gate.refresh_complete = true;
        }
    }

    pub(crate) fn delay_triage_gate_poll(&mut self, next_poll: Instant) {
        if let Some(gate) = self.triage_gate.as_mut() {
            gate.next_poll = next_poll;
        }
    }

    pub(crate) fn clear_triage_gate(&mut self) {
        self.triage_gate = None;
    }

    #[must_use]
    pub(crate) fn daily_triage_check_disabled(&self) -> bool {
        self.skip_daily_triage_check
    }

    #[must_use]
    pub(crate) fn toggle_daily_triage_check(&mut self) -> bool {
        self.skip_daily_triage_check = !self.skip_daily_triage_check;
        self.skip_daily_triage_check
    }

    #[cfg(test)]
    pub(crate) fn set_daily_triage_check_disabled(&mut self, disabled: bool) {
        self.skip_daily_triage_check = disabled;
    }

    #[must_use]
    pub(crate) fn flash(&self) -> Option<&FlashKind> {
        self.flash.as_ref()
    }

    pub(crate) fn set_flash(&mut self, flash: FlashKind) {
        self.flash = Some(flash);
    }

    pub(crate) fn clear_flash(&mut self) {
        self.flash = None;
    }

    #[must_use]
    pub(crate) fn persistent_warning(&self) -> Option<&str> {
        self.persistent_warning.as_deref()
    }

    #[must_use]
    pub(crate) fn alert(&self) -> Option<&str> {
        self.alert.as_deref()
    }

    pub(crate) fn set_alert(&mut self, alert: Option<String>) {
        self.alert = alert;
    }

    pub(crate) fn clear_alert(&mut self) {
        self.alert = None;
    }

    #[must_use]
    pub(crate) fn sync_status(&self) -> Option<&str> {
        self.sync_status.as_deref()
    }

    pub(crate) fn set_sync_status(&mut self, sync_status: Option<String>) {
        self.sync_status = sync_status;
    }

    #[must_use]
    pub(crate) fn sync_poll_due(&self, now: Instant) -> bool {
        now >= self.sync_status_next_poll
    }

    pub(crate) fn schedule_next_sync_poll(&mut self, now: Instant) {
        self.sync_status_next_poll = now + crate::sync::freshness::STATUS_POLL_INTERVAL;
    }

    #[cfg(test)]
    pub(crate) fn set_sync_poll_deadline(&mut self, deadline: Instant) {
        self.sync_status_next_poll = deadline;
    }

    #[must_use]
    pub(crate) fn last_seen_downstream_id(&self) -> Option<i64> {
        self.last_seen_downstream_id
    }

    pub(crate) fn record_downstream_id(&mut self, id: Option<i64>) {
        self.last_seen_downstream_id = id;
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::tui::modal_state::FlashKind;

    use super::{StatusState, StatusStateInit};

    fn status() -> StatusState {
        StatusState::new(StatusStateInit {
            triage_day: NaiveDate::from_ymd_opt(2026, 8, 21).expect("valid day"),
            skip_daily_triage_check: false,
            persistent_warning: Some("receiver phone is incomplete".to_owned()),
            sync_status_next_poll: std::time::Instant::now(),
            last_seen_downstream_id: Some(4),
        })
    }

    #[test]
    fn status_keeps_transient_and_persistent_messages_independent() {
        let mut status = status();
        status.set_flash(FlashKind::Error("could not refresh".to_owned()));
        status.set_alert(Some("daily triage is incomplete".to_owned()));
        status.set_sync_status(Some("pulling workspace".to_owned()));

        status.clear_flash();
        status.clear_alert();

        assert!(status.flash().is_none());
        assert!(status.alert().is_none());
        assert_eq!(
            status.persistent_warning(),
            Some("receiver phone is incomplete")
        );
        assert_eq!(status.sync_status(), Some("pulling workspace"));
    }

    #[test]
    fn status_owns_triage_gate_toggle_and_sync_poll_progress() {
        let mut status = status();
        let now = std::time::Instant::now();
        status.arm_triage_gate(Some(7), now);

        status.mark_triage_refresh_complete();
        status.schedule_next_sync_poll(now);
        status.record_downstream_id(Some(8));

        assert!(status.triage_refresh_complete());
        assert_eq!(status.triage_seen_journal_id(), Some(7));
        assert!(status.toggle_daily_triage_check());
        assert!(!status.sync_poll_due(now));
        assert_eq!(status.last_seen_downstream_id(), Some(8));
    }
}

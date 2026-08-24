use std::time::{Duration, Instant};

use super::{ReceiverProbe, ReceiverRuntime};
use crate::tui::receiver::policy;

#[allow(dead_code)] // This focused module exists only for BR-18 legacy cleanup.
impl ReceiverRuntime {
    pub(crate) fn note_panel_sample(&mut self, now: Instant, digest: Option<u64>) {
        if !self.remote_turn_in_flight() {
            self.panel_activity = None;
            return;
        }
        if self
            .panel_sampled_at
            .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(2))
        {
            return;
        }
        self.panel_sampled_at = Some(now);
        let Some(digest) = digest else {
            return;
        };
        match self.panel_activity {
            Some((previous, _)) if previous == digest => {}
            _ => self.panel_activity = Some((digest, now)),
        }
    }

    #[must_use]
    pub(crate) fn panel_sample_due(&self, now: Instant) -> bool {
        self.remote_turn_in_flight()
            && self
                .panel_sampled_at
                .is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_secs(2))
    }

    #[must_use]
    pub(crate) fn last_panel_change(&self) -> Option<Instant> {
        self.panel_activity.map(|(_, changed)| changed)
    }

    #[must_use]
    pub(crate) fn should_abandon_turn(&self, now: Instant) -> bool {
        policy::abandons_stalled_turn(self.started, self.last_panel_change(), now)
    }

    #[must_use]
    pub(crate) fn take_due_probe(&mut self, now: Instant) -> Option<ReceiverProbe> {
        let (due, fired) = self.probe?;
        if now < due {
            return None;
        }
        let started = self.started?;
        let event = ReceiverProbe {
            elapsed_seconds: now.saturating_duration_since(started).as_secs(),
            response_id: self.receiver_response_id.clone(),
        };
        let next = fired.saturating_add(1);
        self.probe = policy::next_probe(next, started).map(|due| (due, next));
        Some(event)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) const fn has_scheduled_probe(&self) -> bool {
        self.probe.is_some()
    }
}

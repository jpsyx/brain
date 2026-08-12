//! Panel sampling for messages that are dispatched but never answered.
//!
//! A prompt left sitting unsubmitted in the composer and a genuinely slow tool
//! call are the same thing from brain's side: no completion artifact yet. Only
//! the panel itself can tell them apart, and only while the turn is still open,
//! so a dispatched message is sampled on a schedule and the screen is written
//! to the log.

use crate::tui::*;

impl App<'_> {
    /// The last non-blank `lines` rows the panel is showing, flattened for the log.
    pub(crate) fn panel_tail(&self, lines: usize) -> Option<String> {
        let screen = self.brain.as_ref()?.snapshot().ok()?;
        let tail = screen
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        let start = tail.len().saturating_sub(lines);
        Some(tail[start..].join(" ⏎ "))
    }

    /// Begin sampling the message just dispatched.
    pub(super) fn schedule_receiver_probes(&mut self, dispatched_at: std::time::Instant) {
        self.receiver_probe =
            crate::tui::receiver_state::next_probe(0, dispatched_at).map(|due| (due, 0));
    }

    /// Log the panel once each scheduled sample comes due.
    pub(super) fn probe_dispatched_receiver_message(&mut self) {
        let Some((due, fired)) = self.receiver_probe else {
            return;
        };
        let now = std::time::Instant::now();
        if now < due {
            return;
        }
        let elapsed = self
            .receiver_started
            .map_or(0, |started| started.elapsed().as_secs());
        crate::logging::log(format!(
            "receiver probe {}s after dispatch: turn_active={} awaiting_response_for={:?} panel={}",
            elapsed,
            self.brain_turn_active,
            self.receiver_session_id.as_deref().unwrap_or("<none>"),
            self.panel_tail(14).unwrap_or_else(|| "<no panel>".to_owned())
        ));
        let next = fired.saturating_add(1);
        self.receiver_probe = self
            .receiver_started
            .and_then(|started| crate::tui::receiver_state::next_probe(next, started))
            .map(|due| (due, next));
    }
}

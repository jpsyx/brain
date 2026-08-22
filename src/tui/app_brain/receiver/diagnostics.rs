//! Panel sampling for messages that are dispatched but never answered.
//!
//! A prompt left sitting unsubmitted in the composer and a genuinely slow tool
//! call are the same thing from brain's side: no completion artifact yet. Only
//! the panel itself can tell them apart, and only while the turn is still open,
//! so a dispatched message is sampled on a schedule and the screen is written
//! to the log.

use crate::tui::App;

/// A cheap content fingerprint of the rendered screen.
fn screen_digest(screen: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    screen.hash(&mut hasher);
    hasher.finish()
}

impl App {
    /// The last non-blank `lines` rows the panel is showing, flattened for the log.
    pub(crate) fn panel_tail(&self, lines: usize) -> Option<String> {
        let screen = self.brain.main_controller()?.snapshot().ok()?;
        let tail = screen
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        let start = tail.len().saturating_sub(lines);
        Some(tail[start..].join(" ⏎ "))
    }

    /// Note whether the panel is still rendering work.
    ///
    /// This is the frontend-neutral "is it busy" signal: Claude, Codex, and
    /// OpenCode all draw into the same PTY, and all three render something
    /// while a turn runs, so a screen that stops changing means the agent has
    /// stopped working rather than that it is slow. Sampled on a coarse
    /// interval because rendering the screen is not free at the event loop's
    /// rate.
    pub(crate) fn sample_panel_activity(&mut self, now: std::time::Instant) {
        let digest = self
            .brain
            .main_controller()
            .and_then(|controller| controller.snapshot().ok())
            .map(|screen| screen_digest(&screen));
        self.receiver.note_panel_sample(now, digest);
    }

    /// When the panel last rendered something different, if it is being watched.
    pub(crate) fn last_panel_change(&self) -> Option<std::time::Instant> {
        self.receiver.last_panel_change()
    }

    /// Log the panel once each scheduled sample comes due.
    pub(super) fn log_receiver_activity_probe(&self, probe: &crate::tui::receiver::ReceiverProbe) {
        crate::logging::log(format!(
            "receiver probe {}s after dispatch: turn_active={} awaiting_response_for={:?} panel={}",
            probe.elapsed_seconds,
            self.brain.turn_active(),
            probe.response_id.as_deref().unwrap_or("<none>"),
            self.panel_tail(14)
                .unwrap_or_else(|| "<no panel>".to_owned())
        ));
    }
}

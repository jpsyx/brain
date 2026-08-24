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
}

//! Panel sampling for messages that are dispatched but never answered.
//!
//! A prompt left sitting unsubmitted in the composer and a genuinely slow tool
//! call are the same thing from brain's side: no completion artifact yet. Only
//! the panel itself can tell them apart, and only while the turn is still open,
//! so a dispatched message is sampled on a schedule and the screen is written
//! to the log.

use crate::tui::*;

/// How often the panel is rendered for the activity check. Coarse on purpose:
/// the event loop ticks many times a second and every frontend's progress
/// rendering is far slower than this.
const ACTIVITY_SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

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
        let screen = self.brain.as_ref()?.snapshot().ok()?;
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
        if self.receiver_started.is_none() {
            self.receiver_panel_activity = None;
            return;
        }
        if self
            .receiver_panel_sampled_at
            .is_some_and(|last| now.saturating_duration_since(last) < ACTIVITY_SAMPLE_INTERVAL)
        {
            return;
        }
        self.receiver_panel_sampled_at = Some(now);
        let Some(digest) = self
            .brain
            .as_ref()
            .and_then(|controller| controller.snapshot().ok())
            .map(|screen| screen_digest(&screen))
        else {
            return;
        };
        match self.receiver_panel_activity {
            Some((previous, _)) if previous == digest => {}
            _ => self.receiver_panel_activity = Some((digest, now)),
        }
    }

    /// When the panel last rendered something different, if it is being watched.
    pub(crate) fn last_panel_change(&self) -> Option<std::time::Instant> {
        self.receiver_panel_activity.map(|(_, changed)| changed)
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
            self.panel_tail(14)
                .unwrap_or_else(|| "<no panel>".to_owned())
        ));
        let next = fired.saturating_add(1);
        self.receiver_probe = self
            .receiver_started
            .and_then(|started| crate::tui::receiver_state::next_probe(next, started))
            .map(|due| (due, next));
    }
}

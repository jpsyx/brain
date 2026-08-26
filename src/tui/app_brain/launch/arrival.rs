//! What to do when the brain panel's agent has exited.
//!
//! A frontend that refuses a resume prints why and quits immediately, so the
//! user is left staring at a dead panel with no way to reach their brain. Brain
//! can't enumerate every reason a frontend might refuse — it has already found
//! three — so rather than teach the resume queue one more rule, an agent that
//! dies on arrival is treated as a refusal: blocklist that id for this run and
//! open a fresh session instead.

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// How long a resumed agent has to survive before we believe it actually
/// resumed. A refusal exits in well under a second; a conversation the user
/// deliberately ended this fast loses nothing by reopening fresh.
const ARRIVAL_GRACE: Duration = Duration::from_secs(5);

/// Everything the panel remembers about resumes that didn't take: the launch
/// currently inside its arrival window, and every id already known to refuse.
#[derive(Debug, Default)]
pub(crate) struct ResumeRefusals {
    arrival: Option<(String, Instant)>,
    refused: HashSet<String>,
    retried: bool,
}

impl ResumeRefusals {
    /// Start the clock on a resumed launch.
    pub(crate) fn arm(&mut self, session_id: String) {
        self.arrival = Some((session_id, Instant::now()));
    }

    pub(crate) fn disarm(&mut self) {
        self.arrival = None;
    }

    /// The resumed session and how long it has been running, while armed.
    #[must_use]
    pub(crate) fn arrival(&self) -> Option<(&str, Duration)> {
        self.arrival
            .as_ref()
            .map(|(id, started)| (id.as_str(), started.elapsed()))
    }

    /// Never offer this id again for the rest of the run.
    pub(crate) fn refuse(&mut self, session_id: String) {
        self.refused.insert(session_id);
    }

    /// Claim this run's single relaunch. Relaunching costs a capability render
    /// and a frontend probe, and a panel that dies twice in a row is telling us
    /// about the environment rather than about one stale session id.
    pub(crate) fn claim_retry(&mut self) -> bool {
        !std::mem::replace(&mut self.retried, true)
    }

    #[must_use]
    pub(crate) fn was_refused(&self, session_id: &str) -> bool {
        self.refused.contains(session_id)
    }
}

/// The outcome for a panel whose agent is no longer running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExitedPanel {
    /// The session ended; hand the screen back to the tasks view.
    Close,
    /// The frontend refused this resume and quit before the conversation could
    /// start. Never offer the id again this run, and open a fresh panel.
    RetryFresh { refused: String },
}

/// Decide from the launch that produced the dead panel. `resumed` carries the
/// resumed session id and how long ago it launched; a fresh launch passes
/// `None` and always closes, which is what stops a refusal from looping.
#[must_use]
pub(crate) fn decide_exited_panel(resumed: Option<(&str, Duration)>) -> ExitedPanel {
    match resumed {
        Some((session_id, since_launch)) if since_launch < ARRIVAL_GRACE => ExitedPanel::RetryFresh {
            refused: session_id.to_owned(),
        },
        _ => ExitedPanel::Close,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resumed_agent_that_dies_on_arrival_is_a_refusal_to_retry_fresh() {
        assert_eq!(
            decide_exited_panel(Some(("held-session", Duration::from_millis(200)))),
            ExitedPanel::RetryFresh {
                refused: "held-session".to_owned()
            }
        );
    }

    #[test]
    fn a_resumed_conversation_that_ran_for_a_while_just_closes() {
        assert_eq!(
            decide_exited_panel(Some(("worked-fine", Duration::from_secs(600)))),
            ExitedPanel::Close
        );
    }

    #[test]
    fn only_one_relaunch_is_available_per_run() {
        let mut refusals = ResumeRefusals::default();
        assert!(refusals.claim_retry(), "the first refusal recovers");
        assert!(
            !refusals.claim_retry(),
            "a panel dying twice is the environment, not one stale id"
        );
    }

    #[test]
    fn a_refused_id_stays_refused_for_the_rest_of_the_run() {
        let mut refusals = ResumeRefusals::default();
        assert!(!refusals.was_refused("held"));
        refusals.refuse("held".to_owned());
        assert!(refusals.was_refused("held"));
        assert!(!refusals.was_refused("some-other-session"));
    }

    #[test]
    fn disarming_ends_the_arrival_window() {
        let mut refusals = ResumeRefusals::default();
        refusals.arm("resumed".to_owned());
        assert_eq!(refusals.arrival().map(|(id, _)| id), Some("resumed"));
        refusals.disarm();
        assert!(refusals.arrival().is_none());
    }

    #[test]
    fn a_fresh_session_never_retries_so_a_refusal_cannot_loop() {
        assert_eq!(decide_exited_panel(None), ExitedPanel::Close);
    }
}

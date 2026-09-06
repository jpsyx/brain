//! Startup health and refused native IDs shared by manual sessions.

use std::collections::HashSet;
use std::time::{Duration, Instant};

const ARRIVAL_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub(crate) enum SessionStartup {
    #[default]
    Established,
    Starting(Instant),
    Failed,
}

impl SessionStartup {
    pub(crate) const fn starting(now: Instant) -> Self {
        Self::Starting(now)
    }

    /// A dead child never establishes itself merely because observation was late.
    pub(crate) fn observe(
        &mut self,
        alive: bool,
        resumed: Option<&str>,
        now: Instant,
    ) -> Option<ExitedPanel> {
        match *self {
            Self::Starting(started) if alive => {
                if now.saturating_duration_since(started) >= ARRIVAL_GRACE {
                    *self = Self::Established;
                }
                None
            }
            Self::Starting(_) => {
                *self = Self::Failed;
                Some(
                    resumed.map_or(ExitedPanel::StartupFailed, |id| ExitedPanel::RetryFresh {
                        refused: id.to_owned(),
                    }),
                )
            }
            Self::Established if !alive => Some(ExitedPanel::Close),
            Self::Established | Self::Failed => None,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ResumeRefusals {
    startup: SessionStartup,
    resumed: Option<String>,
    refused: HashSet<String>,
}

impl ResumeRefusals {
    pub(crate) fn arm(&mut self, session_id: Option<String>, now: Instant) {
        self.resumed = session_id;
        self.startup = SessionStartup::starting(now);
    }

    pub(crate) fn disarm(&mut self) {
        self.resumed = None;
        self.startup = SessionStartup::Established;
    }

    pub(crate) fn observe(&mut self, alive: bool, now: Instant) -> Option<ExitedPanel> {
        self.startup.observe(alive, self.resumed.as_deref(), now)
    }

    pub(crate) fn refuse(&mut self, session_id: String) {
        self.refused.insert(session_id);
    }

    #[must_use]
    pub(crate) fn was_refused(&self, session_id: &str) -> bool {
        self.refused.contains(session_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExitedPanel {
    /// An established conversation ended. Additional closes; Main relaunches.
    Close,
    RetryFresh {
        refused: String,
    },
    /// A fresh generation never established. Keep its saved identity unavailable.
    StartupFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resumed_agent_that_dies_on_arrival_is_a_refusal_to_retry_fresh() {
        let now = Instant::now();
        let mut startup = SessionStartup::starting(now);
        assert_eq!(
            startup.observe(false, Some("held-session"), now),
            Some(ExitedPanel::RetryFresh {
                refused: "held-session".to_owned()
            })
        );
        assert_eq!(startup.observe(false, Some("held-session"), now), None);
    }

    #[test]
    fn only_an_observed_healthy_generation_has_a_normal_exit() {
        let now = Instant::now();
        for resumed in [None, Some("worked-fine")] {
            let mut startup = SessionStartup::starting(now);
            assert_eq!(
                startup.observe(true, resumed, now + Duration::from_secs(6)),
                None
            );
            assert_eq!(
                startup.observe(false, resumed, now + Duration::from_secs(7)),
                Some(ExitedPanel::Close)
            );
        }
    }

    #[test]
    fn delayed_observation_does_not_turn_a_dead_startup_into_a_normal_exit() {
        let now = Instant::now();
        let mut startup = SessionStartup::starting(now);
        assert_eq!(
            startup.observe(false, None, now + Duration::from_secs(60)),
            Some(ExitedPanel::StartupFailed)
        );
        assert_eq!(
            startup.observe(false, None, now + Duration::from_secs(120)),
            None
        );
    }

    #[test]
    fn a_briefly_live_generation_is_not_yet_established() {
        let now = Instant::now();
        let mut startup = SessionStartup::starting(now);
        assert_eq!(
            startup.observe(true, None, now + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            startup.observe(false, None, now + Duration::from_secs(2)),
            Some(ExitedPanel::StartupFailed)
        );
    }

    #[test]
    fn a_refused_id_stays_refused_for_the_rest_of_the_run() {
        let mut refusals = ResumeRefusals::default();
        refusals.refuse("held".to_owned());
        assert!(refusals.was_refused("held"));
        assert!(!refusals.was_refused("some-other-session"));
    }

    #[test]
    fn a_fresh_replacement_receives_its_own_startup_guard() {
        let now = Instant::now();
        let mut refusals = ResumeRefusals::default();
        refusals.arm(Some("refused".to_owned()), now);
        assert!(matches!(
            refusals.observe(false, now),
            Some(ExitedPanel::RetryFresh { .. })
        ));
        refusals.disarm();
        refusals.arm(None, now);
        assert_eq!(
            refusals.observe(false, now),
            Some(ExitedPanel::StartupFailed)
        );
        assert_eq!(refusals.observe(false, now), None);
    }
}

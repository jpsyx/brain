//! Skill-session tabs: the brain-panel tabs that each run one prompt in their
//! own ephemeral session and close themselves when that run is done.
//!
//! Saying "Yes" to the startup daily-triage nudge used to type `/triage` into
//! the *main* brain session, blocking it for the whole (often long, often
//! interactive) pass. Instead we spawn a dedicated, untracked session as an extra
//! tab, seed it with the session's prompt, and let the main session (`Alt+1`)
//! stay free. That is now generic: daily triage is the one builtin definition,
//! and a workspace can declare its own in the `skill_sessions` env array, each
//! getting an identical tab with identical lifecycle.
//!
//! A run is done not when the agent stops talking (it may ask questions
//! mid-pass) but when it POSTs the completion signal to the brain server, using
//! the token and route brain injected and the protocol brain appended to its
//! prompt; `tick_skill_sessions` polls those signals and auto-closes the tab
//! whose token arrives. See [`crate::skill_session`].

use crate::tui::App;

mod lifecycle;

use crate::skill_session::{SkillSessionKey, SkillSessionSpec};

/// A runnable skill session's configuration identity and command label.
pub(crate) type SkillSessionRows = Vec<(SkillSessionKey, String)>;

impl App {
    /// Every skill session this workspace offers: the builtin daily triage
    /// (only while the daily-triage check is enabled) plus the workspace's own
    /// `skill_sessions` definitions.
    pub(crate) fn available_skill_sessions(&self) -> Vec<SkillSessionSpec> {
        crate::skill_session::available(
            !self.status.daily_triage_check_disabled(),
            self.brain.configured_skill_sessions(),
        )
    }

    /// Configured skill-session starts, excluding every currently running skill.
    pub(crate) fn runnable_skill_session_rows(&self) -> SkillSessionRows {
        let available = self.available_skill_sessions();
        let running = self.brain.running_skill_session_keys();
        crate::skill_session::runnable(&available, &running)
            .into_iter()
            .map(|spec| (spec.key, spec.command_label.clone()))
            .collect()
    }
}

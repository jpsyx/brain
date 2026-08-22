#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Tasks,
    Brain,
}

/// Which session is showing inside the brain panel. The panel normally hosts a
/// single persistent session ([`BrainTab::Main`]); a [`BrainTab::Session`] is one
/// of the ephemeral *skill sessions* that appear as extra tabs only while they
/// run (see `App::skill_sessions` and [`crate::skill_session`]). Selected with
/// `Alt+1` for the main session and `Alt+<n>` for the nth skill session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrainTab {
    Main,
    Session(SessionTabId),
}

/// Identity of one open skill-session tab, unique for the life of the shell.
///
/// Tabs are addressed by this rather than by list position or by
/// [`crate::skill_session::SkillSessionKey`] so that closing one tab can never
/// silently repoint the active tab at another, and so a definition edited in
/// `skill_sessions` mid-session still resolves to the tab it opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SessionTabId(pub(crate) u32);

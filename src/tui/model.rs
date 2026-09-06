#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Tasks,
    Brain,
}

/// Which session is showing inside the brain panel. The panel normally hosts a
/// single persistent session ([`BrainTab::Main`]); a [`BrainTab::Session`] is one
/// additional manual, skill-session, or receiver-run tab. Selected with `Alt+1` for the
/// main session and `Alt+<n>` for the nth additional tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrainTab {
    Main,
    Session(SessionTabId),
}

/// User-owned additional tabs that can offer the close-tab action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionCloseKind {
    Manual,
    Skill,
}

/// Identity of one open additional tab, unique for the life of the shell.
///
/// Tabs are addressed by this rather than by list position or by
/// [`crate::skill_session::SkillSessionKey`] or a receiver job so that closing
/// one tab can never silently repoint the active tab at another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SessionTabId(pub(crate) u32);

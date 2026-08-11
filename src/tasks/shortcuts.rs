//! The single source of truth for every keybinding the tasks shell exposes.
//!
//! Both the compact footer (a curated subset + `?`) and the full help modal
//! (`?` → everything, grouped) render from [`ALL`]. The actual key *handling*
//! lives in `tui::handlers` / `tui::keymap`; this table is the human-facing
//! catalogue, so when you add or change a binding, update its row here and the
//! footer + help modal follow automatically.
//!
//! Keep `keys` short — it's what shows in the footer chip. `label` is the
//! one-word footer caption; `desc` is the fuller sentence the help modal uses.

/// Which surface a shortcut belongs to. Drives the grouping in the help modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Navigation,
    Views,
    TaskActions,
    Brain,
    Search,
    Global,
}

impl Group {
    /// Section heading shown in the help modal, in display order.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Navigation => "Navigation",
            Self::Views => "Views",
            Self::TaskActions => "Task actions",
            Self::Brain => "Brain panel",
            Self::Search => "Search",
            Self::Global => "Global",
        }
    }

    /// Groups in the order the help modal lists them.
    pub const ORDER: [Self; 6] = [
        Self::Navigation,
        Self::Views,
        Self::TaskActions,
        Self::Brain,
        Self::Search,
        Self::Global,
    ];
}

/// One keybinding row.
#[derive(Debug, Clone, Copy)]
pub struct Shortcut {
    /// Display form of the key(s), e.g. `"j / k"`, `"^D"`, `"?"`.
    pub keys: &'static str,
    /// One-word caption for the compact footer.
    pub label: &'static str,
    /// Fuller description shown in the help modal.
    pub desc: &'static str,
    /// Which surface this belongs to.
    pub group: Group,
    /// Whether the compact footer shows this binding (before the ellipsis).
    pub in_footer: bool,
}

/// Every keybinding, in a stable order. The help modal renders these grouped
/// by [`Group::ORDER`]; the footer renders the `in_footer` subset.
pub const ALL: &[Shortcut] = &[
    // --- Navigation ---
    Shortcut {
        keys: "j / k",
        label: "task",
        desc: "Next / previous task (accepts a count prefix, e.g. 3j)",
        group: Group::Navigation,
        in_footer: true,
    },
    Shortcut {
        keys: "d / u",
        label: "½-page",
        desc: "Half-page down / up",
        group: Group::Navigation,
        in_footer: true,
    },
    Shortcut {
        keys: "PgDn / PgUp",
        label: "page",
        desc: "Full page down / up",
        group: Group::Navigation,
        in_footer: false,
    },
    Shortcut {
        keys: "g / G",
        label: "first/last",
        desc: "Jump to the first / last task",
        group: Group::Navigation,
        in_footer: true,
    },
    Shortcut {
        keys: "→ / ←",
        label: "notes",
        desc: "Expand / collapse the highlighted entry's notes",
        group: Group::Navigation,
        in_footer: false,
    },
    Shortcut {
        keys: "l",
        label: "notes",
        desc: "Toggle the selected entry's notes (preview ↔ full)",
        group: Group::Navigation,
        in_footer: false,
    },
    // --- Views ---
    Shortcut {
        keys: "Tab / ⇧Tab",
        label: "view",
        desc: "Cycle view forward / backward",
        group: Group::Views,
        in_footer: true,
    },
    Shortcut {
        keys: "t m p w h b a",
        label: "jump view",
        desc: "Jump to today / mit / past-due / week / habits / backlog / all",
        group: Group::Views,
        in_footer: false,
    },
    // --- Task actions ---
    Shortcut {
        keys: "↵",
        label: "actions",
        desc: "Open the task actions modal for the selected entry",
        group: Group::TaskActions,
        in_footer: true,
    },
    Shortcut {
        keys: "^D",
        label: "done",
        desc: "Mark the selected task complete (confirm modal)",
        group: Group::TaskActions,
        in_footer: true,
    },
    Shortcut {
        keys: "^⌫",
        label: "remove",
        desc: "Remove the selected task (confirm modal) — tasks only",
        group: Group::TaskActions,
        in_footer: false,
    },
    Shortcut {
        keys: "^O",
        label: "links",
        desc: "Open the selected entry's links (Linear + notes URLs)",
        group: Group::TaskActions,
        in_footer: true,
    },
    Shortcut {
        keys: "r",
        label: "refresh",
        desc: "Reload tasks.csv + habits.csv from disk",
        group: Group::TaskActions,
        in_footer: false,
    },
    // --- Brain ---
    Shortcut {
        keys: "^M",
        label: "brain",
        desc: "Open / focus the brain panel (resumes your latest session)",
        group: Group::Brain,
        in_footer: true,
    },
    Shortcut {
        keys: "^⇧M",
        label: "brain·task",
        desc: "Message brain about the selected task (hold Shift; needs kitty protocol)",
        group: Group::Brain,
        in_footer: false,
    },
    Shortcut {
        keys: "^X",
        label: "close brain",
        desc: "Close the brain panel and end its agent session (on a skill-session tab, closes only that tab)",
        group: Group::Brain,
        in_footer: false,
    },
    Shortcut {
        keys: "^N",
        label: "new session",
        desc: "Start a new agent session in the brain panel (types /new and submits it)",
        group: Group::Brain,
        in_footer: false,
    },
    Shortcut {
        keys: "Alt+H / Alt+L",
        label: "switch",
        desc: "Focus the tasks / brain panel (Alt+H always returns to tasks)",
        group: Group::Brain,
        in_footer: false,
    },
    Shortcut {
        keys: "Alt+[ / Alt+]",
        label: "brain tab",
        desc: "Cycle the brain-panel tab (main session ↔ each open skill session, e.g. daily triage), only while a skill session is running. Reliable everywhere; the command palette also carries 'Show main brain session' and a 'Show <title> session' row per open tab. Alt+1 selects the main session and Alt+<n> the nth skill session on terminals that support Alt+digit",
        group: Group::Brain,
        in_footer: false,
    },
    Shortcut {
        keys: "Alt+U / Alt+D",
        label: "scroll",
        desc: "Scroll the focused panel a half-page up / down (fires while typing or in the brain panel)",
        group: Group::Brain,
        in_footer: false,
    },
    Shortcut {
        keys: "^A",
        label: "agenda",
        desc: "Open today's agenda (offers to generate it when missing)",
        group: Group::Brain,
        in_footer: false,
    },
    // (Open habits page moved to the command palette — "Open habits page".)
    // --- Search ---
    Shortcut {
        keys: "/",
        label: "search",
        desc: "Enter search mode (live fuzzy filter)",
        group: Group::Search,
        in_footer: true,
    },
    Shortcut {
        keys: "Esc",
        label: "clear",
        desc: "Clear the active filter (quits when none is set)",
        group: Group::Search,
        in_footer: false,
    },
    // --- Global ---
    Shortcut {
        keys: "^L / ^H",
        label: "cycle view",
        desc: "Cycle the main view right / left (tasks ↔ brain directory). Distinct from Alt+H/L, which move panel focus",
        group: Group::Global,
        in_footer: false,
    },
    Shortcut {
        keys: "^T / ^B",
        label: "jump view",
        desc: "Jump to the tasks / brain-directory main view",
        group: Group::Global,
        in_footer: false,
    },
    Shortcut {
        keys: "^P",
        label: "palette",
        desc: "Open the global command palette (including Enable / Disable receiver)",
        group: Group::Global,
        in_footer: true,
    },
    Shortcut {
        keys: "Alt+S",
        label: "help",
        desc: "Show all keyboard shortcuts",
        group: Group::Global,
        in_footer: false,
    },
    Shortcut {
        keys: "q",
        label: "quit",
        desc: "Quit the shell (also Ctrl+C) — tasks view normal mode only",
        group: Group::Global,
        in_footer: true,
    },
    Shortcut {
        keys: "^Q",
        label: "quit",
        desc: "Unconditional quit from either panel — works while the brain panel is focused or a modal is open",
        group: Group::Global,
        in_footer: false,
    },
];

/// The curated subset rendered in the compact footer (those flagged
/// `in_footer`), in table order.
#[must_use]
pub fn footer_subset() -> Vec<&'static Shortcut> {
    ALL.iter().filter(|s| s.in_footer).collect()
}

/// Shortcuts belonging to `group`, in table order. Used by the help modal.
#[must_use]
pub fn in_group(group: Group) -> Vec<&'static Shortcut> {
    ALL.iter().filter(|s| s.group == group).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_subset_is_nonempty_and_all_flagged() {
        let subset = footer_subset();
        assert!(!subset.is_empty());
        assert!(subset.iter().all(|s| s.in_footer));
    }

    #[test]
    fn every_shortcut_lands_in_exactly_one_ordered_group() {
        // Each row's group is one of the ORDER groups, and the grouped views
        // partition ALL (no row lost, none double-counted).
        let total: usize = Group::ORDER.iter().map(|g| in_group(*g).len()).sum();
        assert_eq!(total, ALL.len());
    }

    #[test]
    fn help_lists_the_brain_close_shortcut() {
        let brain = in_group(Group::Brain);
        assert!(
            brain
                .iter()
                .any(|s| s.keys == "^X" && s.desc.contains("session"))
        );
    }

    #[test]
    fn help_lists_the_new_session_shortcut() {
        let brain = in_group(Group::Brain);
        assert!(
            brain
                .iter()
                .any(|s| s.keys == "^N" && s.desc.contains("/new"))
        );
    }

    #[test]
    fn help_lists_the_brain_tab_switch_shortcut() {
        let brain = in_group(Group::Brain);
        assert!(
            brain
                .iter()
                .any(|s| s.keys == "Alt+[ / Alt+]" && s.desc.contains("skill session"))
        );
    }

    #[test]
    fn help_routes_receiver_enablement_through_the_palette() {
        assert!(ALL.iter().any(|shortcut| {
            shortcut.keys == "^P" && shortcut.desc.contains("Disable receiver")
        }));
    }

    #[test]
    fn help_is_advertised_as_alt_s_not_a_bare_key() {
        assert!(
            ALL.iter()
                .any(|s| s.keys == "Alt+S" && s.desc.contains("shortcuts"))
        );
        assert!(!ALL.iter().any(|s| s.keys == "?" || s.keys == "Alt+?"));
    }

    #[test]
    fn help_lists_the_main_view_switch_shortcuts() {
        assert!(
            ALL.iter()
                .any(|s| s.keys == "^L / ^H" && s.desc.contains("main view"))
        );
        assert!(
            ALL.iter()
                .any(|s| s.keys == "^T / ^B" && s.desc.contains("main view"))
        );
    }

    #[test]
    fn habits_browser_shortcut_is_no_longer_a_binding() {
        // Ctrl+H is now the cycle-view accelerator; opening the habits page
        // moved to the command palette, so no ^H habits row remains.
        assert!(!ALL.iter().any(|s| s.keys == "^H"));
    }
}

//! The declared config-variable schema: the list `config list` prints, the
//! set `config set` accepts, and the built-in defaults. `Resolved` pairs a
//! variable with its effective value.

/// One declared config variable: shown in `config list`, accepted by
/// `config set`, and resolved with an optional built-in default.
pub(super) struct VarSpec {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) default: Option<&'static str>,
}

/// The full schema, in the order `config list` prints them.
pub(super) const VARS: [VarSpec; 9] = [
    VarSpec {
        name: "root",
        description: "Path to your brain (PARA) directory. Tilde-expanded. Defaults to ~/brain.",
        default: Some("~/brain"),
    },
    VarSpec {
        name: "linear_workspace",
        description: "Linear workspace slug (e.g. acme). Builds https://linear.app/<slug>/issue/ for the open-link action.",
        default: None,
    },
    VarSpec {
        name: "markdown_to_pdf_path",
        description: "Path to the markdown-to-pdf command. Auto-discovered on first run; required for the Create-PDF action.",
        default: None,
    },
    VarSpec {
        name: "daily_triage_name_pattern",
        description: "Case-insensitive regex matched against habit names to gate the startup triage nudge. Empty disables it.",
        default: Some("Morning Triage"),
    },
    VarSpec {
        name: "day_rollover_hour",
        description: "Local hour (0-23) at which the logical day rolls over for the triage re-check.",
        default: Some("6"),
    },
    VarSpec {
        name: "agenda_dir",
        description: "Directory the generated daily-agenda PDF is written to. Tilde-expanded. Defaults to ~/Downloads.",
        default: Some("~/Downloads"),
    },
    VarSpec {
        name: "calendar_id",
        description: "Calendar to pull busy blocks from when building the agenda (e.g. a Google Calendar id/email). Empty disables calendar-aware scheduling.",
        default: None,
    },
    VarSpec {
        name: "claude_cmd",
        description: "Command that launches the brain panel's claude session; --resume/--session-id are appended. Defaults to `claude --dangerously-skip-permissions`.",
        default: Some("claude --dangerously-skip-permissions"),
    },
    VarSpec {
        name: "skills_auto_sync",
        description: "When true, config/personalize changes re-render and install the bundled skills into the agent registry. Default true; set false to manage the registry only via explicit `brain skills sync`.",
        default: Some("true"),
    },
];

/// A variable paired with its effective value (explicit override, else the
/// built-in default, else `None`).
pub struct Resolved {
    pub name: &'static str,
    pub value: Option<String>,
    pub description: &'static str,
}

pub(super) fn is_known(name: &str) -> bool {
    VARS.iter().any(|v| v.name == name)
}

pub(super) fn default_of(name: &str) -> Option<&'static str> {
    VARS.iter().find(|v| v.name == name).and_then(|v| v.default)
}

pub(super) fn known_names() -> String {
    VARS.iter()
        .map(|v| v.name)
        .collect::<Vec<_>>()
        .join(", ")
}

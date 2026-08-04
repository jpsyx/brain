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
///
/// `root` is intentionally **not** here: the brain-root location can't live
/// inside the brain root (circular), so it is resolved from `~/.config/brain-root`
/// or the `~/brain` default and edited by hand, never via `brain config`
/// (see [`crate::paths`]).
pub(super) const VARS: [VarSpec; 13] = [
    VarSpec {
        name: "access_mode",
        description: "Portable workspace access policy: unrestricted or workspace_only (advisory, not a filesystem sandbox).",
        default: Some("unrestricted"),
    },
    VarSpec {
        name: "allowed_mcps",
        description: "Portable logical MCP allowlist for workspace-only launches. Connection and credential material stays in brain env.",
        default: Some("[]"),
    },
    VarSpec {
        name: "allowed_skills",
        description: "Portable logical skill allowlist for workspace-only launches. An explicit empty list disables all skills.",
        default: Some("[\"contacts\",\"second-brain\",\"todo\",\"triage\"]"),
    },
    VarSpec {
        name: "enable_triage_habits",
        description: "When true, Brain maintains protected daily and weekly triage habit chains. Disabling purges their managed rows and derived references.",
        default: Some("true"),
    },
    VarSpec {
        name: "response_email",
        description: "Email address for long-form responses requested by SMS and authenticated brain messages.",
        default: None,
    },
    VarSpec {
        name: "allowed_sms_senders",
        description: "Comma-separated E.164 phone numbers permitted to message Brain, including + and country code (for example, +16072809118).",
        default: None,
    },
    VarSpec {
        name: "allowed_email_senders",
        description: "Comma-separated email addresses permitted to send messages to the brain.",
        default: None,
    },
    VarSpec {
        name: "linear_workspace",
        description: "Linear workspace slug (e.g. acme). Builds https://linear.app/<slug>/issue/ for the open-link action.",
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
        name: "skills_auto_sync",
        description: "When true, config/personalize changes re-render and install the bundled skills into the agent registry. Default true; set false to manage the registry only via explicit `brain skills sync`.",
        default: Some("true"),
    },
];

/// A variable paired with its effective value (explicit override, else the
/// built-in default, else `None`).
pub struct Resolved {
    pub name: String,
    pub value: Option<String>,
    pub description: String,
}

pub(super) fn is_known(name: &str) -> bool {
    VARS.iter().any(|v| v.name == name)
}

pub(super) fn default_of(name: &str) -> Option<&'static str> {
    VARS.iter().find(|v| v.name == name).and_then(|v| v.default)
}

pub(super) fn known_names() -> String {
    VARS.iter().map(|v| v.name).collect::<Vec<_>>().join(", ")
}

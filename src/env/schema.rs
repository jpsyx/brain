//! Declared brain-env rows: what `brain env list` prints and their defaults.
//! The virtual `root` row is structural and read-only; all other rows are
//! writable machine env unless a caller applies tighter validation.

/// What a sensitive value renders as: presence, never the secret itself.
pub(super) const REDACTED: &str = "(set)";

pub(super) struct VarSpec {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) default: Option<&'static str>,
    pub(super) legacy_config_fallback: bool,
}

/// Env variables scoped to the **machine**, not to one workspace.
///
/// They live in the registry's top-level `env` map, so every registered
/// workspace reads and writes the same value. The test is whether the value
/// could sensibly differ between two workspaces on one machine: the path to a
/// binary cannot, and neither can the public receiver origin, since one machine
/// serves one URL per channel and providers sign the literal URL.
pub(crate) const MACHINE_GLOBAL_VARS: [&str; 2] =
    ["markdown_to_pdf_path", "brain_receiver_public_url"];

/// Whether `name` is stored once for the whole machine.
#[must_use]
pub fn is_machine_global(name: &str) -> bool {
    MACHINE_GLOBAL_VARS.contains(&name)
}

pub(super) use crate::agent::{
    DEFAULT_CLAUDE_COMMAND as DEFAULT_CLAUDE_CMD, DEFAULT_CODEX_COMMAND as DEFAULT_CODEX_CMD,
    DEFAULT_OPENCODE_COMMAND as DEFAULT_OPENCODE_CMD,
};

/// The declared scalar brain-env schema, in `brain env list` order. Nested
/// values from the raw env object are listed after these rows.
pub(super) const VARS: [VarSpec; 15] = [
    VarSpec {
        name: "root",
        description: "Selected workspace root on THIS machine (read-only structural registry field; change it through workspace management).",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "markdown_to_pdf_path",
        description: "Path to the markdown-to-pdf command on THIS machine. Machine-global: every registered workspace shares one value. Auto-discovered on first run; required for the Create-PDF action.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "claude_cmd",
        description: "Command used to launch Claude for the brain panel on THIS machine. Defaults to `claude --dangerously-skip-permissions`; brain appends Claude resume args.",
        default: Some(DEFAULT_CLAUDE_CMD),
        legacy_config_fallback: true,
    },
    VarSpec {
        name: "codex_cmd",
        description: "Command used to launch Codex for the brain panel on THIS machine. Defaults to codex; brain appends Codex resume args when resuming.",
        default: Some(DEFAULT_CODEX_CMD),
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "opencode_cmd",
        description: "Command used to launch OpenCode for the brain panel on THIS machine. Defaults to opencode; Brain appends the Brain agent and session arguments.",
        default: Some(DEFAULT_OPENCODE_CMD),
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "default_agent_frontend",
        description: "Frontend the brain panel launches on THIS machine when no --claude/--codex/--open-code flag is passed. One of claude, codex, opencode. Defaults to claude.",
        default: Some(crate::agent::default_frontend::DEFAULT),
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "agent_capabilities",
        description: "Selected workspace's machine-local MCP connections, executable paths, skill paths, and credentials. Portable config stores only logical allowlists.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "skill_sessions",
        description: "Skill sessions this machine offers in the tasks-view command palette: a JSON array of {title, prompt, command_label}. Each runs its prompt in its own brain-panel tab and closes when the run signals completion. Daily triage is builtin and is not listed here.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "brain_receiver_public_url",
        description: "This machine's public receiver base URL. Machine-global: every registered workspace shares one origin, and brain serves one `/sms` and one `/email` URL under it, routing each message by the number or address it arrived at. Requirement status reports only whether it is present and never prints it.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "twilio_account_sid",
        description: "Machine-local Twilio Account SID used for SMS delivery and attachments; status output redacts its value.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "twilio_auth_token",
        description: "Machine-local Twilio Auth Token used to authenticate SMS webhooks and delivery.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "twilio_from_number",
        description: "Machine-local Twilio number used as the sender for outbound SMS. Requirement status reports presence only; `brain receiver phone` and `brain receiver` print it on request.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "resend_api_key",
        description: "Machine-local Resend API key used for inbound email retrieval and outbound delivery.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "resend_from_email",
        description: "Machine-local verified Resend sender address. Requirement status reports presence only; `brain receiver email` and `brain receiver` print it on request.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "resend_webhook_signing_secret",
        description: "Machine-local Resend webhook signing secret used to authenticate inbound email.",
        default: None,
        legacy_config_fallback: false,
    },
];

/// Every declared per-workspace variable with its description, in schema order.
pub(super) fn declared_docs() -> impl Iterator<Item = (&'static str, &'static str)> {
    VARS.iter().map(|spec| (spec.name, spec.description))
}

/// The declared description for `name`, if brain declares one.
pub(super) fn declared_description(name: &str) -> Option<&'static str> {
    VARS.iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.description)
}

pub(super) fn is_known(name: &str) -> bool {
    VARS.iter().any(|v| v.name == name) && !is_structural(name)
}

pub(super) fn is_structural(name: &str) -> bool {
    matches!(
        name.split('.').next().unwrap_or_default(),
        "root"
            | "workspace_id"
            | "workspace_name"
            | "canonical_name"
            | "aliases"
            | "local_user_id"
            | "receiver_enabled"
            | "access_mode"
            | "access_policy"
            | "schema_version"
            | "default_workspace"
            | "workspaces"
            | "env"
            | "receiver_ingress_id"
            | "minimum_brain_version"
    )
}

/// Whether a name (or dotted path) holds a credential.
///
/// Env output reports a sensitive value only as present. Identifiers such as
/// `twilio_account_sid` and `sync.b2_key_id` are deliberately visible: a user
/// needs them to confirm which account and bucket a workspace points at.
#[must_use]
pub fn is_sensitive(name: &str) -> bool {
    matches!(
        name,
        "twilio_auth_token"
            | "resend_api_key"
            | "resend_webhook_signing_secret"
            | "sync.b2_app_key"
            | "sync.crypt_password"
            | "sync.crypt_password2"
    ) || name == "agent_capabilities"
        || (name.starts_with("agent_capabilities.mcps.") && name.contains(".credentials."))
}

#[cfg(test)]
pub(super) fn default_of(name: &str) -> Option<&'static str> {
    VARS.iter().find(|v| v.name == name).and_then(|v| v.default)
}

pub(super) fn known_names() -> String {
    VARS.iter()
        .map(|v| v.name)
        .filter(|name| !is_structural(name))
        .collect::<Vec<_>>()
        .join(", ")
}

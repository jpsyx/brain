//! Declared brain-env rows: what `brain env list` prints and their defaults.
//! The virtual `root` row is structural and read-only; all other rows are
//! writable machine env unless a caller applies tighter validation.

pub(super) struct VarSpec {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) default: Option<&'static str>,
    pub(super) legacy_config_fallback: bool,
}

pub(super) use crate::agent::{
    DEFAULT_CLAUDE_COMMAND as DEFAULT_CLAUDE_CMD, DEFAULT_CODEX_COMMAND as DEFAULT_CODEX_CMD,
    DEFAULT_OPENCODE_COMMAND as DEFAULT_OPENCODE_CMD,
};

/// The declared scalar brain-env schema, in `brain env list` order. Nested
/// values from the raw env object are listed after these rows.
pub(super) const VARS: [VarSpec; 13] = [
    VarSpec {
        name: "root",
        description: "Selected workspace root on THIS machine (read-only structural registry field; change it through workspace management).",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "markdown_to_pdf_path",
        description: "Path to the markdown-to-pdf command on THIS machine. Auto-discovered on first run; required for the Create-PDF action.",
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
        description: "Command reserved for the OpenCode brain-panel stub on THIS machine. Defaults to opencode; Brain does not execute it yet.",
        default: Some(DEFAULT_OPENCODE_CMD),
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "agent_capabilities",
        description: "Selected workspace's machine-local MCP connections, executable paths, skill paths, and credentials. Portable config stores only logical allowlists.",
        default: None,
        legacy_config_fallback: false,
    },
    VarSpec {
        name: "brain_receiver_public_url",
        description: "Selected workspace's machine-local public receiver base URL. Requirement status reports only whether it is present and never prints it.",
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
        description: "Machine-local Twilio number used as the sender for outbound SMS; status output never prints the address.",
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
        description: "Machine-local verified Resend sender address; status output never prints the address.",
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

#[must_use]
pub fn is_sensitive(name: &str) -> bool {
    matches!(
        name,
        "twilio_auth_token" | "resend_api_key" | "resend_webhook_signing_secret"
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

//! Declared brain-env variables: what `brain env list` prints, what
//! `brain env set` accepts, and their defaults.

pub(super) struct VarSpec {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) default: Option<&'static str>,
}

/// Default Codex launch command for the brain panel.
pub(super) const DEFAULT_CODEX_CMD: &str = "codex";

/// Default Claude launch command for the brain panel.
pub(super) const DEFAULT_CLAUDE_CMD: &str = "claude --dangerously-skip-permissions";

/// The declared scalar brain-env schema, in `brain env list` order. Nested
/// values from the raw env object are listed after these rows.
pub(super) const VARS: [VarSpec; 11] = [
    VarSpec {
        name: "root",
        description: "Absolute or ~-relative path to the brain (PARA) directory on THIS machine. Defaults to ~/brain; a legacy ~/.config/brain-root pointer is migrated into this key.",
        default: Some("~/brain"),
    },
    VarSpec {
        name: "markdown_to_pdf_path",
        description: "Path to the markdown-to-pdf command on THIS machine. Auto-discovered on first run; required for the Create-PDF action.",
        default: None,
    },
    VarSpec {
        name: "claude_cmd",
        description: "Command used to launch Claude for the brain panel on THIS machine. Defaults to `claude --dangerously-skip-permissions`; brain appends Claude resume args.",
        default: Some(DEFAULT_CLAUDE_CMD),
    },
    VarSpec {
        name: "codex_cmd",
        description: "Command used to launch Codex for the brain panel on THIS machine. Defaults to codex; brain appends Codex resume args when resuming.",
        default: Some(DEFAULT_CODEX_CMD),
    },
    VarSpec {
        name: "brain_receiver_public_url",
        description: "Public base URL for the receiver; /sms and /email webhook paths are derived from it.",
        default: None,
    },
    VarSpec {
        name: "twilio_account_sid",
        description: "Machine-local Twilio Account SID used for SMS delivery and attachments.",
        default: None,
    },
    VarSpec {
        name: "twilio_auth_token",
        description: "Machine-local Twilio Auth Token used to authenticate SMS webhooks and delivery.",
        default: None,
    },
    VarSpec {
        name: "twilio_from_number",
        description: "Machine-local Twilio number used as the sender for outbound SMS.",
        default: None,
    },
    VarSpec {
        name: "resend_api_key",
        description: "Machine-local Resend API key used for inbound email retrieval and outbound delivery.",
        default: None,
    },
    VarSpec {
        name: "resend_from_email",
        description: "Machine-local verified Resend sender address.",
        default: None,
    },
    VarSpec {
        name: "resend_webhook_signing_secret",
        description: "Machine-local Resend webhook signing secret used to authenticate inbound email.",
        default: None,
    },
];

pub(super) fn is_known(name: &str) -> bool {
    VARS.iter().any(|v| v.name == name)
}

pub(super) fn is_sensitive(name: &str) -> bool {
    matches!(
        name,
        "twilio_auth_token"
            | "resend_api_key"
            | "resend_webhook_signing_secret"
    )
}

pub(super) fn default_of(name: &str) -> Option<&'static str> {
    VARS.iter().find(|v| v.name == name).and_then(|v| v.default)
}

pub(super) fn known_names() -> String {
    VARS.iter().map(|v| v.name).collect::<Vec<_>>().join(", ")
}

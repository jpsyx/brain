//! Typed view of the runtime knobs the tasks shell reads.
//!
//! The values live in the shared JSON store owned by [`crate::settings`]
//! (`~/.config/brain/config.json`). This struct deserializes just the fields
//! the tasks shell cares about; unknown keys (e.g. `root`,
//! `markdown_to_pdf_path`) are ignored here and read where they belong.
//! Missing file or missing fields fall back to [`Config::default`], so a blank
//! or absent config is always safe.
//!
//! Adding a new option means: (1) extend `Config`, (2) default it, (3) declare
//! it in `settings::VARS` so `brain config` can manage it, (4) bump
//! `docs/config.md`.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Portable advisory access policy for this workspace.
    pub access_mode: crate::access::AccessMode,
    /// Logical MCP server names available to workspace-only agent launches.
    pub allowed_mcps: Vec<String>,
    /// Logical skill names available to workspace-only agent launches.
    #[serde(default = "default_allowed_skills")]
    pub allowed_skills: Vec<String>,
    /// Whether Brain maintains its daily and weekly triage habit chains.
    pub enable_triage_habits: bool,
    /// Whether the shell may open the daily-triage startup nudge. Portable, so
    /// every machine on the workspace starts with the same answer; the command
    /// palette still flips it for one running session.
    #[serde(default = "enabled")]
    pub enable_daily_triage_check: bool,
    /// Legacy migration input for a portable user's response address.
    pub response_email: String,
    /// Legacy migration input for portable inbound phone mappings.
    #[serde(deserialize_with = "deserialize_sms_allowlist")]
    pub allowed_sms_senders: String,
    /// Legacy migration input for portable inbound email mappings.
    pub allowed_email_senders: String,
    /// Case-insensitive regex matched against habit *names* to find the
    /// daily-triage habit that gates the startup triage modal. Matching by
    /// name (not ID) is deliberate: the triage habit recurs, so every
    /// occurrence gets a fresh ID; a pinned ID would go stale the moment the
    /// habit rolls over. The pattern survives recurrence and tolerates
    /// suffix/capitalization drift. Empty string disables the check; an
    /// invalid regex is treated the same as empty.
    pub daily_triage_name_pattern: String,

    /// Linear workspace slug (e.g. `acme`). [`Config::linear_base_url`]
    /// interpolates it into `https://linear.app/<slug>/issue/`, to which a
    /// task's `linear_issue` identifier is appended for the `Ctrl+O` "open
    /// link" action. Empty disables Linear links.
    pub linear_workspace: String,

    /// Local hour (0-23) at which the "logical day" rolls over for the
    /// daily-triage re-check. The boundary is this hour, not midnight, so a
    /// late-night session isn't nagged for a "new day" the moment the clock
    /// passes 00:00. Defaults to 6 (6 AM); an out-of-range value falls back.
    pub day_rollover_hour: u32,
}

fn deserialize_sms_allowlist<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StoredAllowlist {
        Text(String),
        LegacyNumber(i64),
    }

    Ok(match StoredAllowlist::deserialize(deserializer)? {
        StoredAllowlist::Text(value) => value,
        StoredAllowlist::LegacyNumber(value) if value > 0 => format!("+{value}"),
        StoredAllowlist::LegacyNumber(value) => value.to_string(),
    })
}

impl Default for Config {
    fn default() -> Self {
        Self {
            access_mode: crate::access::AccessMode::Unrestricted,
            allowed_mcps: Vec::new(),
            allowed_skills: default_allowed_skills(),
            enable_triage_habits: true,
            enable_daily_triage_check: true,
            response_email: String::new(),
            allowed_sms_senders: String::new(),
            allowed_email_senders: String::new(),
            daily_triage_name_pattern: "Morning Triage".to_owned(),
            linear_workspace: String::new(),
            day_rollover_hour: 6,
        }
    }
}

const fn enabled() -> bool {
    true
}

fn default_allowed_skills() -> Vec<String> {
    ["contacts", "second-brain", "todo", "triage"]
        .map(str::to_owned)
        .to_vec()
}

impl Config {
    /// Load the typed config from the shared store. Returns defaults when the
    /// file is missing, unreadable, or shaped wrong — a misconfigured file
    /// should never block the tasks shell from opening.
    #[must_use]
    pub fn load(workspace: &crate::workspace::WorkspaceContext) -> Self {
        Self::load_from_root(workspace.root())
    }

    /// Strictly load portable config. Only a missing file yields defaults.
    pub(crate) fn try_load(workspace: &crate::workspace::WorkspaceContext) -> anyhow::Result<Self> {
        Self::try_load_from_root(workspace.root())
    }

    /// Load TUI startup state without parsing capability lists that unrestricted
    /// sessions never consume. Access mode and every live TUI field remain strict.
    pub(crate) fn try_load_for_startup(
        workspace: &crate::workspace::WorkspaceContext,
    ) -> anyhow::Result<Self> {
        Self::try_load_for_startup_from_root(workspace.root())
    }

    fn try_load_for_startup_from_root(root: &std::path::Path) -> anyhow::Result<Self> {
        let Some(mut value) = read_strict_config_value(root)? else {
            return Ok(Self::default());
        };
        let access_mode = value
            .get("access_mode")
            .cloned()
            .map_or(Ok(crate::access::AccessMode::Unrestricted), |value| {
                serde_json::from_value(value).map_err(anyhow::Error::from)
            })?;
        if access_mode == crate::access::AccessMode::Unrestricted {
            let object = value
                .as_object_mut()
                .expect("strict config reader returns an object");
            object.remove("allowed_mcps");
            object.remove("allowed_skills");
        }
        let mut config: Self = serde_json::from_value(value).map_err(anyhow::Error::from)?;
        config.access_mode = access_mode;
        Ok(config)
    }

    /// Strictly load portable config from an explicit workspace root.
    pub(crate) fn try_load_from_root(root: &std::path::Path) -> anyhow::Result<Self> {
        let Some(value) = read_strict_config_value(root)? else {
            return Ok(Self::default());
        };
        serde_json::from_value(value).map_err(anyhow::Error::from)
    }

    #[must_use]
    pub(crate) fn load_from_root(root: &std::path::Path) -> Self {
        let path = root.join(".config/config.json");
        serde_json::from_value(Value::Object(crate::settings::load_map_at(&path)))
            .unwrap_or_default()
    }

    /// Full Linear issue-URL prefix built from the configured workspace, or an
    /// empty string when no workspace is set (Linear links are then omitted).
    #[must_use]
    pub fn linear_base_url(&self) -> String {
        let ws = self.linear_workspace.trim();
        if ws.is_empty() || !self.linear_workspace_is_valid() {
            String::new()
        } else {
            format!("https://linear.app/{ws}/issue/")
        }
    }

    /// Whether the optional Linear slug is empty or safe to interpolate.
    #[must_use]
    pub(crate) fn linear_workspace_is_valid(&self) -> bool {
        let value = self.linear_workspace.trim();
        value.is_empty()
            || value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && value
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && value
                    .bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
    }

    /// Whether the startup daily-triage nudge is suppressed for this workspace.
    ///
    /// The shell's live state is phrased as "skip", so invert once here rather
    /// than at every call site.
    #[must_use]
    pub const fn skip_daily_triage_check(&self) -> bool {
        !self.enable_daily_triage_check
    }

    #[must_use]
    pub fn allowed_sms(&self) -> Vec<String> {
        split_allowlist(&self.allowed_sms_senders)
    }

    #[must_use]
    pub fn allowed_email(&self) -> Vec<String> {
        split_allowlist(&self.allowed_email_senders)
    }
}

fn read_strict_config_value(root: &std::path::Path) -> anyhow::Result<Option<Value>> {
    let path = root.join(".config/config.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(anyhow::Error::from(error)),
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(anyhow::Error::from)?;
    if !value.is_object() {
        anyhow::bail!("{} must contain a JSON object", path.display());
    }
    Ok(Some(value))
}

fn split_allowlist(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn default_linear_workspace_is_empty_and_yields_no_base_url() {
        let cfg = Config::default();
        assert_eq!(cfg.linear_workspace, "");
        assert_eq!(cfg.linear_base_url(), "");
    }

    #[test]
    fn workspace_interpolates_into_the_issue_url() {
        let cfg: Config = serde_json::from_str(r#"{"linear_workspace": "acme"}"#).unwrap();
        assert_eq!(cfg.linear_base_url(), "https://linear.app/acme/issue/");
    }

    #[test]
    fn workspace_is_trimmed_before_interpolation() {
        let cfg: Config = serde_json::from_str(r#"{"linear_workspace": "  acme  "}"#).unwrap();
        assert_eq!(cfg.linear_base_url(), "https://linear.app/acme/issue/");
    }

    #[test]
    fn malformed_linear_workspace_never_becomes_a_link() {
        let cfg: Config = serde_json::from_str(r#"{"linear_workspace":"../outside"}"#).unwrap();

        assert!(!cfg.linear_workspace_is_valid());
        assert_eq!(cfg.linear_base_url(), "");
    }

    #[test]
    fn missing_linear_workspace_falls_back_to_default() {
        let cfg: Config =
            serde_json::from_str(r#"{"daily_triage_name_pattern": "Weekly Review"}"#).unwrap();
        assert_eq!(cfg.daily_triage_name_pattern, "Weekly Review");
        assert_eq!(cfg.linear_workspace, "");
    }

    #[test]
    fn unknown_keys_in_the_store_are_ignored() {
        // `root` / `markdown_to_pdf_path` live in the same file but are read
        // elsewhere; they must not break the typed load.
        let cfg: Config =
            serde_json::from_str(r#"{"root": "~/brain", "markdown_to_pdf_path": "/x"}"#).unwrap();
        assert_eq!(cfg.day_rollover_hour, 6);
    }

    #[test]
    fn default_day_rollover_hour_is_six_am() {
        assert_eq!(Config::default().day_rollover_hour, 6);
    }

    #[test]
    fn daily_triage_check_is_enabled_by_default() {
        assert!(Config::default().enable_daily_triage_check);
    }

    #[test]
    fn disabling_the_daily_triage_check_is_persistent_config_not_a_cli_flag() {
        let cfg: Config = serde_json::from_str(r#"{"enable_daily_triage_check": false}"#).unwrap();

        assert!(!cfg.enable_daily_triage_check);
        assert!(cfg.skip_daily_triage_check());
    }

    #[test]
    fn allowlists_trim_and_normalize_entries() {
        let cfg: Config = serde_json::from_str(
            r#"{"allowed_sms_senders":" +1555, +1666 ","allowed_email_senders":" Me@Example.com "}"#,
        )
        .unwrap();
        assert_eq!(cfg.allowed_sms(), vec!["+1555", "+1666"]);
        assert_eq!(cfg.allowed_email(), vec!["me@example.com"]);
    }

    #[test]
    fn legacy_numeric_sms_allowlist_recovers_its_leading_plus() {
        let cfg: Config = serde_json::from_str(r#"{"allowed_sms_senders":16072809118}"#).unwrap();

        assert_eq!(cfg.allowed_sms(), vec!["+16072809118"]);
    }

    #[test]
    fn explicit_day_rollover_hour_overrides_default() {
        let cfg: Config = serde_json::from_str(r#"{"day_rollover_hour": 4}"#).unwrap();
        assert_eq!(cfg.day_rollover_hour, 4);
    }
}

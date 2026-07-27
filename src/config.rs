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

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
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

impl Default for Config {
    fn default() -> Self {
        Self {
            daily_triage_name_pattern: "Morning Triage".to_owned(),
            linear_workspace: String::new(),
            day_rollover_hour: 6,
        }
    }
}

impl Config {
    /// Load the typed config from the shared store. Returns defaults when the
    /// file is missing, unreadable, or shaped wrong — a misconfigured file
    /// should never block the tasks shell from opening.
    #[must_use]
    pub fn load() -> Self {
        serde_json::from_value(Value::Object(crate::settings::load_map())).unwrap_or_default()
    }

    /// Full Linear issue-URL prefix built from the configured workspace, or an
    /// empty string when no workspace is set (Linear links are then omitted).
    #[must_use]
    pub fn linear_base_url(&self) -> String {
        let ws = self.linear_workspace.trim();
        if ws.is_empty() {
            String::new()
        } else {
            format!("https://linear.app/{ws}/issue/")
        }
    }
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
    fn explicit_day_rollover_hour_overrides_default() {
        let cfg: Config = serde_json::from_str(r#"{"day_rollover_hour": 4}"#).unwrap();
        assert_eq!(cfg.day_rollover_hour, 4);
    }
}

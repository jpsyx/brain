//! Runtime configuration for the `tasks` shell.
//!
//! The config file lives next to the source / binary at `config.json` and is
//! resolved by walking up from the running executable. Missing file or
//! missing fields fall back to [`Config::default`] — i.e. it's safe to
//! delete the file entirely, the tasks shell just loses the daily-triage check.
//!
//! Adding a new option means: (1) extend `Config`, (2) default it, (3) bump
//! `docs/architecture.md` with the new knob.

use std::{fs, path::PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Case-insensitive regex matched against habit *names* to find the
    /// daily-triage habit that gates the startup triage modal. Matching by
    /// name (not ID) is deliberate: the triage habit recurs, so every
    /// occurrence gets a fresh ID (H24 → H31 → H41 → …); a pinned ID would
    /// go stale the moment the habit rolls over and the nudge would fire
    /// every day forever. The pattern survives recurrence and tolerates
    /// suffix/capitalization drift (e.g. matches both `Morning Triage` and
    /// `Morning Triage (5mins)`). Empty string disables the check (useful
    /// for setups without the `/triage` skill installed); an invalid regex
    /// is treated the same as empty.
    pub daily_triage_name_pattern: String,

    /// Base URL prefix for Linear issue links. The task's `linear_issue`
    /// identifier (e.g. `AVA-123`) is appended to this to form the full
    /// issue URL opened by the "open link" command / `Ctrl+O`.
    pub linear_base_url: String,

    /// Local hour (0-23) at which the "logical day" rolls over for the
    /// daily-triage re-check. A tasks session can stay open for days, so on
    /// every user refresh (the `r` hotkey) the shell re-evaluates whether to
    /// re-open the triage nudge. The boundary is this hour, not midnight:
    /// working past midnight still counts as the previous day until the
    /// rollover hour, so a late-night session isn't nagged for a "new day"
    /// the moment the clock ticks past 00:00. Defaults to 6 (6 AM). An
    /// out-of-range value (>23) falls back to the default.
    pub day_rollover_hour: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            daily_triage_name_pattern: "Morning Triage".to_owned(),
            linear_base_url: "https://linear.app/avandar/issue/".to_owned(),
            day_rollover_hour: 6,
        }
    }
}

impl Config {
    /// Load `config.json` from the package root (the directory containing
    /// `Cargo.toml` / the release binary). Returns defaults when the file
    /// is missing or unreadable — a misconfigured file should never block
    /// the user from opening the tasks shell.
    #[must_use]
    pub fn load() -> Self {
        find_config_path()
            .and_then(|p| fs::read_to_string(&p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
}

/// Walk up from the current exe looking for `config.json`. Covers both
/// `target/release/tasks` (config sits two dirs up) and a dev `cargo run`
/// from inside the package (one dir up).
fn find_config_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    for _ in 0..4 {
        let candidate = dir.join("config.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn default_linear_base_url_points_at_avandar_workspace() {
        assert_eq!(
            Config::default().linear_base_url,
            "https://linear.app/avandar/issue/"
        );
    }

    #[test]
    fn missing_linear_base_url_falls_back_to_default() {
        // A config that only sets other fields should still get the
        // default linear_base_url via `#[serde(default)]`.
        let cfg: Config =
            serde_json::from_str(r#"{"daily_triage_name_pattern": "Weekly Review"}"#).unwrap();
        assert_eq!(cfg.daily_triage_name_pattern, "Weekly Review");
        assert_eq!(cfg.linear_base_url, "https://linear.app/avandar/issue/");
    }

    #[test]
    fn default_day_rollover_hour_is_six_am() {
        assert_eq!(Config::default().day_rollover_hour, 6);
    }

    #[test]
    fn missing_day_rollover_hour_falls_back_to_default() {
        let cfg: Config =
            serde_json::from_str(r#"{"daily_triage_name_pattern": "Morning Triage"}"#).unwrap();
        assert_eq!(cfg.day_rollover_hour, 6);
    }

    #[test]
    fn explicit_day_rollover_hour_overrides_default() {
        let cfg: Config = serde_json::from_str(r#"{"day_rollover_hour": 4}"#).unwrap();
        assert_eq!(cfg.day_rollover_hour, 4);
    }

    #[test]
    fn explicit_linear_base_url_overrides_default() {
        let cfg: Config =
            serde_json::from_str(r#"{"linear_base_url": "https://linear.app/acme/issue/"}"#)
                .unwrap();
        assert_eq!(cfg.linear_base_url, "https://linear.app/acme/issue/");
    }
}

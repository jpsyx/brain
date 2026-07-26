//! Typed, parse-only view of the `sync` block in `~/.config/brain/env.json`.
//!
//! C1 only *parses* this — no rclone, no transfers, no triggers. C2+ reads these
//! values to drive Backblaze sync. All fields are optional; an absent block ⇒
//! sync disabled and brain behaves exactly as before.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    pub enabled: bool,
    pub b2_bucket: String,
    pub b2_path: String,
    pub b2_key_id: String,
    pub b2_app_key: String,
    #[serde(default = "default_true")]
    pub on_start: bool,
    #[serde(default = "default_true")]
    pub on_exit: bool,
    #[serde(default = "default_true")]
    pub watch: bool,
    #[serde(default = "default_max_delete")]
    pub max_delete_percent: u8,
    /// Extra rclone exclude patterns (appended to the built-in excludes), e.g.
    /// "**/test-data/**". Lets large/unwanted paths stay out of the sync.
    pub exclude: Vec<String>,
    /// Skip files larger than this rclone size string (e.g. "100M"); empty = no cap.
    pub max_size: String,
    /// Watcher quiescence window in milliseconds (fire a sync once changes settle).
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
}

fn default_true() -> bool {
    true
}
fn default_max_delete() -> u8 {
    50
}
fn default_debounce_ms() -> u64 {
    3000
}

impl SyncConfig {
    /// Load the `sync` block from the brain-env store; defaults when absent.
    #[must_use]
    pub fn load() -> Self {
        crate::env::load_map()
            .get("sync")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    /// True when sync is switched on AND a bucket is configured.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.enabled && !self.b2_bucket.trim().is_empty()
    }

    /// Effective watcher state: on by default whenever sync is configured,
    /// unless explicitly disabled via `watch=false`.
    #[must_use]
    pub fn watch_effective(&self) -> bool {
        self.is_configured() && self.watch
    }

    /// The watcher's quiescence window.
    #[must_use]
    pub fn debounce(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.debounce_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> SyncConfig {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn absent_fields_default_and_disable_sync() {
        let c = parse("{}");
        assert!(!c.enabled && !c.is_configured() && !c.watch_effective());
        assert_eq!(c.max_delete_percent, 50);
        assert!(c.on_start && c.on_exit && c.watch);
        assert_eq!(c.debounce_ms, 3000);
    }

    #[test]
    fn debounce_defaults_to_3s_and_maps_to_duration() {
        let c = parse("{}");
        assert_eq!(c.debounce_ms, 3000);
        assert_eq!(c.debounce(), std::time::Duration::from_secs(3));
        let c2 = parse(r#"{"debounce_ms": 500}"#);
        assert_eq!(c2.debounce(), std::time::Duration::from_millis(500));
    }

    #[test]
    fn configured_requires_enabled_and_a_bucket() {
        assert!(!parse(r#"{"enabled": true}"#).is_configured());
        assert!(!parse(r#"{"b2_bucket": "b"}"#).is_configured());
        assert!(parse(r#"{"enabled": true, "b2_bucket": "b"}"#).is_configured());
    }

    #[test]
    fn watch_defaults_on_when_configured_and_off_when_disabled() {
        assert!(parse(r#"{"enabled": true, "b2_bucket": "b"}"#).watch_effective());
        assert!(!parse(r#"{"enabled": true, "b2_bucket": "b", "watch": false}"#).watch_effective());
    }

    #[test]
    fn exclude_and_max_size_default_empty() {
        let c: SyncConfig = serde_json::from_str("{}").unwrap();
        assert!(c.exclude.is_empty());
        assert!(c.max_size.is_empty());
    }
}

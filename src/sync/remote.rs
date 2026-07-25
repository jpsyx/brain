//! Pure builder: a `SyncConfig` → the rclone B2 remote.
//!
//! Expressed as `RCLONE_CONFIG_*` environment variables plus the
//! `BRAIN:<bucket>/<path>` argument. Credentials travel via env, never on argv
//! (so they don't leak via `ps`) and never in a persisted rclone.conf.

use crate::sync::config::SyncConfig;

/// The remote name used in both the env-var keys and the argv reference.
const REMOTE: &str = "BRAIN";

/// A fully-resolved rclone remote: the env vars that define it and the argv
/// token that references it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub env: Vec<(String, String)>,
    pub arg: String,
}

/// Build the B2 remote from sync config. `b2_path` is an optional prefix within
/// the bucket; a trailing slash is trimmed and an empty prefix is omitted.
#[must_use]
pub fn build_remote(cfg: &SyncConfig) -> Remote {
    let env = vec![
        (format!("RCLONE_CONFIG_{REMOTE}_TYPE"), "b2".to_owned()),
        (format!("RCLONE_CONFIG_{REMOTE}_ACCOUNT"), cfg.b2_key_id.clone()),
        (format!("RCLONE_CONFIG_{REMOTE}_KEY"), cfg.b2_app_key.clone()),
    ];
    let prefix = cfg.b2_path.trim().trim_matches('/');
    let arg = if prefix.is_empty() {
        format!("{REMOTE}:{}", cfg.b2_bucket.trim())
    } else {
        format!("{REMOTE}:{}/{prefix}", cfg.b2_bucket.trim())
    };
    Remote { env, arg }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SyncConfig {
        serde_json::from_str(
            r#"{"enabled":true,"b2_bucket":"my-brain","b2_key_id":"KID","b2_app_key":"AKEY"}"#,
        )
        .unwrap()
    }

    #[test]
    fn creds_go_in_env_never_in_the_arg() {
        let r = build_remote(&cfg());
        assert!(r.env.contains(&("RCLONE_CONFIG_BRAIN_TYPE".to_owned(), "b2".to_owned())));
        assert!(r.env.contains(&("RCLONE_CONFIG_BRAIN_ACCOUNT".to_owned(), "KID".to_owned())));
        assert!(r.env.contains(&("RCLONE_CONFIG_BRAIN_KEY".to_owned(), "AKEY".to_owned())));
        assert!(!r.arg.contains("KID") && !r.arg.contains("AKEY"));
    }

    #[test]
    fn arg_omits_an_empty_path_prefix() {
        assert_eq!(build_remote(&cfg()).arg, "BRAIN:my-brain");
    }

    #[test]
    fn arg_includes_and_trims_a_path_prefix() {
        let mut c = cfg();
        c.b2_path = "/sub/dir/".to_owned();
        assert_eq!(build_remote(&c).arg, "BRAIN:my-brain/sub/dir");
    }
}

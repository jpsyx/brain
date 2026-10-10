use super::remote::{parse_link, private_bucket, signing_remote, validate_config};
use crate::sync::config::SyncConfig;

fn config() -> SyncConfig {
    serde_json::from_value(serde_json::json!({"enabled": true, "b2_bucket": "example-bucket", "b2_path": "workspace/", "b2_key_id": "example-id", "b2_app_key": "example-secret"})).unwrap()
}

#[test]
fn signing_uses_exact_object_s3_and_b2_filename_encoding_without_argv_secrets() {
    let remote = signing_remote(&config(), "https://s3.us-west-004.backblazeb2.com").unwrap();
    assert_eq!(remote.arg, "BRAINSHARE:example-bucket/workspace");
    for pair in [
        ("TYPE", "s3"),
        ("REGION", "us-west-004"),
        ("ENCODING", "Slash,BackSlash,Del,Ctl,InvalidUtf8,Dot"),
        ("FORCE_PATH_STYLE", "true"),
    ] {
        assert!(remote.env.contains(&(
            format!("RCLONE_CONFIG_BRAINSHARE_{}", pair.0),
            pair.1.to_owned()
        )));
    }
    assert!(!remote.arg.contains("example-secret"));
    for endpoint in [
        "http://s3.us-west-004.backblazeb2.com",
        "https://attacker.test",
        "https://s3.us-west-004.backblazeb2.com.evil.test",
        "https://s3.us-west-004.backblazeb2.com/path",
    ] {
        assert!(signing_remote(&config(), endpoint).is_err());
    }
}

#[test]
fn sharing_refuses_unconfigured_and_client_encrypted_remotes() {
    assert!(validate_config(&SyncConfig::default()).is_err());
    let mut encrypted = config();
    encrypted.crypt_password = "obscured".to_owned();
    assert!(
        validate_config(&encrypted)
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
}

#[test]
fn only_a_private_matching_bucket_can_promise_expiry() {
    let private = serde_json::json!({"buckets": [{"bucketName":"example-bucket", "bucketType":"allPrivate"}]});
    assert!(private_bucket(&private, "example-bucket").is_ok());
    assert!(private_bucket(&private, "different-bucket").is_err());
    let public =
        serde_json::json!({"buckets": [{"bucketName":"example-bucket", "bucketType":"allPublic"}]});
    assert!(private_bucket(&public, "example-bucket").is_err());
}

#[test]
fn unsigned_or_wrong_duration_output_is_never_presented_as_a_temporary_link() {
    let base = "https://s3.us-west-004.backblazeb2.com/example-bucket/workspace/paper.pdf";
    let query = "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Date=20261010T120000Z&X-Amz-Expires=3600&X-Amz-Signature=abc";
    let result = parse_link(&format!("{base}?{query}\n"), 3600).unwrap();
    assert_eq!(result.expires_at.to_rfc3339(), "2026-10-10T13:00:00+00:00");
    assert!(parse_link(base, 3600).is_err());
    assert!(parse_link(&format!("{base}?{query}"), 604_800).is_err());
    assert!(parse_link(&format!("{base}?{query}\n{base}"), 3600).is_err());
}

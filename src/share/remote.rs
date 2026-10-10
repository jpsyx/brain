use crate::sync::{config::SyncConfig, remote::Remote};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDateTime, Utc};

pub(super) fn validate_config(config: &SyncConfig) -> Result<()> {
    if !config.is_configured()
        || config.b2_key_id.trim().is_empty()
        || config.b2_app_key.trim().is_empty()
    {
        bail!("Sharing needs Backblaze sync configuration; run brain sync setup first");
    }
    if config.crypt_enabled() {
        bail!(
            "Direct sharing is unavailable for a client-encrypted remote; its files cannot be downloaded as plaintext"
        );
    }
    Ok(())
}

pub(super) fn private_bucket(response: &serde_json::Value, bucket: &str) -> Result<()> {
    let found = response.get("buckets").and_then(serde_json::Value::as_array)
        .and_then(|buckets| buckets.iter().find(|item| item["bucketName"] == bucket))
        .context("Backblaze did not return the configured bucket; check the application's listBuckets permission")?;
    if found["bucketType"] != "allPrivate" {
        bail!("Expiring access requires a private Backblaze bucket; this bucket is public");
    }
    Ok(())
}

pub(super) fn signing_remote(config: &SyncConfig, endpoint: &str) -> Result<Remote> {
    let region = endpoint
        .strip_prefix("https://s3.")
        .and_then(|host| host.strip_suffix(".backblazeb2.com"))
        .filter(|region| {
            !region.is_empty()
                && region
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
        .context("Backblaze returned an invalid S3 endpoint")?;
    let env = [
        ("TYPE", "s3"),
        ("PROVIDER", "Other"),
        ("ENV_AUTH", "false"),
        ("ACCESS_KEY_ID", config.b2_key_id.trim()),
        ("SECRET_ACCESS_KEY", config.b2_app_key.trim()),
        ("ENDPOINT", endpoint),
        ("REGION", region),
        ("FORCE_PATH_STYLE", "true"),
        ("NO_CHECK_BUCKET", "true"),
        // The signer must address the same encoded object names as the B2 uploader.
        ("ENCODING", "Slash,BackSlash,Del,Ctl,InvalidUtf8,Dot"),
    ]
    .into_iter()
    .map(|(key, value)| (format!("RCLONE_CONFIG_BRAINSHARE_{key}"), value.to_owned()))
    .collect();
    let prefix = config.b2_path.trim().trim_matches('/');
    let bucket = config.b2_bucket.trim();
    let arg = if prefix.is_empty() {
        format!("BRAINSHARE:{bucket}")
    } else {
        format!("BRAINSHARE:{bucket}/{prefix}")
    };
    Ok(Remote { env, arg })
}

pub(super) struct Link {
    pub url: String,
    pub expires_at: DateTime<Utc>,
}

pub(super) fn parse_link(output: &str, seconds: u64) -> Result<Link> {
    let url = output.trim();
    let (base, query) = url
        .split_once('?')
        .context("rclone returned no signed download URL")?;
    if !base.starts_with("https://") || url.chars().any(char::is_whitespace) {
        bail!("rclone returned an invalid download URL");
    }
    let values: std::collections::BTreeMap<_, _> = query
        .split('&')
        .filter_map(|part| part.split_once('='))
        .collect();
    if values.get("X-Amz-Algorithm") != Some(&"AWS4-HMAC-SHA256")
        || values
            .get("X-Amz-Expires")
            .and_then(|n| n.parse::<u64>().ok())
            != Some(seconds)
        || values.get("X-Amz-Signature").is_none_or(|s| s.is_empty())
    {
        bail!("rclone did not return a signed URL with the requested expiration");
    }
    let date = values
        .get("X-Amz-Date")
        .context("Signed URL is missing its creation time")?;
    let created = NaiveDateTime::parse_from_str(date, "%Y%m%dT%H%M%SZ")
        .context("Invalid signed URL creation time")?
        .and_utc();
    let expires_at = created
        .checked_add_signed(chrono::Duration::seconds(i64::try_from(seconds)?))
        .context("Invalid signed URL expiration")?;
    Ok(Link {
        url: url.to_owned(),
        expires_at,
    })
}

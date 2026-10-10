use super::workflow::Backend;
use crate::sync::{config::SyncConfig, identity::VerifiedRemote, remote::Remote};
use crate::workspace::WorkspaceContext;
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

pub(super) struct SystemBackend;

impl Backend for SystemBackend {
    fn verify<'a>(
        &mut self,
        workspace: &WorkspaceContext,
        remote: &'a Remote,
    ) -> Result<VerifiedRemote<'a>> {
        crate::sync::identity::require_remote_identity(workspace.root(), workspace.id(), remote)
    }

    fn signing_endpoint(&mut self, config: &SyncConfig, duration: Duration) -> Result<String> {
        let credentials = base64::engine::general_purpose::STANDARD.encode(format!(
            "{}:{}",
            config.b2_key_id.trim(),
            config.b2_app_key.trim()
        ));
        let account = request_json(
            "https://api.backblazeb2.com/b2api/v4/b2_authorize_account",
            &format!("Basic {credentials}"),
            None,
        )?;
        let api = account
            .pointer("/apiInfo/storageApi")
            .context("Backblaze storage API is unavailable")?;
        let endpoint = field(api, "s3ApiUrl")?;
        super::remote::signing_remote(config, endpoint)?;
        if let Some(expires) = account["applicationKeyExpirationTimestamp"].as_i64() {
            let requested =
                chrono::Utc::now().timestamp_millis() + i64::try_from(duration.as_millis())?;
            if expires < requested {
                bail!(
                    "The Backblaze application key expires before this link would; choose a shorter duration"
                );
            }
        }
        let api_url = field(api, "apiUrl")?;
        validate_api_url(api_url)?;
        let body = serde_json::json!({"accountId": field(&account, "accountId")?, "bucketName": config.b2_bucket.trim()});
        let buckets = request_json(
            &format!("{api_url}/b2api/v4/b2_list_buckets"),
            field(&account, "authorizationToken")?,
            Some(&body),
        )?;
        super::remote::private_bucket(&buckets, config.b2_bucket.trim())?;
        Ok(endpoint.to_owned())
    }

    fn upload(&mut self, remote: VerifiedRemote<'_>, path: &Path, relative: &str) -> Result<()> {
        let target = format!("{}/{relative}", remote.remote().arg.trim_end_matches('/'));
        let path = path.to_str().context("Shared path is not UTF-8")?;
        rclone(
            remote.remote(),
            &["copyto", "--checksum", "--", path, &target],
            "Upload failed; check Backblaze write access and the network",
        )?;
        Ok(())
    }

    fn sign(&mut self, remote: &Remote, relative: &str, duration: Duration) -> Result<String> {
        let target = format!("{}/{relative}", remote.arg.trim_end_matches('/'));
        rclone(
            remote,
            &[
                "link",
                "--expire",
                &format!("{}s", duration.as_secs()),
                "--",
                &target,
            ],
            "Download link failed; check Backblaze read access, S3-compatible credentials, and the network",
        )
    }
}

fn rclone(remote: &Remote, args: &[&str], failure: &str) -> Result<String> {
    let mut command = Command::new("rclone");
    // A caller's verbose/dump/log-file settings must not persist bearer links or keys.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RCLONE_") {
            command.env_remove(key);
        }
    }
    command
        .envs(remote.env.iter().cloned())
        .args([
            "--config",
            "/dev/null",
            "--log-level",
            "ERROR",
            "--contimeout",
            "15s",
            "--timeout",
            "60s",
            "--retries",
            "2",
            "--low-level-retries",
            "2",
        ])
        .args(args)
        .stdin(Stdio::null());
    let output = command
        .output()
        .context("rclone is required for sharing; install it and run brain sync setup")?;
    if !output.status.success() {
        bail!("{failure}");
    }
    String::from_utf8(output.stdout).context("rclone returned invalid text")
}

fn field<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .with_context(|| format!("Backblaze response is missing {key}"))
}

fn validate_api_url(url: &str) -> Result<()> {
    let host = url
        .strip_prefix("https://api")
        .and_then(|host| host.strip_suffix(".backblazeb2.com"));
    if !host.is_some_and(|part| {
        !part.is_empty() && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    }) {
        bail!("Backblaze returned an invalid API endpoint");
    }
    Ok(())
}

fn request_json(
    url: &str,
    authorization: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    // curl's stdin config keeps authorization material out of argv and disk.
    let mut config = format!(
        "url = {}\nheader = {}\n",
        serde_json::to_string(url)?,
        serde_json::to_string(&format!("Authorization: {authorization}"))?
    );
    if let Some(body) = body {
        writeln!(
            config,
            "header = \"Content-Type: application/json\"\ndata = {}",
            serde_json::to_string(&body.to_string())?
        )?;
    }
    let mut child = Command::new("curl")
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--proto",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "30",
            "--max-filesize",
            "1048576",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("curl is required to authorize Backblaze sharing")?;
    let written = child
        .stdin
        .take()
        .context("Open curl input")?
        .write_all(config.as_bytes());
    let output = child.wait_with_output()?;
    written.context("Send Backblaze authorization")?;
    if !output.status.success() {
        bail!(
            "Backblaze authorization or bucket lookup failed; check credentials, listBuckets permission, and the network"
        );
    }
    serde_json::from_slice(&output.stdout).context("Backblaze returned an invalid response")
}

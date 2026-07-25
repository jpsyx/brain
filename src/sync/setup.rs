//! `brain sync setup`: check rclone, collect the B2 bucket + credentials into
//! the brain-env `sync` block, verify/create the bucket, and establish the
//! baseline.
//!
//! Interactive on /dev/tty; only the input validation is pure and unit-tested.

use std::io::{BufRead, BufReader, Write};

use anyhow::{Result, bail};

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;

/// Validate collected setup inputs before writing them to env. Pure.
pub fn validate(bucket: &str, key_id: &str, app_key: &str) -> Result<()> {
    if bucket.trim().is_empty() {
        bail!("bucket name is required");
    }
    if key_id.trim().is_empty() || app_key.trim().is_empty() {
        bail!("both a B2 key ID and application key are required");
    }
    Ok(())
}

/// Interactive setup. Writes the `sync` block into brain env, verifies/creates
/// the bucket, and runs the initial baseline sync. Never unit-tested (I/O + net).
pub fn run() -> Result<()> {
    if !rclone_present() {
        eprintln!(
            "rclone is not installed. Install it (https://rclone.org/downloads/) and re-run `brain sync setup`."
        );
        return Ok(());
    }
    let existing = SyncConfig::load();
    let bucket = prompt("B2 bucket", &existing.b2_bucket)?;
    let key_id = prompt("B2 key ID", &existing.b2_key_id)?;
    let app_key = prompt("B2 application key", &existing.b2_app_key)?;
    validate(&bucket, &key_id, &app_key)?;

    let block = serde_json::json!({
        "enabled": true,
        "b2_bucket": bucket,
        "b2_path": existing.b2_path,
        "b2_key_id": key_id,
        "b2_app_key": app_key,
    });
    crate::env::set_raw("sync", block)?;

    verify_or_create_bucket()?;

    println!("Establishing the baseline (this may take a while)…");
    let cfg = SyncConfig::load();
    let root = crate::paths::brain_root()?;
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    crate::sync::command::sync_once(&cfg, &root, Direction::Resync, (&ts, &ts, &date))?;
    println!("sync configured.");
    Ok(())
}

/// Read one line from `/dev/tty`, prompting with `label` (showing `current` as
/// the default). Empty input keeps `current`; non-empty input is trimmed and
/// used. Same open-the-controlling-terminal pattern as
/// `personalization::onboarding`, so the prompt works even when the TUI owns
/// /dev/tty and regardless of stdin redirection.
fn prompt(label: &str, current: &str) -> Result<String> {
    let tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    let mut out = tty.try_clone()?;
    let mut reader = BufReader::new(tty);

    if current.is_empty() {
        write!(out, "  {label}: ")?;
    } else {
        write!(out, "  {label} [{current}]: ")?;
    }
    out.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let trimmed = line.trim();
    Ok(if trimmed.is_empty() { current.to_owned() } else { trimmed.to_owned() })
}

fn rclone_present() -> bool {
    std::process::Command::new("rclone")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Probe the configured bucket with rclone; offer to create it if missing.
fn verify_or_create_bucket() -> Result<()> {
    let cfg = SyncConfig::load();
    let remote = crate::sync::remote::build_remote(&cfg);
    // `rclone lsd <remote>` lists dirs; success means the bucket is reachable.
    let mut probe = std::process::Command::new("rclone");
    probe.arg("lsd").arg(&remote.arg);
    for (k, v) in &remote.env {
        probe.env(k, v);
    }
    let ok = probe.output().is_ok_and(|o| o.status.success());
    if ok {
        return Ok(());
    }
    println!("Bucket not reachable; attempting to create it…");
    let mut mk = std::process::Command::new("rclone");
    mk.arg("mkdir").arg(&remote.arg);
    for (k, v) in &remote.env {
        mk.env(k, v);
    }
    let created = mk.output().is_ok_and(|o| o.status.success());
    if !created {
        bail!("could not reach or create the B2 bucket — check the bucket name and credentials");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_fields() {
        assert!(validate("", "k", "a").is_err());
        assert!(validate("b", "", "a").is_err());
        assert!(validate("b", "k", "").is_err());
        assert!(validate("b", "k", "a").is_ok());
    }
}

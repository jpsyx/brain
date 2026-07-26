//! `brain sync setup`: check rclone, collect the B2 bucket + credentials into
//! the brain-env `sync` block, verify/create the bucket, and establish the
//! baseline.
//!
//! Interactive on /dev/tty; only the input validation is pure and unit-tested.

use std::io::{BufRead, BufReader, Write};

use anyhow::{bail, Result};

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;
use crate::theme::Theme;

/// Parse a yes/no answer. Yes-ish (`y`/`yes`, case-insensitive) is `true`;
/// anything else, including empty, is `false` — so the safe default is "no
/// bucket yet", which shows the walkthrough.
#[must_use]
pub fn parse_yes_no(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// The step-by-step Backblaze bucket + application-key walkthrough shown when
/// the user doesn't have a bucket yet. Pure static text, so a unit test guards
/// that it keeps covering every critical setting.
#[must_use]
pub fn bucket_walkthrough() -> String {
    "\
No problem — we'll walk you through it (about 2 minutes):

  1. Sign in at https://www.backblaze.com (a free account is fine to start).

  2. B2 Cloud Storage -> Buckets -> \"Create a Bucket\":
       - Bucket Unique Name:  a globally-unique name (e.g. \"yourname-brain\")
       - Files in Bucket are: Private
       - Default Encryption:  Enable  (Backblaze-managed; nothing for you to hold)
       - Object Lock:         Disable (sync must be able to update and delete files)

  3. Account -> Application Keys -> \"Add a New Application Key\":
       - Name:            brain-sync
       - Allow access to: the bucket you just created (scope it to only that one)
       - Leave the file-name prefix and duration blank.
       - Create it, then COPY both values right away:
           * keyID
           * applicationKey  (shown ONLY once -- copy it before you leave the page)

  Backblaze manages the encryption key, so there is nothing you can lose that
  would lock you out, and your notes also stay on this machine.
"
    .to_owned()
}

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
    let theme = Theme::active();
    if !rclone_present() {
        eprintln!(
            "{}",
            theme.error(
                "rclone is not installed. Install it (https://rclone.org/downloads/) and re-run `brain sync setup`."
            )
        );
        return Ok(());
    }
    println!("{}", theme.heading("Brain sync setup"));
    println!(
        "{} keeps your ~/brain in sync across machines through a private\nBackblaze B2 bucket (encrypted at rest, and an off-site backup).\n",
        theme.accent("brain sync")
    );

    if !ask_has_bucket(theme)? {
        println!("{}", theme.heading("\nBackblaze bucket walkthrough"));
        println!("\n{}", bucket_walkthrough());
        prompt(
            &theme.prompt("Press Enter once your bucket and application key are ready"),
            "",
        )?;
    }

    println!("\nEnter your bucket details (from the Backblaze console):");
    let existing = SyncConfig::load();
    let bucket = prompt(&theme.prompt("B2 bucket name"), &existing.b2_bucket)?;
    let key_id = prompt(&theme.prompt("B2 keyID"), &existing.b2_key_id)?;
    let app_key = prompt(&theme.prompt("B2 applicationKey"), &existing.b2_app_key)?;
    validate(&bucket, &key_id, &app_key)?;

    let block = sync_block(&bucket, &key_id, &app_key, &existing);
    crate::env::set_raw("sync", block)?;

    verify_or_create_bucket(theme)?;

    println!(
        "{}",
        theme.info("Establishing the baseline (this may take a while)…")
    );
    let cfg = SyncConfig::load();
    let root = crate::paths::brain_root()?;
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    crate::sync::command::sync_once(&cfg, &root, Direction::Resync, (&ts, &ts, &date))?;
    println!("{}", theme.success("✓ sync configured."));
    Ok(())
}

/// Read one line from `/dev/tty`, prompting with `label` (showing `current` as
/// the default). Empty input keeps `current`; non-empty input is trimmed and
/// used. Same open-the-controlling-terminal pattern as
/// `personalization::onboarding`, so the prompt works even when the TUI owns
/// /dev/tty and regardless of stdin redirection.
///
/// `pub(crate)` so `sync::command`'s interactive `resolve` picker can reuse
/// this rather than reimplementing the /dev/tty dance.
pub(crate) fn prompt(label: &str, current: &str) -> Result<String> {
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
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
    Ok(if trimmed.is_empty() {
        current.to_owned()
    } else {
        trimmed.to_owned()
    })
}

/// Ask whether the user already has a bucket. Thin `/dev/tty` shell over
/// [`parse_yes_no`]; a bare Enter means "no" (show the walkthrough).
fn ask_has_bucket(theme: Theme) -> Result<bool> {
    let answer = prompt(
        &theme.prompt("Do you already have a Backblaze private bucket to connect to? [y/N]"),
        "",
    )?;
    Ok(parse_yes_no(&answer))
}

#[must_use]
fn sync_block(
    bucket: &str,
    key_id: &str,
    app_key: &str,
    existing: &SyncConfig,
) -> serde_json::Value {
    serde_json::json!({
        "enabled": true,
        "b2_bucket": bucket,
        "b2_path": existing.b2_path,
        "b2_key_id": key_id,
        "b2_app_key": app_key,
        "crypt_password": existing.crypt_password,
        "crypt_password2": existing.crypt_password2,
        "crypt_filename_encryption": existing.crypt_filename_encryption,
        "crypt_directory_name_encryption": existing.crypt_directory_name_encryption,
    })
}

fn rclone_present() -> bool {
    std::process::Command::new("rclone")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Probe the configured bucket with rclone; offer to create it if missing.
fn verify_or_create_bucket(theme: Theme) -> Result<()> {
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
    println!(
        "{}",
        theme.warning("Bucket not reachable; attempting to create it…")
    );
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

    #[test]
    fn parse_yes_no_reads_affirmatives_only() {
        assert!(parse_yes_no("y"));
        assert!(parse_yes_no("Yes"));
        assert!(parse_yes_no("  YES  "));
        assert!(!parse_yes_no("n"));
        assert!(!parse_yes_no("no"));
        assert!(!parse_yes_no("")); // default: no bucket yet → show the walkthrough
        assert!(!parse_yes_no("maybe"));
    }

    #[test]
    fn walkthrough_covers_the_critical_bucket_settings() {
        let w = bucket_walkthrough();
        assert!(w.contains("Private"), "must say the bucket is Private");
        assert!(
            w.contains("Default Encryption") && w.contains("Enable"),
            "must tell them to Enable Default Encryption"
        );
        assert!(
            w.contains("Object Lock") && w.contains("Disable"),
            "must tell them to Disable Object Lock"
        );
        assert!(
            w.contains("Application Key"),
            "must cover creating an application key"
        );
        assert!(
            w.contains("keyID") && w.contains("applicationKey"),
            "must name both credential values to copy"
        );
    }

    #[test]
    fn sync_block_preserves_existing_crypt_fields() {
        let existing = SyncConfig {
            b2_path: "prefix".to_owned(),
            crypt_password: "obscured-pass".to_owned(),
            crypt_password2: "obscured-salt".to_owned(),
            crypt_filename_encryption: "obfuscate".to_owned(),
            crypt_directory_name_encryption: false,
            ..SyncConfig::default()
        };

        let block = sync_block("bucket", "key-id", "app-key", &existing);

        assert_eq!(block["b2_bucket"], "bucket");
        assert_eq!(block["b2_key_id"], "key-id");
        assert_eq!(block["b2_app_key"], "app-key");
        assert_eq!(block["b2_path"], "prefix");
        assert_eq!(block["crypt_password"], "obscured-pass");
        assert_eq!(block["crypt_password2"], "obscured-salt");
        assert_eq!(block["crypt_filename_encryption"], "obfuscate");
        assert_eq!(block["crypt_directory_name_encryption"], false);
    }
}

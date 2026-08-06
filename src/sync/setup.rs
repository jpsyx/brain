//! `brain sync setup`: check rclone, collect the B2 bucket + credentials into
//! the brain-env `sync` block, verify the remote workspace identity, and
//! establish the baseline.
//!
//! Interactive on /dev/tty; only the input validation is pure and unit-tested.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};

use anyhow::{Result, bail};

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;
use crate::theme::Theme;
use crate::workspace::{WorkspaceId, WorkspaceName};

/// Whether setup has enough authority to adopt a nonempty manifestless target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptionAuthorization {
    NeedsInteractiveConfirmation,
    Authorized,
}

/// Validate the dedicated noninteractive adoption flag against the selected
/// workspace identity. Absence deliberately requires a separate human prompt.
pub fn adoption_authorization(
    local_workspace_id: WorkspaceId,
    provided_workspace_id: Option<&str>,
) -> Result<AdoptionAuthorization> {
    let Some(provided_workspace_id) = provided_workspace_id else {
        return Ok(AdoptionAuthorization::NeedsInteractiveConfirmation);
    };
    let provided = WorkspaceId::parse(provided_workspace_id)
        .map_err(|_| anyhow::anyhow!("--adopt-workspace-id must be a valid workspace UUID"))?;
    if provided != local_workspace_id {
        bail!(
            "--adopt-workspace-id {provided} does not match selected workspace UUID {local_workspace_id}"
        );
    }
    Ok(AdoptionAuthorization::Authorized)
}

/// Render the selected local identity and the configured target's observed
/// ownership state before setup asks for any adoption confirmation.
#[must_use]
pub fn format_identity_summary(
    local_name: &WorkspaceName,
    local_workspace_id: WorkspaceId,
    remote_target: &str,
    observed: &crate::sync::identity::RemoteIdentityObservation,
    theme: Theme,
) -> String {
    use crate::sync::identity::RemoteIdentityObservation;

    let status = match observed {
        RemoteIdentityObservation::Empty => theme.info("empty, no workspace manifest"),
        RemoteIdentityObservation::ManifestlessNonempty => {
            theme.warning("nonempty, no workspace manifest")
        }
        RemoteIdentityObservation::CompatibleManifest { .. } => {
            theme.success("compatible workspace manifest")
        }
        RemoteIdentityObservation::InvalidManifest { error } => theme.error(&format!(
            "invalid or incompatible workspace manifest ({error})"
        )),
        RemoteIdentityObservation::UnreadableManifest { message } => theme.error(&format!(
            "workspace manifest is present but unreadable ({message})"
        )),
    };
    let mut summary = format!(
        "{}\n  {} {}\n  {} {}\n  {} {}\n  {} {}",
        theme.heading("Workspace identity"),
        theme.muted("Local workspace:"),
        theme.value(local_name.as_str()),
        theme.muted("Local UUID:"),
        theme.value(&local_workspace_id.to_string()),
        theme.muted("Remote target:"),
        theme.value(remote_target),
        theme.muted("Remote status:"),
        status,
    );
    if let RemoteIdentityObservation::CompatibleManifest { workspace_id } = observed {
        write!(
            &mut summary,
            "\n  {} {}",
            theme.muted("Remote UUID:"),
            theme.value(&workspace_id.to_string())
        )
        .expect("writing to a String cannot fail");
    }
    summary
}

fn adoption_for_observation(
    local_workspace_id: WorkspaceId,
    authorization: AdoptionAuthorization,
    observed: &crate::sync::identity::RemoteIdentityObservation,
    confirm: impl FnOnce() -> Result<bool>,
) -> Result<crate::sync::identity::ManifestlessRemoteAdoption> {
    use crate::sync::identity::{ManifestlessRemoteAdoption, RemoteIdentityObservation};

    if observed != &RemoteIdentityObservation::ManifestlessNonempty {
        return Ok(ManifestlessRemoteAdoption::Refuse);
    }
    match authorization {
        AdoptionAuthorization::Authorized => {
            Ok(ManifestlessRemoteAdoption::Authorized(local_workspace_id))
        }
        AdoptionAuthorization::NeedsInteractiveConfirmation if confirm()? => {
            Ok(ManifestlessRemoteAdoption::Authorized(local_workspace_id))
        }
        AdoptionAuthorization::NeedsInteractiveConfirmation => {
            bail!("remote workspace adoption was not confirmed; no changes were made")
        }
    }
}

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

/// Interactive setup. Verifies the remote workspace identity, writes the
/// `sync` block into brain env, and runs the initial baseline sync.
pub fn run(
    command: &crate::workspace::CommandContext,
    adopt_workspace_id: Option<&str>,
) -> Result<()> {
    let theme = Theme::active();
    if !crate::sync::run::rclone_present() {
        eprintln!(
            "{}",
            crate::sync::run::missing_rclone_guidance(theme, "brain sync setup")
        );
        return Ok(());
    }
    println!("{}", theme.heading("Brain sync setup"));
    println!("{}", setup_intro(theme));

    if !ask_has_bucket(theme)? {
        println!("{}", theme.heading("\nBackblaze bucket walkthrough"));
        println!("\n{}", bucket_walkthrough());
        prompt(
            &theme.prompt("Press Enter once your bucket and application key are ready"),
            "",
        )?;
    }

    println!("\nEnter your bucket details (from the Backblaze console):");
    let existing = SyncConfig::load(command);
    let bucket = prompt(&theme.prompt("B2 bucket name"), &existing.b2_bucket)?;
    let key_id = prompt(&theme.prompt("B2 keyID"), &existing.b2_key_id)?;
    let app_key = prompt(&theme.prompt("B2 applicationKey"), &existing.b2_app_key)?;
    validate(&bucket, &key_id, &app_key)?;

    let block = sync_block(&bucket, &key_id, &app_key, &existing);
    let candidate: SyncConfig = serde_json::from_value(block.clone())?;
    let remote = crate::sync::remote::build_remote(&candidate);
    let root = command.workspace.root();
    let local_id = command.workspace.id();
    let adoption = adoption_authorization(local_id, adopt_workspace_id)?;
    run_setup_stages(
        || {
            println!("{}", theme.info("Validating the local workspace manifest…"));
            println!("{}", theme.info("Probing the remote workspace identity…"));
            crate::sync::identity::ensure_remote_identity_for_setup_with_authorization(
                root,
                local_id,
                &remote,
                |observed| {
                    println!();
                    println!(
                        "{}",
                        format_identity_summary(
                            command.workspace.name(),
                            local_id,
                            &remote.arg,
                            observed,
                            theme,
                        )
                    );
                    adoption_for_observation(local_id, adoption, observed, || {
                        confirm_manifestless_adoption(theme, command.workspace.name(), local_id)
                    })
                },
            )
            .map(|_| ())
        },
        || crate::env::set_raw(command, "sync", block),
        || {
            println!(
                "{}",
                theme.info("Establishing the baseline (this may take a while)…")
            );
            let now = chrono::Utc::now();
            let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
            let date = now.format("%Y-%m-%d").to_string();
            crate::sync::command::sync_once(
                command.workspace.paths(),
                command.workspace.id(),
                &candidate,
                root,
                Direction::Resync,
                (&ts, &ts, &date),
            )?;
            Ok(())
        },
    )?;
    println!("{}", theme.success("✓ sync configured."));
    Ok(())
}

fn run_setup_stages(
    identity: impl FnOnce() -> Result<()>,
    persist_credentials: impl FnOnce() -> Result<()>,
    baseline: impl FnOnce() -> Result<()>,
) -> Result<()> {
    identity()?;
    persist_credentials()?;
    baseline()
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

fn confirm_manifestless_adoption(
    theme: Theme,
    local_name: &WorkspaceName,
    local_workspace_id: WorkspaceId,
) -> Result<bool> {
    let question = theme.prompt(&format!(
        "Adopt this nonempty remote as workspace {local_name} ({local_workspace_id})? [y/N]"
    ));
    let answer = prompt(&question, "")?;
    Ok(parse_yes_no(&answer))
}

#[must_use]
pub fn setup_intro(theme: Theme) -> String {
    format!(
        "{}\n\nThis will enable cloud sync on this machine: brain will connect to an existing private Backblaze B2 bucket, verify the remote workspace identity, save the sync credentials in machine-local brain env, create the RCLONE_TEST safety marker, and establish the first baseline.\n",
        theme.accent("brain sync setup")
    )
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    const LOCAL_WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
    const OTHER_WORKSPACE_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

    fn local_workspace_id() -> crate::workspace::WorkspaceId {
        crate::workspace::WorkspaceId::parse(LOCAL_WORKSPACE_ID).expect("fixed workspace UUID")
    }

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
    fn intro_says_setup_enables_cloud_sync() {
        let intro = setup_intro(Theme::dark(false));
        assert!(intro.contains("This will enable cloud sync"), "{intro}");
        assert!(intro.contains("brain sync setup"), "{intro}");
        assert!(
            intro.contains("verify the remote workspace identity"),
            "{intro}"
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

    #[test]
    fn setup_stages_verify_remote_identity_before_persisting_credentials_or_syncing_data() {
        let stages = RefCell::new(Vec::new());

        run_setup_stages(
            || {
                stages.borrow_mut().push("identity");
                Ok(())
            },
            || {
                stages.borrow_mut().push("credentials");
                Ok(())
            },
            || {
                stages.borrow_mut().push("baseline");
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(*stages.borrow(), ["identity", "credentials", "baseline"]);
    }

    #[test]
    fn identity_refusal_preserves_credentials_and_skips_the_baseline() {
        let stages = RefCell::new(Vec::new());

        let error = run_setup_stages(
            || {
                stages.borrow_mut().push("identity");
                anyhow::bail!("wrong workspace")
            },
            || {
                stages.borrow_mut().push("credentials");
                Ok(())
            },
            || {
                stages.borrow_mut().push("baseline");
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("wrong workspace"));
        assert_eq!(*stages.borrow(), ["identity"]);
    }

    #[test]
    fn adoption_authority_requires_the_exact_selected_workspace_uuid() {
        assert_eq!(
            adoption_authorization(local_workspace_id(), None).unwrap(),
            AdoptionAuthorization::NeedsInteractiveConfirmation
        );
        assert_eq!(
            adoption_authorization(local_workspace_id(), Some(LOCAL_WORKSPACE_ID)).unwrap(),
            AdoptionAuthorization::Authorized
        );

        let mismatch =
            adoption_authorization(local_workspace_id(), Some(OTHER_WORKSPACE_ID)).unwrap_err();
        assert!(mismatch.to_string().contains(LOCAL_WORKSPACE_ID));
        assert!(mismatch.to_string().contains(OTHER_WORKSPACE_ID));
        let malformed = adoption_authorization(local_workspace_id(), Some("not-a-uuid"))
            .expect_err("malformed authority must fail closed");
        assert!(malformed.to_string().contains("valid workspace UUID"));
    }

    #[test]
    fn identity_summary_names_the_local_workspace_target_and_observed_remote_state() {
        let local_name = crate::workspace::WorkspaceName::parse("family").unwrap();
        let target = "BRAIN:shared/brain";
        let manifestless = format_identity_summary(
            &local_name,
            local_workspace_id(),
            target,
            &crate::sync::identity::RemoteIdentityObservation::ManifestlessNonempty,
            Theme::dark(false),
        );
        let local_uuid = format!("Local UUID: {LOCAL_WORKSPACE_ID}");

        for expected in [
            "Workspace identity",
            "Local workspace: family",
            &local_uuid,
            "Remote target: BRAIN:shared/brain",
            "Remote status: nonempty, no workspace manifest",
        ] {
            assert!(manifestless.contains(expected), "{manifestless}");
        }
        assert!(!manifestless.contains("Remote UUID:"), "{manifestless}");

        let matching = format_identity_summary(
            &local_name,
            local_workspace_id(),
            target,
            &crate::sync::identity::RemoteIdentityObservation::CompatibleManifest {
                workspace_id: local_workspace_id(),
            },
            Theme::dark(false),
        );
        assert!(
            matching.contains("Remote status: compatible workspace manifest"),
            "{matching}"
        );
        assert!(
            matching.contains(&format!("Remote UUID: {LOCAL_WORKSPACE_ID}")),
            "{matching}"
        );
    }

    #[test]
    fn manifestless_adoption_prompts_only_when_exact_flag_authority_is_absent() {
        use std::cell::Cell;

        use crate::sync::identity::{ManifestlessRemoteAdoption, RemoteIdentityObservation};

        let prompts = Cell::new(0);
        let authorized = adoption_for_observation(
            local_workspace_id(),
            AdoptionAuthorization::Authorized,
            &RemoteIdentityObservation::ManifestlessNonempty,
            || -> Result<bool> { panic!("exact authority must not prompt") },
        )
        .unwrap();
        assert_eq!(
            authorized,
            ManifestlessRemoteAdoption::Authorized(local_workspace_id())
        );

        let interactive = adoption_for_observation(
            local_workspace_id(),
            AdoptionAuthorization::NeedsInteractiveConfirmation,
            &RemoteIdentityObservation::ManifestlessNonempty,
            || {
                prompts.set(prompts.get() + 1);
                Ok(true)
            },
        )
        .unwrap();
        assert_eq!(
            interactive,
            ManifestlessRemoteAdoption::Authorized(local_workspace_id())
        );
        assert_eq!(prompts.get(), 1);

        let refusal = adoption_for_observation(
            local_workspace_id(),
            AdoptionAuthorization::NeedsInteractiveConfirmation,
            &RemoteIdentityObservation::ManifestlessNonempty,
            || Ok(false),
        )
        .unwrap_err();
        assert!(refusal.to_string().contains("not confirmed"));

        let matching = adoption_for_observation(
            local_workspace_id(),
            AdoptionAuthorization::NeedsInteractiveConfirmation,
            &RemoteIdentityObservation::CompatibleManifest {
                workspace_id: local_workspace_id(),
            },
            || -> Result<bool> { panic!("matching identity must not prompt") },
        )
        .unwrap();
        assert_eq!(matching, ManifestlessRemoteAdoption::Refuse);
    }
}

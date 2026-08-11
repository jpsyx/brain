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

mod intro;
mod sync_block;
pub use intro::setup_intro;
use sync_block::sync_block;

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
        command.workspace.paths(),
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
            prepare_current_schema_for_setup(command, &candidate)?;
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
            )
        },
    )?;
    println!("{}", theme.success("✓ sync configured."));
    Ok(())
}

pub fn prepare_current_schema_for_setup_with_transport(
    paths: &crate::workspace::WorkspacePaths,
    root: &std::path::Path,
    remote_schema: Option<&str>,
    remote_csvs: crate::sync::csv_merge::RemoteCsvState,
    publish: impl FnMut(&str, &[u8]) -> bool,
) -> Result<bool> {
    let local = crate::tasks::schema::inspect_inactive(root)?;
    if !local.current {
        return Ok(false);
    }
    let remote = crate::sync::csv_merge::remote_schema_status(remote_schema)?;
    if remote == crate::sync::csv_merge::SchemaStatus::Current {
        return Ok(false);
    }
    // Whether the remote holds legacy *rows*, not whether CSV files exist. Mere
    // existence had blocked initialization of a remote whose CSVs were already
    // current, leaving neither `brain sync` nor `brain sync setup` able to run.
    if remote_csvs == crate::sync::csv_merge::RemoteCsvState::Legacy {
        bail!(
            "current local task schema cannot overwrite legacy remote task rows; run `{}` to reconcile the remote workspace first",
            crate::workspace::suggest("workspace migrate")
        );
    }
    crate::migration::publish_task_schema_transition_with_transport(
        paths,
        root,
        remote_schema,
        publish,
    )?;
    Ok(true)
}

fn prepare_current_schema_for_setup(
    command: &crate::workspace::CommandContext,
    config: &SyncConfig,
) -> Result<()> {
    let local = crate::tasks::schema::inspect_inactive(command.workspace.root())?;
    if !local.current {
        return Ok(());
    }
    let remote = crate::sync::remote::build_remote(config);
    let verified = crate::sync::identity::require_remote_identity(
        command.workspace.root(),
        command.workspace.id(),
        &remote,
    )?;
    let state = crate::sync::csv_sync::inspect_remote_task_state(
        command.workspace.paths(),
        verified.remote(),
    )?;
    let remote_status = crate::sync::csv_merge::remote_schema_status(state.schema.as_deref())?;
    if remote_status == crate::sync::csv_merge::SchemaStatus::Current {
        return Ok(());
    }
    let remote_csvs = crate::sync::csv_sync::classify_remote_csvs_for_setup(
        command.workspace.paths(),
        verified.remote(),
        state.has_csvs,
    )?;
    if remote_csvs == crate::sync::csv_merge::RemoteCsvState::Legacy {
        bail!(
            "current local task schema cannot overwrite legacy remote task rows; run `{}` to reconcile the remote workspace first",
            crate::workspace::suggest("workspace migrate")
        );
    }
    crate::migration::publish_task_schema_transition(command, config)
}

fn run_setup_stages(
    paths: &crate::workspace::WorkspacePaths,
    identity: impl FnOnce() -> Result<()>,
    persist_credentials: impl FnOnce() -> Result<()>,
    baseline: impl FnOnce() -> Result<crate::sync::verify::Outcome>,
) -> Result<()> {
    let _guard = crate::sync::lock::try_acquire(&paths.sync_lock()).ok_or_else(|| {
        anyhow::anyhow!("another sync owns this workspace; retry setup after it finishes")
    })?;
    crate::migration::require_no_active_rollout(paths)?;
    identity()?;
    match baseline()? {
        crate::sync::verify::Outcome::Clean => persist_credentials(),
        crate::sync::verify::Outcome::NeedsAttention(message)
        | crate::sync::verify::Outcome::Aborted(message) => {
            bail!("initial sync baseline was not clean: {message}")
        }
    }
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

#[cfg(test)]
mod tests;

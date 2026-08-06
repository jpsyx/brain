//! Workspace identity decisions, setup ownership election, and the shared rclone manifest gate.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::sync::remote::Remote;
use crate::workspace::{ManifestError, WorkspaceId, WorkspaceManifest};

mod claim;

const REMOTE_MANIFEST: &str = ".config/workspace.json";

/// The only safe outcomes of comparing local and remote workspace identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteIdentityDecision {
    /// An empty remote may receive this workspace's manifest during setup.
    Initialize,
    /// The remote already belongs to this workspace.
    Proceed,
    /// The target belongs to a different workspace and must not be touched.
    RefuseMismatch {
        local: WorkspaceId,
        remote: WorkspaceId,
    },
    /// A nonempty target has no ownership manifest and cannot be adopted implicitly.
    RefuseMissingManifest,
    /// The target manifest cannot be trusted by this Brain version.
    RefuseInvalidManifest { error: ManifestError },
    /// The target listing proves a manifest exists, but it could not be read.
    RefuseUnreadableManifest { message: String },
}

/// What setup learned from the configured remote before making any write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteIdentityObservation {
    /// The target listed successfully and contained no files.
    Empty,
    /// The target contains files but no portable ownership manifest.
    ManifestlessNonempty,
    /// The target carries a schema-compatible portable manifest.
    CompatibleManifest { workspace_id: WorkspaceId },
    /// The target manifest cannot be trusted by this Brain version.
    InvalidManifest { error: ManifestError },
    /// The target lists an ownership manifest that rclone could not read.
    UnreadableManifest { message: String },
}

/// Explicit authority supplied only by `brain sync setup` after showing the
/// observed manifestless nonempty target to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestlessRemoteAdoption {
    Refuse,
    Authorized(WorkspaceId),
}

/// Compare one selected workspace ID with an optional remote owner.
#[must_use]
pub fn check_remote_identity(
    local: WorkspaceId,
    remote: Option<WorkspaceId>,
) -> RemoteIdentityDecision {
    match remote {
        None => RemoteIdentityDecision::Initialize,
        Some(remote) if remote == local => RemoteIdentityDecision::Proceed,
        Some(remote) => RemoteIdentityDecision::RefuseMismatch { local, remote },
    }
}

/// Parse remote manifest bytes and make the complete fail-closed identity decision.
#[must_use]
pub fn check_remote_manifest_identity(
    local: WorkspaceId,
    remote_manifest: Option<&[u8]>,
    remote_is_empty: bool,
    brain_version: &str,
) -> RemoteIdentityDecision {
    let observed = observe_remote_manifest(remote_manifest, remote_is_empty, brain_version);
    decision_from_observation(local, &observed)
}

fn observe_remote_manifest(
    remote_manifest: Option<&[u8]>,
    remote_is_empty: bool,
    brain_version: &str,
) -> RemoteIdentityObservation {
    let Some(bytes) = remote_manifest else {
        return if remote_is_empty {
            RemoteIdentityObservation::Empty
        } else {
            RemoteIdentityObservation::ManifestlessNonempty
        };
    };
    match WorkspaceManifest::parse(bytes, brain_version) {
        Ok(manifest) => RemoteIdentityObservation::CompatibleManifest {
            workspace_id: manifest.workspace_id(),
        },
        Err(error) => RemoteIdentityObservation::InvalidManifest { error },
    }
}

fn decision_from_observation(
    local: WorkspaceId,
    observed: &RemoteIdentityObservation,
) -> RemoteIdentityDecision {
    match observed {
        RemoteIdentityObservation::Empty => RemoteIdentityDecision::Initialize,
        RemoteIdentityObservation::ManifestlessNonempty => {
            RemoteIdentityDecision::RefuseMissingManifest
        }
        RemoteIdentityObservation::CompatibleManifest { workspace_id } => {
            check_remote_identity(local, Some(*workspace_id))
        }
        RemoteIdentityObservation::InvalidManifest { error } => {
            RemoteIdentityDecision::RefuseInvalidManifest {
                error: error.clone(),
            }
        }
        RemoteIdentityObservation::UnreadableManifest { message } => {
            RemoteIdentityDecision::RefuseUnreadableManifest {
                message: message.clone(),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteCommandOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: String,
}

/// A configured remote whose manifest matched the selected workspace.
#[derive(Debug, Clone, Copy)]
pub struct VerifiedRemote<'a> {
    remote: &'a Remote,
}

impl VerifiedRemote<'_> {
    /// The identity-checked rclone remote.
    #[must_use]
    pub const fn remote(&self) -> &Remote {
        self.remote
    }
}

fn remote_manifest_arg(remote_root: &str) -> String {
    format!("{}/{REMOTE_MANIFEST}", remote_root.trim_end_matches('/'))
}

pub(crate) fn validate_local_manifest(
    root: &Path,
    expected_id: WorkspaceId,
) -> Result<WorkspaceManifest> {
    let manifest = WorkspaceManifest::load(root, env!("CARGO_PKG_VERSION"))
        .context("validate local workspace manifest")?;
    if manifest.workspace_id() != expected_id {
        bail!(
            "selected workspace UUID {expected_id} does not match local manifest UUID {}",
            manifest.workspace_id()
        );
    }
    Ok(manifest)
}

/// Require the configured remote to carry a compatible manifest for this workspace.
pub fn require_remote_identity<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
) -> Result<VerifiedRemote<'remote>> {
    require_remote_identity_with(root, expected_id, remote, run_remote_command)
}

fn require_remote_identity_with<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
    mut run: impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<VerifiedRemote<'remote>> {
    let manifest = validate_local_manifest(root, expected_id)?;
    let observed = probe_remote_identity_with(remote, &mut run)?;
    match decision_from_observation(manifest.workspace_id(), &observed) {
        RemoteIdentityDecision::Proceed => Ok(VerifiedRemote { remote }),
        RemoteIdentityDecision::Initialize => {
            bail!("remote workspace is not initialized; run `brain sync setup`")
        }
        refusal => refuse(refusal),
    }
}

#[cfg(test)]
pub(crate) const fn verified_remote_for_test(remote: &Remote) -> VerifiedRemote<'_> {
    VerifiedRemote { remote }
}

/// During setup, publish the existing local manifest only when the remote is empty.
pub fn ensure_remote_identity_for_setup<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
) -> Result<VerifiedRemote<'remote>> {
    ensure_remote_identity_for_setup_with(
        root,
        expected_id,
        remote,
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        run_remote_command,
    )
}

/// Inspect setup's configured target before making any remote write.
///
/// The callback presents the exact observation and collects any manifestless
/// adoption authority. Initialization then publishes and verifies the existing
/// local manifest.
pub fn ensure_remote_identity_for_setup_with_authorization<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
    authorize: impl FnOnce(&RemoteIdentityObservation) -> Result<ManifestlessRemoteAdoption>,
) -> Result<VerifiedRemote<'remote>> {
    ensure_remote_identity_for_setup_with(root, expected_id, remote, authorize, run_remote_command)
}

fn ensure_remote_identity_for_setup_with<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
    authorize: impl FnOnce(&RemoteIdentityObservation) -> Result<ManifestlessRemoteAdoption>,
    mut run: impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<VerifiedRemote<'remote>> {
    let manifest = validate_local_manifest(root, expected_id)?;
    let observed = probe_remote_identity_with(remote, &mut run)?;
    let adoption = authorize(&observed)?;
    match decision_from_observation(manifest.workspace_id(), &observed) {
        RemoteIdentityDecision::Proceed => return Ok(VerifiedRemote { remote }),
        RemoteIdentityDecision::Initialize => {}
        RemoteIdentityDecision::RefuseMissingManifest
            if adoption == ManifestlessRemoteAdoption::Authorized(manifest.workspace_id()) => {}
        RemoteIdentityDecision::RefuseMissingManifest
            if matches!(adoption, ManifestlessRemoteAdoption::Authorized(_)) =>
        {
            let ManifestlessRemoteAdoption::Authorized(authorized) = adoption else {
                unreachable!("guard requires authorized adoption")
            };
            bail!(
                "remote adoption authority UUID {authorized} does not match selected workspace UUID {}",
                manifest.workspace_id()
            );
        }
        refusal => return refuse(refusal),
    }

    let local_path = WorkspaceManifest::path(root);
    let winner = claim::register_and_elect(&local_path, manifest.workspace_id(), remote, &mut run)?;
    if winner != manifest.workspace_id() {
        bail!(
            "remote workspace ownership claim was won by UUID {winner}; selected workspace UUID {} was not published",
            manifest.workspace_id()
        );
    }

    let established = probe_remote_identity_with(remote, &mut run)?;
    match decision_from_observation(manifest.workspace_id(), &established) {
        RemoteIdentityDecision::Proceed => return Ok(VerifiedRemote { remote }),
        RemoteIdentityDecision::Initialize => {}
        RemoteIdentityDecision::RefuseMissingManifest
            if adoption == ManifestlessRemoteAdoption::Authorized(manifest.workspace_id()) => {}
        refusal => return refuse(refusal),
    }

    let remote_path = remote_manifest_arg(&remote.arg);
    let publish_args = vec![
        "copyto".to_owned(),
        local_path.to_string_lossy().into_owned(),
        remote_path.clone(),
        "--immutable".to_owned(),
    ];
    let published = run(&remote.env, &publish_args);
    if !published.success {
        bail!(
            "could not publish remote workspace manifest: {}",
            published.stderr.trim()
        );
    }

    let readback_args = vec!["cat".to_owned(), remote_path];
    let readback = run(&remote.env, &readback_args);
    if !readback.success {
        bail!(
            "could not verify published remote workspace manifest: {}",
            readback.stderr.trim()
        );
    }
    match check_remote_manifest_identity(
        manifest.workspace_id(),
        Some(&readback.stdout),
        false,
        env!("CARGO_PKG_VERSION"),
    ) {
        RemoteIdentityDecision::Proceed => Ok(VerifiedRemote { remote }),
        decision => refuse_publication(decision),
    }
}

fn probe_remote_identity_with(
    remote: &Remote,
    run: &mut impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<RemoteIdentityObservation> {
    let manifest_path = remote_manifest_arg(&remote.arg);
    let cat_args = vec!["cat".to_owned(), manifest_path];
    let manifest = run(&remote.env, &cat_args);
    if manifest.success {
        return Ok(observe_remote_manifest(
            Some(&manifest.stdout),
            false,
            env!("CARGO_PKG_VERSION"),
        ));
    }

    let list_args = vec![
        "lsf".to_owned(),
        remote.arg.clone(),
        "--recursive".to_owned(),
        "--files-only".to_owned(),
    ];
    let listing = run(&remote.env, &list_args);
    if !listing.success {
        bail!(
            "could not inspect remote workspace identity: {}",
            listing.stderr.trim()
        );
    }
    if listing_contains_manifest(&listing.stdout) {
        return Ok(RemoteIdentityObservation::UnreadableManifest {
            message: manifest.stderr.trim().to_owned(),
        });
    }
    let empty = listing
        .stdout
        .split(|byte| *byte == b'\n')
        .all(|line| line.iter().all(u8::is_ascii_whitespace) || claim::is_claim_path(line));
    Ok(observe_remote_manifest(
        None,
        empty,
        env!("CARGO_PKG_VERSION"),
    ))
}

fn listing_contains_manifest(listing: &[u8]) -> bool {
    listing.split(|byte| *byte == b'\n').any(|line| {
        let start = line
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(line.len());
        let end = line
            .iter()
            .rposition(|byte| !byte.is_ascii_whitespace())
            .map_or(start, |index| index + 1);
        &line[start..end] == REMOTE_MANIFEST.as_bytes()
    })
}

fn refuse<T>(decision: RemoteIdentityDecision) -> Result<T> {
    match decision {
        RemoteIdentityDecision::RefuseMismatch { local, remote } => {
            bail!("remote workspace UUID {remote} does not match selected workspace UUID {local}")
        }
        RemoteIdentityDecision::RefuseMissingManifest => {
            bail!("remote target has data but no workspace manifest; implicit adoption is refused")
        }
        RemoteIdentityDecision::RefuseInvalidManifest { error } => {
            bail!("remote workspace manifest is invalid or incompatible: {error}")
        }
        RemoteIdentityDecision::RefuseUnreadableManifest { message } => {
            bail!("remote workspace manifest is present but unreadable: {message}")
        }
        RemoteIdentityDecision::Initialize | RemoteIdentityDecision::Proceed => {
            bail!("remote identity decision was used in the wrong sync phase")
        }
    }
}

fn refuse_publication<T>(decision: RemoteIdentityDecision) -> Result<T> {
    refuse(decision).context("published remote workspace manifest failed read-back verification")
}

fn run_remote_command(env: &[(String, String)], args: &[String]) -> RemoteCommandOutput {
    crate::logging::log(format!(
        "spawn rclone identity args={args:?} env_keys={}",
        env.len()
    ));
    let mut command = Command::new("rclone");
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    match command.output() {
        Ok(output) => RemoteCommandOutput {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => RemoteCommandOutput {
            success: false,
            stdout: Vec::new(),
            stderr: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};

    use super::*;
    use crate::sync::remote::Remote;

    const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
    const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
    const INGRESS_ID: &str = "c48b0de2-361d-43aa-8e7d-9a60ba6caf39";

    fn workspace_id(raw: &str) -> WorkspaceId {
        WorkspaceId::parse(raw).expect("fixed workspace UUID")
    }

    fn manifest_bytes(id: &str) -> Vec<u8> {
        format!(
            "{{\n  \"schema_version\": 1,\n  \"workspace_id\": \"{id}\",\n  \"receiver_ingress_id\": \"{INGRESS_ID}\",\n  \"minimum_brain_version\": \"0.1.0\"\n}}\n"
        )
        .into_bytes()
    }

    fn write_manifest(root: &Path, bytes: &[u8]) {
        let path = WorkspaceManifest::path(root);
        std::fs::create_dir_all(path.parent().expect("manifest parent")).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn remote() -> Remote {
        Remote {
            env: vec![("RCLONE_CONFIG_BRAIN_TYPE".to_owned(), "b2".to_owned())],
            arg: "BRAIN:shared/brain".to_owned(),
        }
    }

    fn output(success: bool, stdout: &[u8], stderr: &str) -> RemoteCommandOutput {
        RemoteCommandOutput {
            success,
            stdout: stdout.to_vec(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn setup_publishes_the_existing_local_manifest_first_and_verifies_readback() {
        let root = tempfile::tempdir().unwrap();
        let bytes = manifest_bytes(PERSONAL_ID);
        write_manifest(root.path(), &bytes);
        let calls = RefCell::new(Vec::<Vec<String>>::new());
        let mut step = 0;

        ensure_remote_identity_for_setup_with(
            root.path(),
            workspace_id(PERSONAL_ID),
            &remote(),
            |_| Ok(ManifestlessRemoteAdoption::Refuse),
            |_, args| {
                calls.borrow_mut().push(args.to_vec());
                let response = match step {
                    0 | 2 | 7 => output(false, b"", "object not found"),
                    1 | 3 | 8 | 9 => output(true, b"", ""),
                    4 | 6 | 10 => output(true, &bytes, ""),
                    5 => output(true, format!("{PERSONAL_ID}.json\n").as_bytes(), ""),
                    _ => panic!("unexpected remote command"),
                };
                step += 1;
                response
            },
        )
        .unwrap();

        let calls = calls.into_inner();
        assert_eq!(
            &calls[0][..2],
            ["cat", "BRAIN:shared/brain/.config/workspace.json"]
        );
        assert_eq!(calls[1][0], "lsf");
        assert_eq!(
            calls[3],
            [
                "copyto",
                WorkspaceManifest::path(root.path())
                    .to_string_lossy()
                    .as_ref(),
                &format!("BRAIN:shared/brain/.config/workspace-claims/{PERSONAL_ID}.json"),
                "--immutable",
            ]
        );
        assert_eq!(
            &calls[9][..3],
            [
                "copyto",
                WorkspaceManifest::path(root.path())
                    .to_string_lossy()
                    .as_ref(),
                "BRAIN:shared/brain/.config/workspace.json",
            ]
        );
        assert_eq!(
            &calls[10][..2],
            ["cat", "BRAIN:shared/brain/.config/workspace.json"]
        );
        assert_eq!(
            std::fs::read(WorkspaceManifest::path(root.path())).unwrap(),
            bytes
        );
    }

    #[test]
    fn concurrent_empty_setup_elects_one_claim_without_overwriting_the_manifest() {
        #[derive(Default)]
        struct RemoteState {
            manifest: Option<Vec<u8>>,
            claims: BTreeMap<String, Vec<u8>>,
            manifest_publications: usize,
        }

        let personal = tempfile::tempdir().unwrap();
        let family = tempfile::tempdir().unwrap();
        write_manifest(personal.path(), &manifest_bytes(PERSONAL_ID));
        write_manifest(family.path(), &manifest_bytes(FAMILY_ID));
        let state = Arc::new(Mutex::new(RemoteState::default()));
        let empty_probe_barrier = Arc::new(Barrier::new(2));
        let claim_publish_barrier = Arc::new(Barrier::new(2));
        let root_listings = Arc::new(AtomicUsize::new(0));

        let results = std::thread::scope(|scope| {
            let launch = |root: &Path, id: WorkspaceId| {
                let root = root.to_path_buf();
                let state = Arc::clone(&state);
                let empty_probe_barrier = Arc::clone(&empty_probe_barrier);
                let claim_publish_barrier = Arc::clone(&claim_publish_barrier);
                let root_listings = Arc::clone(&root_listings);
                scope.spawn(move || {
                    ensure_remote_identity_for_setup_with(
                        &root,
                        id,
                        &remote(),
                        |_| Ok(ManifestlessRemoteAdoption::Refuse),
                        |_, args| match args.first().map(String::as_str) {
                            Some("cat") => {
                                let target = args.get(1).expect("cat target");
                                let state = state.lock().unwrap();
                                if target.ends_with(REMOTE_MANIFEST) {
                                    state.manifest.as_ref().map_or_else(
                                        || output(false, b"", "object not found"),
                                        |bytes| output(true, bytes, ""),
                                    )
                                } else {
                                    let name = target.rsplit('/').next().unwrap_or_default();
                                    state.claims.get(name).map_or_else(
                                        || output(false, b"", "object not found"),
                                        |bytes| output(true, bytes, ""),
                                    )
                                }
                            }
                            Some("lsf") => {
                                let target = args.get(1).expect("lsf target");
                                if target.ends_with("/.config/workspace-claims") {
                                    let listing = state.lock().unwrap().claims.keys().fold(
                                        String::new(),
                                        |mut listing, name| {
                                            writeln!(listing, "{name}").unwrap();
                                            listing
                                        },
                                    );
                                    output(true, listing.as_bytes(), "")
                                } else {
                                    if root_listings.fetch_add(1, Ordering::SeqCst) < 2 {
                                        empty_probe_barrier.wait();
                                    }
                                    output(true, b"", "")
                                }
                            }
                            Some("copyto") => {
                                let source = args.get(1).expect("copy source");
                                let target = args.get(2).expect("copy target");
                                let bytes = std::fs::read(source).unwrap();
                                if target.contains("/.config/workspace-claims/") {
                                    let name = target.rsplit('/').next().unwrap().to_owned();
                                    state.lock().unwrap().claims.insert(name, bytes);
                                    claim_publish_barrier.wait();
                                } else {
                                    let mut state = state.lock().unwrap();
                                    state.manifest_publications += 1;
                                    state.manifest = Some(bytes);
                                }
                                output(true, b"", "")
                            }
                            command => panic!("unexpected remote command: {command:?} {args:?}"),
                        },
                    )
                    .map(|_| ())
                })
            };
            let personal = launch(personal.path(), workspace_id(PERSONAL_ID));
            let family = launch(family.path(), workspace_id(FAMILY_ID));
            [personal.join().unwrap(), family.join().unwrap()]
        });

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let (manifest_publications, manifest) = {
            let state = state.lock().unwrap();
            (state.manifest_publications, state.manifest.clone())
        };
        assert_eq!(manifest_publications, 1);
        assert_eq!(
            manifest.as_deref(),
            Some(manifest_bytes(PERSONAL_ID).as_slice()),
            "the deterministic lowest UUID claim owns the remote"
        );
    }

    #[test]
    fn setup_refuses_a_mismatched_remote_before_any_publication() {
        let root = tempfile::tempdir().unwrap();
        write_manifest(root.path(), &manifest_bytes(PERSONAL_ID));
        let remote_bytes = manifest_bytes(FAMILY_ID);
        let calls = RefCell::new(Vec::<Vec<String>>::new());

        let error = ensure_remote_identity_for_setup_with(
            root.path(),
            workspace_id(PERSONAL_ID),
            &remote(),
            |_| Ok(ManifestlessRemoteAdoption::Refuse),
            |_, args| {
                calls.borrow_mut().push(args.to_vec());
                output(true, &remote_bytes, "")
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains(PERSONAL_ID), "{error:#}");
        assert!(error.to_string().contains(FAMILY_ID), "{error:#}");
        assert_eq!(
            calls.into_inner().len(),
            1,
            "mismatch must stop before copyto"
        );
    }

    #[test]
    fn setup_refuses_a_nonempty_manifestless_remote_without_publication() {
        let root = tempfile::tempdir().unwrap();
        write_manifest(root.path(), &manifest_bytes(PERSONAL_ID));
        let calls = RefCell::new(Vec::<Vec<String>>::new());
        let mut step = 0;

        let error = ensure_remote_identity_for_setup_with(
            root.path(),
            workspace_id(PERSONAL_ID),
            &remote(),
            |_| Ok(ManifestlessRemoteAdoption::Refuse),
            |_, args| {
                calls.borrow_mut().push(args.to_vec());
                let response = match step {
                    0 => output(false, b"", "object not found"),
                    1 => output(true, b"notes.md\n", ""),
                    _ => panic!("manifestless nonempty remote must stop before publication"),
                };
                step += 1;
                response
            },
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("has data but no workspace manifest"),
            "{error:#}"
        );
        assert_eq!(calls.into_inner().len(), 2);
    }

    #[test]
    fn setup_adopts_a_nonempty_manifestless_remote_only_with_exact_authority() {
        let root = tempfile::tempdir().unwrap();
        let bytes = manifest_bytes(PERSONAL_ID);
        write_manifest(root.path(), &bytes);
        let calls = RefCell::new(Vec::<Vec<String>>::new());
        let observations = RefCell::new(Vec::new());
        let mut step = 0;
        let remote = remote();

        let verified = ensure_remote_identity_for_setup_with(
            root.path(),
            workspace_id(PERSONAL_ID),
            &remote,
            |observed| {
                observations.borrow_mut().push(observed.clone());
                Ok(ManifestlessRemoteAdoption::Authorized(workspace_id(
                    PERSONAL_ID,
                )))
            },
            |_, args| {
                calls.borrow_mut().push(args.to_vec());
                let response = match step {
                    0 | 2 | 7 => output(false, b"", "object not found"),
                    1 | 8 => output(true, b"notes.md\n", ""),
                    3 | 9 => output(true, b"", ""),
                    4 | 6 | 10 => output(true, &bytes, ""),
                    5 => output(true, format!("{PERSONAL_ID}.json\n").as_bytes(), ""),
                    _ => panic!("unexpected remote command"),
                };
                step += 1;
                response
            },
        )
        .expect("exact authority adopts the target");

        assert_eq!(verified.remote(), &remote);
        assert_eq!(
            observations.into_inner(),
            [RemoteIdentityObservation::ManifestlessNonempty]
        );
        let calls = calls.into_inner();
        assert_eq!(calls[0][0], "cat");
        assert_eq!(calls[1][0], "lsf");
        assert_eq!(calls[3][0], "copyto");
        assert_eq!(calls[9][0], "copyto");
        assert_eq!(calls[10][0], "cat");
    }

    #[test]
    fn setup_never_adopts_when_the_listing_contains_an_unreadable_manifest() {
        let root = tempfile::tempdir().unwrap();
        write_manifest(root.path(), &manifest_bytes(PERSONAL_ID));
        let calls = RefCell::new(Vec::<Vec<String>>::new());
        let observations = RefCell::new(Vec::new());
        let mut step = 0;

        let error = ensure_remote_identity_for_setup_with(
            root.path(),
            workspace_id(PERSONAL_ID),
            &remote(),
            |observed| {
                observations.borrow_mut().push(observed.clone());
                Ok(ManifestlessRemoteAdoption::Authorized(workspace_id(
                    PERSONAL_ID,
                )))
            },
            |_, args| {
                calls.borrow_mut().push(args.to_vec());
                let response = match step {
                    0 => output(false, b"", "temporary read failure"),
                    1 => output(true, b".config/workspace.json\nnotes.md\n", ""),
                    _ => panic!("an unreadable manifest must stop before publication"),
                };
                step += 1;
                response
            },
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("manifest is present but unreadable")
        );
        assert!(error.to_string().contains("temporary read failure"));
        assert!(matches!(
            observations.into_inner().as_slice(),
            [RemoteIdentityObservation::UnreadableManifest { .. }]
        ));
        assert_eq!(calls.into_inner().len(), 2);
    }

    #[test]
    fn ordinary_gate_refuses_an_empty_uninitialized_remote() {
        let root = tempfile::tempdir().unwrap();
        write_manifest(root.path(), &manifest_bytes(PERSONAL_ID));
        let mut step = 0;

        let error = require_remote_identity_with(
            root.path(),
            workspace_id(PERSONAL_ID),
            &remote(),
            |_, _| {
                let response = match step {
                    0 => output(false, b"", "object not found"),
                    1 => output(true, b"", ""),
                    _ => panic!("ordinary gate must never initialize"),
                };
                step += 1;
                response
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("not initialized"), "{error:#}");
    }

    #[test]
    fn local_validation_refuses_record_manifest_mismatch_without_rewriting_bytes() {
        let root = tempfile::tempdir().unwrap();
        let bytes = manifest_bytes(FAMILY_ID);
        write_manifest(root.path(), &bytes);

        let error = validate_local_manifest(root.path(), workspace_id(PERSONAL_ID)).unwrap_err();

        assert!(error.to_string().contains(PERSONAL_ID), "{error:#}");
        assert!(error.to_string().contains(FAMILY_ID), "{error:#}");
        assert_eq!(
            std::fs::read(WorkspaceManifest::path(root.path())).unwrap(),
            bytes
        );
    }
}

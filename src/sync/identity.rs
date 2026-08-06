//! Workspace identity decisions and the shared rclone manifest gate.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::sync::remote::Remote;
use crate::workspace::{ManifestError, WorkspaceId, WorkspaceManifest};

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
    let Some(bytes) = remote_manifest else {
        return if remote_is_empty {
            RemoteIdentityDecision::Initialize
        } else {
            RemoteIdentityDecision::RefuseMissingManifest
        };
    };
    match WorkspaceManifest::parse(bytes, brain_version) {
        Ok(manifest) => check_remote_identity(local, Some(manifest.workspace_id())),
        Err(error) => RemoteIdentityDecision::RefuseInvalidManifest { error },
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
    match probe_remote_identity_with(manifest.workspace_id(), remote, &mut run)? {
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
pub fn ensure_remote_identity_for_setup(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &Remote,
) -> Result<()> {
    ensure_remote_identity_for_setup_with(root, expected_id, remote, run_remote_command)
}

fn ensure_remote_identity_for_setup_with(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &Remote,
    mut run: impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<()> {
    let manifest = validate_local_manifest(root, expected_id)?;
    match probe_remote_identity_with(manifest.workspace_id(), remote, &mut run)? {
        RemoteIdentityDecision::Proceed => return Ok(()),
        RemoteIdentityDecision::Initialize => {}
        refusal => return refuse(refusal),
    }

    let local_path = WorkspaceManifest::path(root);
    let remote_path = remote_manifest_arg(&remote.arg);
    let publish_args = vec![
        "copyto".to_owned(),
        local_path.to_string_lossy().into_owned(),
        remote_path.clone(),
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
        RemoteIdentityDecision::Proceed => Ok(()),
        decision => refuse_publication(decision),
    }
}

fn probe_remote_identity_with(
    local_id: WorkspaceId,
    remote: &Remote,
    run: &mut impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<RemoteIdentityDecision> {
    let manifest_path = remote_manifest_arg(&remote.arg);
    let cat_args = vec!["cat".to_owned(), manifest_path];
    let manifest = run(&remote.env, &cat_args);
    if manifest.success {
        return Ok(check_remote_manifest_identity(
            local_id,
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
    Ok(check_remote_manifest_identity(
        local_id,
        None,
        listing.stdout.iter().all(u8::is_ascii_whitespace),
        env!("CARGO_PKG_VERSION"),
    ))
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
        RemoteIdentityDecision::Initialize | RemoteIdentityDecision::Proceed => {
            bail!("remote identity decision was used in the wrong sync phase")
        }
    }
}

fn refuse_publication(decision: RemoteIdentityDecision) -> Result<()> {
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
    use std::path::Path;

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
            |_, args| {
                calls.borrow_mut().push(args.to_vec());
                let response = match step {
                    0 => output(false, b"", "object not found"),
                    1 | 2 => output(true, b"", ""),
                    3 => output(true, &bytes, ""),
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
            calls[2],
            [
                "copyto",
                WorkspaceManifest::path(root.path())
                    .to_string_lossy()
                    .as_ref(),
                "BRAIN:shared/brain/.config/workspace.json",
            ]
        );
        assert_eq!(
            &calls[3][..2],
            ["cat", "BRAIN:shared/brain/.config/workspace.json"]
        );
        assert_eq!(
            std::fs::read(WorkspaceManifest::path(root.path())).unwrap(),
            bytes
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

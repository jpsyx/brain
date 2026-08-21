use anyhow::{Result, bail};

use crate::sync::remote::Remote;

use super::{REMOTE_MANIFEST, RemoteIdentityObservation, claim, observe_remote_manifest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RemoteCommandOutput {
    pub(super) success: bool,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: String,
}

pub(super) fn remote_manifest_arg(remote_root: &str) -> String {
    format!("{}/{REMOTE_MANIFEST}", remote_root.trim_end_matches('/'))
}

pub(super) fn probe_remote_identity_with(
    remote: &Remote,
    run: &mut impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<RemoteIdentityObservation> {
    let manifest_path = remote_manifest_arg(&remote.arg);
    let cat_args = vec!["cat".to_owned(), manifest_path];
    let manifest = run(&remote.env, &cat_args);
    // A successful read of *nothing* is not a manifest. Some backends exit 0
    // with empty output when the object does not exist, and treating that as a
    // manifest reports a pristine bucket as corrupt and refuses every sync,
    // including the first one that would have created it. Bytes that are
    // present but malformed still fail closed below: those could be a damaged
    // ownership claim, while no bytes claim nothing.
    if manifest.success && !manifest.stdout.iter().all(u8::is_ascii_whitespace) {
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

pub(super) fn listing_contains_manifest(listing: &[u8]) -> bool {
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

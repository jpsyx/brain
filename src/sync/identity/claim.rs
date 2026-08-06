//! Append-only remote ownership claims used only while setup initializes a target.

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::RemoteCommandOutput;
use crate::sync::remote::Remote;
use crate::workspace::{WorkspaceId, WorkspaceManifest};

const CLAIMS_DIRECTORY: &str = ".config/workspace-claims";

pub(super) fn register_and_elect(
    local_manifest: &Path,
    local_id: WorkspaceId,
    remote: &Remote,
    run: &mut impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<WorkspaceId> {
    let expected = std::fs::read(local_manifest)
        .with_context(|| format!("reading local ownership claim {}", local_manifest.display()))?;
    let claim = claim_arg(&remote.arg, local_id);
    ensure_claim(&remote.env, local_manifest, &claim, &expected, run)?;

    let listing = run(
        &remote.env,
        &[
            "lsf".to_owned(),
            claims_directory_arg(&remote.arg),
            "--files-only".to_owned(),
        ],
    );
    if !listing.success {
        bail!(
            "could not enumerate remote workspace ownership claims: {}",
            listing.stderr.trim()
        );
    }
    let mut claimants = parse_claimants(&listing.stdout)?;
    claimants.sort_unstable_by_key(ToString::to_string);
    claimants.dedup();
    let winner = claimants
        .first()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("remote ownership claim disappeared after publication"))?;
    validate_claim(&remote.env, &remote.arg, winner, run)?;
    Ok(winner)
}

pub(super) fn is_claim_path(path: &[u8]) -> bool {
    let path = trim_ascii(path);
    path.starts_with(format!("{CLAIMS_DIRECTORY}/").as_bytes())
}

fn ensure_claim(
    env: &[(String, String)],
    local_manifest: &Path,
    remote_claim: &str,
    expected: &[u8],
    run: &mut impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<()> {
    let existing = run(env, &["cat".to_owned(), remote_claim.to_owned()]);
    if existing.success {
        return exact_claim(expected, &existing.stdout);
    }

    let published = run(
        env,
        &[
            "copyto".to_owned(),
            local_manifest.to_string_lossy().into_owned(),
            remote_claim.to_owned(),
            "--immutable".to_owned(),
        ],
    );
    let readback = run(env, &["cat".to_owned(), remote_claim.to_owned()]);
    if !readback.success {
        let publication = published.stderr.trim();
        let verification = readback.stderr.trim();
        bail!(
            "could not publish or verify remote workspace ownership claim: publication={publication}; verification={verification}"
        );
    }
    exact_claim(expected, &readback.stdout)
}

fn exact_claim(expected: &[u8], observed: &[u8]) -> Result<()> {
    if observed != expected {
        bail!("remote workspace ownership claim does not match the local manifest");
    }
    Ok(())
}

fn validate_claim(
    env: &[(String, String)],
    remote_root: &str,
    claimant: WorkspaceId,
    run: &mut impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<()> {
    let path = claim_arg(remote_root, claimant);
    let output = run(env, &["cat".to_owned(), path]);
    if !output.success {
        bail!(
            "elected remote workspace ownership claim is unreadable: {}",
            output.stderr.trim()
        );
    }
    let manifest = WorkspaceManifest::parse(&output.stdout, env!("CARGO_PKG_VERSION"))
        .context("elected remote workspace ownership claim is invalid or incompatible")?;
    if manifest.workspace_id() != claimant {
        bail!(
            "remote workspace ownership claim filename UUID {claimant} does not match its manifest UUID {}",
            manifest.workspace_id()
        );
    }
    Ok(())
}

fn parse_claimants(listing: &[u8]) -> Result<Vec<WorkspaceId>> {
    listing
        .split(|byte| *byte == b'\n')
        .map(trim_ascii)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let name = std::str::from_utf8(line).context("ownership claim name is not UTF-8")?;
            let raw = name.strip_suffix(".json").ok_or_else(|| {
                anyhow::anyhow!("invalid remote workspace ownership claim name {name}")
            })?;
            WorkspaceId::parse(raw)
                .with_context(|| format!("invalid remote workspace ownership claim UUID {raw}"))
        })
        .collect()
}

fn claims_directory_arg(remote_root: &str) -> String {
    format!("{}/{CLAIMS_DIRECTORY}", remote_root.trim_end_matches('/'))
}

fn claim_arg(remote_root: &str, id: WorkspaceId) -> String {
    format!("{}/{}.json", claims_directory_arg(remote_root), id)
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::parse_claimants;

    #[test]
    fn claim_listing_is_strict_and_uuid_orderable() {
        let claims = parse_claimants(
            b"e806258e-491a-436d-9db4-a5ca9903e0d4.json\n8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b.json\n",
        )
        .unwrap();
        assert_eq!(claims.len(), 2);
        assert!(parse_claimants(b"not-a-uuid.json\n").is_err());
        assert!(parse_claimants(b"8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b.txt\n").is_err());
    }
}

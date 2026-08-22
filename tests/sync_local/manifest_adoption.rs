//! A second machine joining a workspace that is already synced.
//!
//! The portable manifest is excluded from bisync and every other identity write
//! publishes upward, so nothing brought it down: a joining machine had no way to
//! obtain the identity its first sync demands. Exercised through real rclone, so
//! the blank-read and transport behavior are the ones B2 actually shows.

use brain::sync::identity::{ManifestAdoption, adopt_remote_manifest};
use brain::sync::remote::Remote;
use brain::workspace::{WorkspaceId, WorkspaceManifest};

use super::rclone_available;

fn local_remote(path: &std::path::Path) -> Remote {
    Remote {
        env: Vec::new(),
        arg: path.to_string_lossy().into_owned(),
    }
}

#[test]
fn a_joining_machine_adopts_the_remote_identity_including_its_ingress_id() {
    if !rclone_available() {
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let remote_root = base.path().join("remote");
    let joining_root = base.path().join("joined");
    std::fs::create_dir_all(&remote_root).unwrap();
    std::fs::create_dir_all(&joining_root).unwrap();

    // The workspace as it exists on the machine that created it.
    let established = WorkspaceManifest::new(WorkspaceId::new());
    established.write_new(&remote_root).unwrap();

    let adoption = adopt_remote_manifest(
        &joining_root,
        established.workspace_id(),
        &local_remote(&remote_root),
    )
    .expect("adopt the established identity");

    assert_eq!(adoption, ManifestAdoption::Adopted);
    let adopted = WorkspaceManifest::load(&joining_root, env!("CARGO_PKG_VERSION"))
        .expect("adopted manifest is loadable");
    assert_eq!(adopted.workspace_id(), established.workspace_id());
    assert_eq!(
        adopted.receiver_ingress_id(),
        established.receiver_ingress_id(),
        "minting locally would fork this, and bisync never reconciles the manifest"
    );
}

#[test]
fn a_joining_machine_refuses_a_remote_owned_by_a_different_workspace() {
    if !rclone_available() {
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let remote_root = base.path().join("remote");
    let joining_root = base.path().join("joined");
    std::fs::create_dir_all(&remote_root).unwrap();
    std::fs::create_dir_all(&joining_root).unwrap();
    WorkspaceManifest::new(WorkspaceId::new())
        .write_new(&remote_root)
        .unwrap();

    let error = adopt_remote_manifest(
        &joining_root,
        WorkspaceId::new(),
        &local_remote(&remote_root),
    )
    .expect_err("a foreign workspace must be refused");

    assert!(error.to_string().contains("does not match"), "{error:#}");
    assert!(!WorkspaceManifest::path(&joining_root).exists());
}

#[test]
fn an_empty_remote_leaves_the_registry_uuid_as_the_fallback() {
    if !rclone_available() {
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let remote_root = base.path().join("remote");
    let joining_root = base.path().join("joined");
    std::fs::create_dir_all(&remote_root).unwrap();
    std::fs::create_dir_all(&joining_root).unwrap();

    let adoption = adopt_remote_manifest(
        &joining_root,
        WorkspaceId::new(),
        &local_remote(&remote_root),
    )
    .expect("an empty remote is not an error");

    assert_eq!(adoption, ManifestAdoption::RemoteHasNoManifest);
    assert!(!WorkspaceManifest::path(&joining_root).exists());
}

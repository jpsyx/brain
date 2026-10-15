#[allow(dead_code, unused_imports)]
#[path = "../receiver_workspace_support/mod.rs"]
mod receiver_workspace_support;
#[path = "../status_read_only/snapshot.rs"]
mod snapshot;
#[path = "../status_read_only/receiver_observer.rs"]
mod receiver_observer;

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sha2::{Digest as _, Sha256};

use receiver_workspace_support::DualWorkspaceReceiverFixture;
use snapshot::{snapshot, snapshot_entry};

fn assert_receiver_state_unchanged(
    before: &snapshot::Entry,
    after: &snapshot::Entry,
    ids: &[brain::workspace::WorkspaceId],
) {
    assert_eq!(receiver_observer::compare(before, after, ids), Ok(()),
        "receiver observation changed state outside SQLite coordination paths");
}

fn selected_status_workspace() -> brain::workspace::WorkspaceId {
    brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap()
}

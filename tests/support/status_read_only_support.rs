#[allow(dead_code, unused_imports)]
#[path = "../receiver_workspace_support/mod.rs"]
mod receiver_workspace_support;
#[path = "../status_read_only/snapshot.rs"]
mod snapshot;

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sha2::{Digest as _, Sha256};

use receiver_workspace_support::DualWorkspaceReceiverFixture;
use snapshot::{snapshot, snapshot_entry};

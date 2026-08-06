use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use brain::tasks::identity::{CsvKind, legacy_task_uuid};
use brain::workspace::{WorkspaceId, WorkspaceManifest};

use super::rclone_available;

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

include!("coordinator_join_parts/part_01.rs");
include!("coordinator_join_parts/part_02.rs");
include!("coordinator_join_parts/part_03.rs");
include!("coordinator_join_parts/part_04.rs");

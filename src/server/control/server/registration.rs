//! Filesystem-backed registration validation for one live workspace TUI.

use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};

use super::ControlServer;
use crate::server::control::LeaseRegistration;
use crate::server::lifecycle::{LEASE_TTL, WorkspaceLease};
use crate::workspace::{RegistryStore, WorkspaceManifest, WorkspaceName};

impl ControlServer {
    pub(super) fn validate_registration(
        &self,
        registration: &LeaseRegistration,
        now: Instant,
        deadline: Instant,
    ) -> Result<WorkspaceLease> {
        validate_registration_with(
            &self.registry_store,
            &self.runtime_home,
            registration,
            now,
            deadline,
        )
    }
}

pub(super) fn validate_registration_with(
    registry_store: &RegistryStore,
    runtime_home: &Path,
    registration: &LeaseRegistration,
    now: Instant,
    deadline: Instant,
) -> Result<WorkspaceLease> {
    let registry = RegistryStore::load_from(registry_store.path())
        .context("reopening the machine workspace registry")?;
    let selected = registry
        .select(Some(&registration.canonical_name))
        .context("selecting the registered canonical workspace")?;
    if selected.canonical_name().as_str() != registration.canonical_name {
        anyhow::bail!("workspace registration must use its canonical name");
    }
    let record = selected.record();
    if record.workspace_id != registration.workspace_id {
        anyhow::bail!("workspace registration UUID does not match the machine registry");
    }
    let authoritative_root = crate::workspace::normalize_root(&record.root, Path::new("/"))?;
    let resolved_root =
        crate::workspace::normalize_root(&registration.resolved_root, Path::new("/"))?;
    if authoritative_root != resolved_root {
        anyhow::bail!("workspace root changed after the TUI resolved it");
    }
    let manifest = WorkspaceManifest::load(&record.root, env!("CARGO_PKG_VERSION"))
        .context("reopening the registered workspace manifest")?;
    if manifest.workspace_id() != registration.workspace_id {
        anyhow::bail!("workspace manifest UUID does not match the machine registry");
    }
    if crate::server::lifecycle::IngressId::from(manifest.receiver_ingress_id())
        != registration.ingress_id
    {
        anyhow::bail!("workspace ingress UUID does not match its manifest");
    }
    let runtime_paths =
        crate::workspace::WorkspacePaths::new(runtime_home, registration.workspace_id);
    let expected_job_socket = runtime_paths.job_socket();
    if registration.job_socket != expected_job_socket {
        anyhow::bail!("job socket does not match the validated workspace");
    }
    validate_live_tui(&runtime_paths, registration.tui_pid, deadline)?;
    let expires_at = now
        .checked_add(LEASE_TTL)
        .context("lease expiry exceeds the monotonic clock range")?;
    Ok(WorkspaceLease {
        lease_id: registration.lease_id,
        workspace_id: registration.workspace_id,
        canonical_name: WorkspaceName::parse(&registration.canonical_name)?,
        ingress_id: registration.ingress_id,
        tui_pid: registration.tui_pid,
        job_socket: expected_job_socket,
        receiver_enabled: record.receiver_enabled,
        expires_at,
    })
}

pub(super) fn validate_background_with(
    registry_store: &RegistryStore,
    registration: &LeaseRegistration,
    now: Instant,
) -> Result<WorkspaceLease> {
    let registry = RegistryStore::load_from(registry_store.path())
        .context("reopening the machine workspace registry")?;
    let selected = registry
        .select(Some(&registration.canonical_name))
        .context("selecting the registered canonical workspace")?;
    let record = selected.record();
    if record.workspace_id != registration.workspace_id {
        anyhow::bail!("workspace registration UUID does not match the machine registry");
    }
    let manifest = WorkspaceManifest::load(&record.root, env!("CARGO_PKG_VERSION"))
        .context("reopening the registered workspace manifest")?;
    if manifest.workspace_id() != registration.workspace_id
        || crate::server::lifecycle::IngressId::from(manifest.receiver_ingress_id())
            != registration.ingress_id
    {
        anyhow::bail!("workspace ingress UUID does not match its manifest");
    }
    Ok(WorkspaceLease {
        lease_id: registration.lease_id,
        workspace_id: registration.workspace_id,
        canonical_name: WorkspaceName::parse(&registration.canonical_name)?,
        ingress_id: registration.ingress_id,
        tui_pid: 0,
        job_socket: std::path::PathBuf::new(),
        receiver_enabled: record.receiver_enabled,
        expires_at: now + std::time::Duration::from_secs(100 * 365 * 24 * 60 * 60),
    })
}

fn validate_live_tui(
    runtime_paths: &crate::workspace::WorkspacePaths,
    expected_pid: u32,
    deadline: Instant,
) -> Result<()> {
    let lock_pid = fs::read_to_string(runtime_paths.tui_lock())
        .context("reading the workspace TUI singleton")?
        .trim()
        .parse::<u32>()
        .context("parsing the workspace TUI singleton PID")?;
    if lock_pid != expected_pid || !crate::server::lifecycle::pid_alive(expected_pid) {
        anyhow::bail!("workspace TUI singleton does not match a live process");
    }
    crate::server::control::connect::connect_until(&runtime_paths.job_socket(), deadline)
        .context("connecting to the live workspace job listener")?;
    Ok(())
}

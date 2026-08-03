//! Command bootstrap policy and selected workspace construction.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use super::bootstrap_policy::{RegistryOnlyPromptOrder, registry_only_prompt_order};
use super::{
    BootstrapPolicy, InteractionMode, Invocation, ReadinessAction, ReadinessField, RegistryStore,
    WorkspaceContext, WorkspaceManifest, bootstrap_policy, invocation_for, readiness_action,
};

/// One ready selected workspace plus the machine registry capability.
#[derive(Debug, Clone)]
pub struct CommandContext {
    pub workspace: Arc<WorkspaceContext>,
    pub registry_store: RegistryStore,
}

/// Bootstrap capability returned to top-level dispatch.
#[derive(Debug, Clone)]
pub enum BootstrapContext {
    None,
    RegistryOnly(RegistryStore),
    Ready(CommandContext),
}

/// Bootstrap one real process invocation.
pub fn bootstrap(cli: &mut crate::cli::Cli) -> Result<BootstrapContext> {
    let policy = bootstrap_policy(invocation_for(cli));
    if matches!(
        policy,
        BootstrapPolicy::None | BootstrapPolicy::InternalNoPrompt
    ) {
        return Ok(BootstrapContext::None);
    }
    let store = RegistryStore::real();
    if policy == BootstrapPolicy::RegistryOnly {
        return registry_only_bootstrap_with(
            cli,
            store,
            crate::workspace::command::preflight_registry_only,
            |prepared| {
                let invocation = invocation_for(prepared);
                debug_assert_eq!(
                    registry_only_prompt_order(invocation),
                    Some(RegistryOnlyPromptOrder::BeforeMigration)
                );
                let should_migrate = matches!(
                    invocation,
                    Invocation::WorkspaceRemove | Invocation::WorkspaceRepair
                ) || (matches!(
                    invocation,
                    Invocation::WorkspaceCreate | Invocation::WorkspaceAttach
                ) && crate::env::registry_setup_needs_migration()?);
                if should_migrate {
                    crate::env::migrate_checked()?;
                }
                Ok(())
            },
        );
    }

    crate::env::migrate_checked()?;
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))?;
    let current_dir = std::env::current_dir().context("read current directory")?;
    let interaction = if std::io::stdin().is_terminal() {
        InteractionMode::Interactive
    } else {
        InteractionMode::NonInteractive
    };
    if interaction == InteractionMode::Interactive {
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("open /dev/tty for workspace readiness")?;
        let mut writer = tty.try_clone().context("clone readiness terminal")?;
        let mut reader = BufReader::new(tty);
        bootstrap_with_io(
            cli,
            store,
            &home,
            &current_dir,
            interaction,
            &mut reader,
            &mut writer,
        )
    } else {
        bootstrap_with_io(
            cli,
            store,
            &home,
            &current_dir,
            interaction,
            &mut std::io::empty(),
            &mut std::io::sink(),
        )
    }
}

/// Bootstrap against injected paths and terminal IO.
pub fn bootstrap_with_io(
    cli: &mut crate::cli::Cli,
    store: RegistryStore,
    home: &Path,
    current_dir: &Path,
    interaction: InteractionMode,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<BootstrapContext> {
    if bootstrap_policy(invocation_for(cli)) == BootstrapPolicy::RegistryOnly {
        crate::workspace::command::preflight_registry_only_with_io(
            cli,
            reader,
            writer,
            crate::theme::Theme::active(),
        )?;
        return Ok(BootstrapContext::RegistryOnly(store));
    }
    bootstrap_with_io_and_hook(
        cli,
        store,
        (home, current_dir),
        interaction,
        reader,
        writer,
        || Ok(()),
    )
}

fn registry_only_bootstrap_with(
    cli: &mut crate::cli::Cli,
    store: RegistryStore,
    preflight: impl FnOnce(&mut crate::cli::Cli) -> Result<()>,
    after_preflight: impl FnOnce(&crate::cli::Cli) -> Result<()>,
) -> Result<BootstrapContext> {
    preflight(cli)?;
    after_preflight(cli)?;
    Ok(BootstrapContext::RegistryOnly(store))
}

fn bootstrap_with_io_and_hook(
    cli: &crate::cli::Cli,
    store: RegistryStore,
    paths: (&Path, &Path),
    interaction: InteractionMode,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    after_readiness: impl FnOnce() -> Result<()>,
) -> Result<BootstrapContext> {
    let (home, current_dir) = paths;
    let policy = bootstrap_policy(invocation_for(cli));
    match policy {
        BootstrapPolicy::None | BootstrapPolicy::InternalNoPrompt => {
            return Ok(BootstrapContext::None);
        }
        BootstrapPolicy::RegistryOnly => return Ok(BootstrapContext::RegistryOnly(store)),
        BootstrapPolicy::ReadyWorkspace => {}
    }

    let registry = RegistryStore::load_from(store.path())?;
    let selected = registry.select(cli.brain.as_deref())?;
    let canonical_name = selected.canonical_name().clone();
    let workspace_id = selected.record().workspace_id;
    let record = selected.record().clone();
    if !selected.record().root.is_dir() {
        anyhow::bail!(
            "workspace root {} is unavailable; restore it or detach the workspace",
            selected.record().root.display()
        );
    }
    let manifest = WorkspaceManifest::load(&selected.record().root, env!("CARGO_PKG_VERSION"));
    let action = readiness_action(
        selected.canonical_name(),
        selected.record(),
        manifest,
        interaction,
    )?;
    match action {
        ReadinessAction::Ready(_) => {
            after_readiness()?;
            context_from_record(&store, canonical_name, &record, home, current_dir)
        }
        ReadinessAction::Prompt(fields) => {
            repair_interactively(
                &store,
                canonical_name.as_str(),
                workspace_id,
                &fields,
                reader,
                writer,
            )?;
            after_readiness()?;
            repaired_context(&store, &canonical_name, workspace_id, home, current_dir)
        }
    }
}

fn repair_interactively(
    store: &RegistryStore,
    selector: &str,
    expected_workspace_id: super::WorkspaceId,
    fields: &[ReadinessField],
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<()> {
    let local_user_id = if fields.contains(&ReadinessField::LocalUserId) {
        Some(super::command::prompt::read_required(
            writer,
            reader,
            super::command::prompt::PromptField::LocalUserId,
            crate::theme::Theme::active(),
        )?)
    } else {
        None
    };
    store.transaction(|transaction| -> Result<()> {
        let mut registry = transaction.load()?;
        let selected = registry.select(Some(selector))?;
        if selected.record().workspace_id != expected_workspace_id {
            anyhow::bail!("selected workspace identity changed during readiness repair");
        }
        let canonical_name = selected.canonical_name().clone();
        let root = selected.record().root.clone();
        let workspace_id = selected.record().workspace_id;
        if fields.contains(&ReadinessField::Manifest) {
            WorkspaceManifest::new(workspace_id).write_new(&root)?;
        }
        if let Some(local_user_id) = local_user_id.as_deref() {
            transaction.update(&mut registry, |candidate| {
                let target = &mut candidate
                    .workspaces
                    .get_mut(&canonical_name)
                    .expect("selected workspace remains present")
                    .local_user_id;
                local_user_id.clone_into(target);
                Ok(())
            })?;
        }
        Ok(())
    })
}

fn repaired_context(
    store: &RegistryStore,
    canonical_name: &super::WorkspaceName,
    expected_workspace_id: super::WorkspaceId,
    home: &Path,
    current_dir: &Path,
) -> Result<BootstrapContext> {
    let registry = RegistryStore::load_from(store.path())?;
    let selected = registry.select(Some(canonical_name.as_str()))?;
    if selected.record().workspace_id != expected_workspace_id {
        anyhow::bail!("selected workspace identity changed during command bootstrap");
    }
    let manifest = WorkspaceManifest::load(&selected.record().root, env!("CARGO_PKG_VERSION"))?;
    readiness_action(
        selected.canonical_name(),
        selected.record(),
        Ok(manifest),
        InteractionMode::NonInteractive,
    )?;
    context_from_record(
        store,
        selected.canonical_name().clone(),
        selected.record(),
        home,
        current_dir,
    )
}

fn context_from_record(
    store: &RegistryStore,
    canonical_name: super::WorkspaceName,
    record: &super::WorkspaceRecord,
    home: &Path,
    current_dir: &Path,
) -> Result<BootstrapContext> {
    let workspace = WorkspaceContext::new(
        home,
        record.workspace_id,
        canonical_name,
        &record.root,
        record.local_user_id.clone(),
        current_dir,
    )?;
    Ok(BootstrapContext::Ready(CommandContext {
        workspace: Arc::new(workspace),
        registry_store: store.clone(),
    }))
}

#[cfg(test)]
#[path = "bootstrap/tests.rs"]
mod tests;

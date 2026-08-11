//! Command bootstrap policy and selected workspace construction.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use super::bootstrap_policy::{RegistryOnlyPromptOrder, registry_only_prompt_order};
use super::{
    BootstrapPolicy, InteractionMode, Invocation, ReadinessAction, ReadinessField, RegistryStore,
    WorkspaceContext, WorkspaceManifest, bootstrap_policy, invocation_for,
    readiness_action_with_users,
};

/// One ready selected workspace plus the machine registry capability.
#[derive(Debug, Clone)]
pub struct CommandContext {
    pub workspace: Arc<WorkspaceContext>,
    pub actor: crate::actor::ActorContext,
    pub registry_store: RegistryStore,
}

impl CommandContext {
    /// Bind one immutable local actor to an ordinary command request.
    pub fn new(workspace: Arc<WorkspaceContext>, registry_store: RegistryStore) -> Result<Self> {
        let actor = crate::actor::local_actor(&workspace)?;
        Ok(Self {
            workspace,
            actor,
            registry_store,
        })
    }

    pub(super) fn new_read_only(
        workspace: Arc<WorkspaceContext>,
        registry_store: RegistryStore,
    ) -> Result<Self> {
        let actor = crate::actor::local_actor_read_only(&workspace)?;
        Ok(Self {
            workspace,
            actor,
            registry_store,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        workspace: Arc<WorkspaceContext>,
        registry_store: RegistryStore,
        actor_id: &str,
    ) -> Self {
        Self {
            workspace,
            actor: crate::actor::test_actor(actor_id),
            registry_store,
        }
    }
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
    // Strict mode is checked against what the *command line* said, before any
    // inheritance or discovery fills the selector in — its whole job is to catch
    // a Brain-built command that forgot `-w`.
    enforce_strict_selector(cli)?;
    let store = RegistryStore::real();
    // Resolved once, before anything reads the selector, so selection,
    // readiness, `is_some()` scope checks, and suggested commands all see one
    // answer.
    let current_dir = std::env::current_dir().context("read current directory")?;
    resolve_workspace_selector(cli, &store, &current_dir);
    if policy == BootstrapPolicy::ReadOnlyWorkspace {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| anyhow!("HOME is not set"))?;
        return super::read_only::bootstrap(cli, &store, &home, &current_dir);
    }
    if policy == BootstrapPolicy::RegistryOnly {
        let access_store = store.clone();
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
                let should_migrate = match invocation {
                    Invocation::WorkspaceCreate | Invocation::WorkspaceAttach => {
                        crate::env::registry_setup_needs_migration()?
                    }
                    Invocation::WorkspaceRemove | Invocation::WorkspaceRepair => {
                        !crate::env::registry_is_current()?
                    }
                    Invocation::User => !crate::env::registry_is_current()?,
                    _ => false,
                };
                if should_migrate {
                    crate::env::migrate_checked()?;
                }
                if !matches!(
                    invocation,
                    Invocation::WorkspaceCreate | Invocation::WorkspaceAttach
                ) {
                    ensure_selected_registry_access_mode(
                        &access_store,
                        prepared.workspace_selector.as_deref(),
                    )?;
                }
                Ok(())
            },
        );
    }

    if !crate::env::registry_is_current()? {
        crate::env::migrate_checked()?;
    }
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))?;
    let interaction = if std::io::stdin().is_terminal() {
        InteractionMode::Interactive
    } else {
        InteractionMode::NonInteractive
    };
    let context = if interaction == InteractionMode::Interactive {
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
        )?
    } else {
        bootstrap_with_io(
            cli,
            store,
            &home,
            &current_dir,
            interaction,
            &mut std::io::empty(),
            &mut std::io::sink(),
        )?
    };
    // First run of a new brain binary against a ready workspace re-renders the
    // bundled skills so a version bump ships its skill changes the way it ships
    // code changes. Deterministic, LLM-free, and a no-op once stamped. Only the
    // ready path reaches here, so `--help`/`--version`, the internal
    // hook/server, and registry-only maintenance never trigger it.
    if let BootstrapContext::Ready(command_context) = &context {
        validate_expected_workspace_id(
            std::env::var_os("BRAIN_WORKSPACE_ID").as_deref(),
            command_context.workspace.id(),
        )?;
        if should_migrate_global_skills(invocation_for(cli)) {
            crate::skills::migrate_global_skills_for_all_workspaces(None);
        }
        if should_resync_skills(invocation_for(cli)) {
            crate::skills::resync_on_version_change(&command_context.workspace);
        }
        // A member brain knows nothing about is a gap it should close the next
        // time it sees them, on whatever command they happened to run. Only the
        // person at *this* machine is asked; other members' gaps are reported by
        // `brain workspace status`, never prompted for here.
        if crate::personalization::onboarding::prompts_for_missing_persona(invocation_for(cli)) {
            crate::personalization::onboarding::prompt_for_missing_local_persona(
                &command_context.workspace,
            );
        }
    }
    Ok(context)
}

/// Fill in the workspace an invocation acts on when it named none.
///
/// `-w` wins; then the workspace a Brain-launched process was launched for
/// (`BRAIN_WORKSPACE`); then the workspace whose root contains the current
/// directory, discovered by walking upward the way git finds its repository;
/// then the machine default.
///
/// The launching workspace deliberately outranks the current directory: an agent
/// panel opened for `family` stays on `family` even while it reads files under
/// another root, and `BRAIN_WORKSPACE_ID` validation stays consistent with it.
fn resolve_workspace_selector(
    cli: &mut crate::cli::Cli,
    store: &RegistryStore,
    current_dir: &Path,
) {
    let inherited = std::env::var(super::selector::WORKSPACE_ENV).ok();
    if let Some(selector) =
        super::selector::effective_selector(cli.workspace_selector.as_deref(), inherited.as_deref())
    {
        cli.workspace_selector = Some(selector);
        return;
    }
    cli.workspace_selector = discover_workspace_from(store, current_dir);
}

/// The registered workspace containing `current_dir`, if any.
///
/// Roots and the current directory are canonicalized before comparison: on macOS
/// a `/tmp` root and a `/private/tmp` current directory are the same place, and a
/// lexical comparison would miss it. An unreadable registry simply discovers
/// nothing, leaving the machine default.
fn discover_workspace_from(store: &RegistryStore, current_dir: &Path) -> Option<String> {
    let registry = RegistryStore::load_readable(store.path()).ok()?;
    let roots = registry
        .workspaces
        .iter()
        .map(|(name, record)| {
            (
                name.as_str().to_owned(),
                record
                    .root
                    .canonicalize()
                    .unwrap_or_else(|_| record.root.clone()),
            )
        })
        .collect::<Vec<_>>();
    let here = current_dir
        .canonicalize()
        .unwrap_or_else(|_| current_dir.to_path_buf());
    super::selector::discover_from_ancestors(&roots, &here)
}

/// Refuse a strict child that names no workspace.
///
/// Brain sets `BRAIN_REQUIRE_WORKSPACE` on the children it spawns for its own
/// work, so any code path that builds a `brain …` command without `-w` fails
/// here instead of quietly operating on whichever workspace is default. An
/// ordinary interactive invocation is unaffected.
fn enforce_strict_selector(cli: &crate::cli::Cli) -> Result<()> {
    if super::selector::violates_strict_selector(
        super::selector::strict_selector_required(),
        cli.workspace_selector.is_some(),
    ) {
        anyhow::bail!(
            "{} is set, so this command must name its workspace explicitly with -w/--workspace; \
             Brain spawned it and a missing selector would silently target the default workspace",
            super::selector::STRICT_ENV
        );
    }
    Ok(())
}

const fn should_resync_skills(invocation: Invocation) -> bool {
    !matches!(invocation, Invocation::WorkspaceMigrate | Invocation::Tui)
}

pub(crate) const fn should_migrate_global_skills(invocation: Invocation) -> bool {
    !matches!(invocation, Invocation::WorkspaceMigrate | Invocation::Tui)
}

pub(super) fn validate_expected_workspace_id(
    raw_expected: Option<&std::ffi::OsStr>,
    selected: super::WorkspaceId,
) -> Result<()> {
    let Some(raw_expected) = raw_expected else {
        return Ok(());
    };
    let expected = raw_expected
        .to_str()
        .ok_or_else(|| anyhow!("BRAIN_WORKSPACE_ID is not valid UTF-8"))?;
    let expected = super::WorkspaceId::parse(expected)
        .map_err(|error| anyhow!("BRAIN_WORKSPACE_ID is invalid: {error}"))?;
    if expected != selected {
        anyhow::bail!(
            "BRAIN_WORKSPACE_ID {expected} does not match selected workspace UUID {selected}"
        );
    }
    Ok(())
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
    if bootstrap_policy(invocation_for(cli)) == BootstrapPolicy::ReadOnlyWorkspace {
        return super::read_only::bootstrap(cli, &store, home, current_dir);
    }
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
        BootstrapPolicy::ReadOnlyWorkspace => {
            return super::read_only::bootstrap(cli, &store, home, current_dir);
        }
        BootstrapPolicy::ReadyWorkspace => {}
    }

    let registry = RegistryStore::load_from(store.path())?;
    let selected = registry.select(cli.workspace_selector.as_deref())?;
    let canonical_name = selected.canonical_name().clone();
    let workspace_id = selected.record().workspace_id;
    let record = selected.record().clone();
    // Every command Brain suggests from here on names this workspace.
    super::selector::remember_selected(&canonical_name);
    let provisional = WorkspaceContext::new(
        home,
        record.workspace_id,
        canonical_name.clone(),
        &record.root,
        record.local_user_id.clone(),
        current_dir,
    )?;
    if invocation_for(cli) == Invocation::WorkspaceMigrate {
        anyhow::ensure!(
            selected.record().root.is_dir(),
            "workspace root {} is unavailable; restore it or detach the workspace",
            selected.record().root.display()
        );
        after_readiness()?;
        return context_from_record(&store, canonical_name, &record, home, current_dir);
    }
    // Set the machine up rather than reporting what it lacks: create the root
    // when this machine has never had it, fill it from the configured sync, and
    // seed PARA when there is nothing to pull. Migration is excluded above —
    // it is a transactional rewrite of an existing workspace, not a setup path.
    super::initialize::initialize_workspace_directory(
        &provisional,
        &store,
        super::initialize::performs_setup_sync(invocation_for(cli)),
    )?;
    let access_mode = if selected.canonical_name() == &registry.default_workspace {
        crate::access::AccessMode::Unrestricted
    } else {
        crate::access::AccessMode::WorkspaceOnly
    };
    crate::access::ensure_portable_access_mode(&selected.record().root, access_mode)
        .map_err(|error| anyhow!("validate portable workspace access mode: {error:#}"))?;
    let manifest = WorkspaceManifest::load(&selected.record().root, env!("CARGO_PKG_VERSION"));
    let users = crate::users::UsersStore::load(&provisional);
    let action = readiness_action_with_users(
        selected.canonical_name(),
        selected.record(),
        manifest,
        users,
        interaction,
    )?;
    match action {
        ReadinessAction::Ready(_) => {
            after_readiness()?;
            context_from_record(&store, canonical_name, &record, home, current_dir)
        }
        ReadinessAction::AdoptLocalUser(user_id) => {
            adopt_local_user(&store, canonical_name.as_str(), workspace_id, &user_id)?;
            after_readiness()?;
            repaired_context(&store, &canonical_name, workspace_id, home, current_dir)
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

/// Adopt the sole portable user as this machine's local actor. Runs with no
/// prompt for both interactive and headless commands, so any command against a
/// single-user workspace self-heals instead of failing with a follow-up command
/// to run. A themed note (stderr) reports the one-time link.
mod bootstrap_helpers;
use bootstrap_helpers::*;
#[cfg(test)]
#[path = "bootstrap/tests.rs"]
mod tests;

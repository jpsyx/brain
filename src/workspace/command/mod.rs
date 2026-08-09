//! Workspace CLI decisions and thin command shells.

mod list;
mod mutate;
mod preflight;
pub(super) mod prompt;
mod provision;

pub(crate) use preflight::registry_only as preflight_registry_only;
pub(crate) use preflight::registry_only_with_io as preflight_registry_only_with_io;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use mutate::{MutationInput, decide_mutation};

use crate::cli::{WorkspaceAction, WorkspaceAliasAction, WorkspaceArgs};
use crate::theme::Theme;
use crate::workspace::{
    CommandContext, MachineRegistry, RegistryStore, WorkspaceId, WorkspaceName,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum CommandSelection<'a> {
    Raw(Option<&'a str>),
    Pinned {
        canonical_name: &'a WorkspaceName,
        workspace_id: WorkspaceId,
    },
}

impl<'a> CommandSelection<'a> {
    fn validate(self, registry: &MachineRegistry) -> Result<()> {
        match self {
            Self::Raw(Some(selector)) => {
                registry.select(Some(selector))?;
            }
            Self::Raw(None) => {}
            Self::Pinned {
                canonical_name,
                workspace_id,
            } => {
                let selected = registry.select(Some(canonical_name.as_str()))?;
                if selected.record().workspace_id != workspace_id {
                    anyhow::bail!("selected workspace identity changed after command bootstrap");
                }
            }
        }
        Ok(())
    }

    fn raw_selector(self) -> Result<Option<&'a str>> {
        match self {
            Self::Raw(selector) => Ok(selector),
            Self::Pinned { .. } => {
                anyhow::bail!("internal workspace repair expected a registry-only selection")
            }
        }
    }
}

/// Run an ordinary workspace command against bootstrap's pinned identity.
pub(crate) fn run_ready(
    args: &WorkspaceArgs,
    context: &CommandContext,
    explicit_workspace: bool,
) -> Result<()> {
    if let WorkspaceAction::Migrate {
        acknowledge_all_machines_updated,
    } = &args.action
    {
        return crate::migration::run(
            context,
            explicit_workspace,
            *acknowledge_all_machines_updated,
        )
        .map_err(mutate::render_command_error);
    }
    run_inner(
        args,
        CommandSelection::Pinned {
            canonical_name: context.workspace.name(),
            workspace_id: context.workspace.id(),
        },
        &context.registry_store,
        Some(context),
        explicit_workspace,
    )
    .map_err(mutate::render_command_error)
}

/// Run a registry-only workspace command whose global selector has not yet
/// been resolved by bootstrap.
pub(crate) fn run_registry_only(
    args: &WorkspaceArgs,
    selector: Option<&str>,
    store: &RegistryStore,
) -> Result<()> {
    run_inner(
        args,
        CommandSelection::Raw(selector),
        store,
        None,
        selector.is_some(),
    )
    .map_err(mutate::render_command_error)
}

#[must_use]
pub fn render_error(error: anyhow::Error) -> anyhow::Error {
    mutate::render_command_error(error)
}

fn run_inner(
    args: &WorkspaceArgs,
    selection: CommandSelection<'_>,
    store: &RegistryStore,
    context: Option<&CommandContext>,
    explicit_workspace: bool,
) -> Result<()> {
    let answers = prompt::collect(&args.action)?;
    match &args.action {
        WorkspaceAction::List => {
            let registry = mutate::load_registry(store)?;
            selection.validate(&registry)?;
            list::print(&registry, context, explicit_workspace, Theme::active());
            Ok(())
        }
        WorkspaceAction::Create { name, root } => {
            let prompted_root = answers.value(prompt::PromptField::Root).map(PathBuf::from);
            let root = root
                .as_deref()
                .or(prompted_root.as_deref())
                .ok_or_else(|| anyhow!("workspace root was not provided"))?;
            let home = mutate::home_dir()?;
            let current_dir = std::env::current_dir().context("read the current directory")?;
            let decision = decide_mutation(
                MutationInput::Create {
                    name: name
                        .as_deref()
                        .or_else(|| answers.value(prompt::PromptField::Name)),
                    root,
                },
                &home,
                &current_dir,
            )?;
            provision::create(store, decision)?;
            Ok(())
        }
        WorkspaceAction::Attach { root } => {
            let prompted_root = answers.value(prompt::PromptField::Root).map(PathBuf::from);
            let root = root
                .as_deref()
                .or(prompted_root.as_deref())
                .ok_or_else(|| anyhow!("workspace root was not provided"))?;
            let home = mutate::home_dir()?;
            let current_dir = std::env::current_dir().context("read the current directory")?;
            let decision = decide_mutation(MutationInput::Attach { root }, &home, &current_dir)?;
            provision::attach(store, decision)?;
            Ok(())
        }
        WorkspaceAction::Rename { workspace, name } => {
            let workspace = required(
                workspace
                    .as_deref()
                    .or_else(|| answers.value(prompt::PromptField::Workspace)),
                "workspace to rename",
            )?;
            let name = required(
                name.as_deref()
                    .or_else(|| answers.value(prompt::PromptField::Name)),
                "new workspace name",
            )?;
            mutate::execute(
                store,
                selection,
                decide_mutation(
                    MutationInput::Rename {
                        selector: workspace,
                        new_name: name,
                    },
                    Path::new("/"),
                    Path::new("/"),
                )?,
            )
        }
        WorkspaceAction::Alias(args) => match &args.action {
            WorkspaceAliasAction::Add { workspace, alias } => mutate::execute(
                store,
                selection,
                decide_mutation(
                    MutationInput::AddAlias {
                        selector: required(
                            workspace
                                .as_deref()
                                .or_else(|| answers.value(prompt::PromptField::Workspace)),
                            "workspace",
                        )?,
                        alias: required(
                            alias
                                .as_deref()
                                .or_else(|| answers.value(prompt::PromptField::Alias)),
                            "alias to add",
                        )?,
                    },
                    Path::new("/"),
                    Path::new("/"),
                )?,
            ),
            WorkspaceAliasAction::Remove { workspace, alias } => mutate::execute(
                store,
                selection,
                decide_mutation(
                    MutationInput::RemoveAlias {
                        selector: required(
                            workspace
                                .as_deref()
                                .or_else(|| answers.value(prompt::PromptField::Workspace)),
                            "workspace",
                        )?,
                        alias: required(
                            alias
                                .as_deref()
                                .or_else(|| answers.value(prompt::PromptField::Alias)),
                            "alias to remove",
                        )?,
                    },
                    Path::new("/"),
                    Path::new("/"),
                )?,
            ),
        },
        WorkspaceAction::Default { workspace } => mutate::execute(
            store,
            selection,
            decide_mutation(
                MutationInput::SetDefault {
                    selector: required(
                        workspace
                            .as_deref()
                            .or_else(|| answers.value(prompt::PromptField::Workspace)),
                        "workspace to make default",
                    )?,
                },
                Path::new("/"),
                Path::new("/"),
            )?,
        ),
        WorkspaceAction::Remove { workspace } => mutate::execute(
            store,
            selection,
            decide_mutation(
                MutationInput::Remove {
                    selector: required(
                        workspace
                            .as_deref()
                            .or_else(|| answers.value(prompt::PromptField::Workspace)),
                        "workspace to remove",
                    )?,
                },
                Path::new("/"),
                Path::new("/"),
            )?,
        ),
        WorkspaceAction::Repair {
            manifest,
            local_user_id,
        } => {
            let interactive_repair = !manifest && local_user_id.is_none();
            repair(
                store,
                selection.raw_selector()?,
                *manifest || interactive_repair,
                local_user_id
                    .as_deref()
                    .or_else(|| answers.value(prompt::PromptField::LocalUserId)),
            )
        }
        WorkspaceAction::Migrate { .. } => {
            anyhow::bail!("internal workspace migration expected a ready command context")
        }
    }
}

fn repair(
    store: &RegistryStore,
    selector: Option<&str>,
    repair_manifest: bool,
    local_user_id: Option<&str>,
) -> Result<()> {
    if local_user_id.is_some_and(|value| value.trim().is_empty()) {
        anyhow::bail!("local user ID cannot be empty");
    }
    let local_user_id = local_user_id.map(str::trim).map(str::to_owned);
    store.transaction(|transaction| -> Result<()> {
        let mut registry = transaction.load()?;
        let selected = registry.select(selector)?;
        let canonical_name = selected.canonical_name().clone();
        let root = selected.record().root.clone();
        let workspace_id = selected.record().workspace_id;
        if repair_manifest {
            match crate::workspace::WorkspaceManifest::load(&root, env!("CARGO_PKG_VERSION")) {
                Ok(manifest) if manifest.workspace_id() == workspace_id => {}
                Ok(manifest) => anyhow::bail!(
                    "workspace manifest UUID {} does not match registry UUID {}",
                    manifest.workspace_id(),
                    workspace_id
                ),
                Err(crate::workspace::ManifestError::Io {
                    kind: std::io::ErrorKind::NotFound,
                    ..
                }) => {
                    crate::workspace::WorkspaceManifest::new(workspace_id).write_new(&root)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        if let Some(local_user_id) = local_user_id.as_deref() {
            transaction.update(&mut registry, |candidate| {
                let target = &mut candidate
                    .workspaces
                    .get_mut(&canonical_name)
                    .expect("selected canonical workspace remains present")
                    .local_user_id;
                local_user_id.clone_into(target);
                Ok(())
            })?;
        }
        Ok(())
    })?;
    println!("{}", Theme::active().success("Workspace setup repaired"));
    Ok(())
}

fn required<'a>(value: Option<&'a str>, label: &str) -> Result<&'a str> {
    value.ok_or_else(|| anyhow!("missing {label}; provide it as an argument"))
}

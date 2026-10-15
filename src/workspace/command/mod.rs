//! Workspace CLI decisions and thin command shells.

mod list;
mod mutate;
mod preflight;
pub(super) mod prompt;
mod provision;
mod setup;
pub(crate) use setup::prepared as apply_prepared_setup;

pub(crate) use preflight::registry_only as preflight_registry_only;
pub(crate) use preflight::registry_only_with_io as preflight_registry_only_with_io;

use std::path::Path;

use anyhow::{Result, anyhow};

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
    if setup::is_action(&args.action) {
        println!(
            "{}",
            setup::apply(&args.action, selection.raw_selector()?, store, &answers)?
        );
        return Ok(());
    }
    match &args.action {
        WorkspaceAction::List => {
            let registry = mutate::load_registry(store)?;
            selection.validate(&registry)?;
            list::print(&registry, context, explicit_workspace, Theme::active());
            Ok(())
        }
        WorkspaceAction::Create { .. } | WorkspaceAction::Attach { .. } => {
            unreachable!("setup handled before ordinary commands")
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
        WorkspaceAction::Repair { .. } => unreachable!("setup handled before ordinary commands"),
        WorkspaceAction::Migrate { .. } => {
            anyhow::bail!("internal workspace migration expected a ready command context")
        }
    }
}

fn required<'a>(value: Option<&'a str>, label: &str) -> Result<&'a str> {
    value.ok_or_else(|| anyhow!("missing {label}; provide it as an argument"))
}

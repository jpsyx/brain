//! Workspace CLI decisions and thin command shells.

mod list;
mod mutate;
mod prompt;
mod provision;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use mutate::{MutationInput, decide_mutation};

use crate::cli::{WorkspaceAction, WorkspaceAliasAction, WorkspaceArgs};
use crate::theme::Theme;
use crate::workspace::RegistryStore;

/// Run one workspace registry management command.
pub fn run(args: &WorkspaceArgs, selector: Option<&str>) -> Result<()> {
    run_inner(args, selector).map_err(mutate::render_command_error)
}

fn run_inner(args: &WorkspaceArgs, selector: Option<&str>) -> Result<()> {
    let store = RegistryStore::real();
    let answers = prompt::collect(&args.action)?;
    match &args.action {
        WorkspaceAction::List => {
            let registry = mutate::load_registry(&store)?;
            if let Some(selector) = selector {
                registry.select(Some(selector))?;
            }
            list::print(&registry, Theme::active());
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
            provision::create(&store, decision)?;
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
            provision::attach(&store, decision)?;
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
                &store,
                selector,
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
                &store,
                selector,
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
                &store,
                selector,
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
            &store,
            selector,
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
            &store,
            selector,
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
    }
}

fn required<'a>(value: Option<&'a str>, label: &str) -> Result<&'a str> {
    value.ok_or_else(|| anyhow!("missing {label}; provide it as an argument"))
}

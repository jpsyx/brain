//! Pure registry-mutation decisions.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};

use crate::theme::Theme;
use crate::workspace::{
    MachineRegistry, RegistryError, RegistryStore, WorkspaceContextError, WorkspaceName,
    WorkspaceNameError, normalize_root,
};

/// Raw, fully collected values for one non-interactive registry mutation.
#[derive(Debug, Clone, Copy)]
pub(super) enum MutationInput<'a> {
    Create {
        name: Option<&'a str>,
        root: &'a Path,
    },
    Attach {
        root: &'a Path,
    },
    Rename {
        selector: &'a str,
        new_name: &'a str,
    },
    AddAlias {
        selector: &'a str,
        alias: &'a str,
    },
    RemoveAlias {
        selector: &'a str,
        alias: &'a str,
    },
    SetDefault {
        selector: &'a str,
    },
    Remove {
        selector: &'a str,
    },
}

/// A validated registry mutation. Filesystem actions are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Mutation {
    Create {
        canonical_name: WorkspaceName,
        root: PathBuf,
    },
    Attach {
        canonical_name: WorkspaceName,
        root: PathBuf,
    },
    Rename {
        selector: String,
        new_name: WorkspaceName,
    },
    AddAlias {
        selector: String,
        alias: WorkspaceName,
    },
    RemoveAlias {
        selector: String,
        alias: WorkspaceName,
    },
    SetDefault {
        selector: String,
    },
    Remove {
        selector: String,
    },
}

/// A command value could not be converted into a validated mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MutationDecisionError {
    Name(WorkspaceNameError),
    Root(WorkspaceContextError),
}

impl Display for MutationDecisionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(error) => Display::fmt(error, formatter),
            Self::Root(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for MutationDecisionError {}

/// Validate and normalize collected CLI values without touching the filesystem.
pub(super) fn decide_mutation(
    input: MutationInput<'_>,
    home: &Path,
    current_dir: &Path,
) -> Result<Mutation, MutationDecisionError> {
    match input {
        MutationInput::Create { name, root } => {
            let root = normalize_cli_root(root, home, current_dir)?;
            let canonical_name =
                name.map_or_else(|| WorkspaceName::from_root(&root), WorkspaceName::parse)?;
            Ok(Mutation::Create {
                canonical_name,
                root,
            })
        }
        MutationInput::Attach { root } => {
            let root = normalize_cli_root(root, home, current_dir)?;
            Ok(Mutation::Attach {
                canonical_name: WorkspaceName::from_root(&root)?,
                root,
            })
        }
        MutationInput::Rename { selector, new_name } => Ok(Mutation::Rename {
            selector: selector.to_owned(),
            new_name: WorkspaceName::parse(new_name)?,
        }),
        MutationInput::AddAlias { selector, alias } => Ok(Mutation::AddAlias {
            selector: selector.to_owned(),
            alias: WorkspaceName::parse(alias)?,
        }),
        MutationInput::RemoveAlias { selector, alias } => Ok(Mutation::RemoveAlias {
            selector: selector.to_owned(),
            alias: WorkspaceName::parse(alias)?,
        }),
        MutationInput::SetDefault { selector } => Ok(Mutation::SetDefault {
            selector: selector.to_owned(),
        }),
        MutationInput::Remove { selector } => Ok(Mutation::Remove {
            selector: selector.to_owned(),
        }),
    }
}

fn normalize_cli_root(
    root: &Path,
    home: &Path,
    current_dir: &Path,
) -> Result<PathBuf, WorkspaceContextError> {
    let expanded = if root == Path::new("~") {
        home.to_path_buf()
    } else if let Ok(rest) = root.strip_prefix("~") {
        home.join(rest)
    } else {
        root.to_path_buf()
    };
    normalize_root(&expanded, current_dir)
}

impl From<WorkspaceNameError> for MutationDecisionError {
    fn from(error: WorkspaceNameError) -> Self {
        Self::Name(error)
    }
}

impl From<WorkspaceContextError> for MutationDecisionError {
    fn from(error: WorkspaceContextError) -> Self {
        Self::Root(error)
    }
}

pub(super) fn execute(
    store: &RegistryStore,
    selection: super::CommandSelection<'_>,
    mutation: Mutation,
) -> anyhow::Result<()> {
    let message = store.transaction(|transaction| -> anyhow::Result<String> {
        let mut registry = transaction.load()?;
        selection.validate(&registry)?;
        let message = match mutation {
            Mutation::Rename { selector, new_name } => {
                let display = format!("Renamed workspace to {new_name}");
                transaction.update(&mut registry, |candidate| {
                    let canonical = candidate.select(Some(&selector))?.canonical_name().clone();
                    candidate.rename(canonical.as_str(), new_name)
                })?;
                display
            }
            Mutation::AddAlias { selector, alias } => {
                let display = format!("Added workspace alias {alias}");
                transaction.update(&mut registry, |candidate| {
                    let canonical = candidate.select(Some(&selector))?.canonical_name().clone();
                    candidate.add_alias(canonical.as_str(), alias)
                })?;
                display
            }
            Mutation::RemoveAlias { selector, alias } => {
                let display = format!("Removed workspace alias {alias}");
                transaction.update(&mut registry, |candidate| {
                    let canonical = candidate.select(Some(&selector))?.canonical_name().clone();
                    candidate.remove_alias(canonical.as_str(), alias.as_str())
                })?;
                display
            }
            Mutation::SetDefault { selector } => {
                let display = format!("Set default workspace to {selector}");
                transaction.update(&mut registry, |candidate| candidate.set_default(&selector))?;
                display
            }
            Mutation::Remove { selector } => {
                let display =
                    format!("Detached workspace {selector}; root contents were preserved");
                transaction.update(&mut registry, |candidate| {
                    candidate.remove(&selector).map(|_| ())
                })?;
                display
            }
            Mutation::Create { .. } | Mutation::Attach { .. } => {
                bail!("internal workspace mutation decision mismatch")
            }
        };
        Ok(message)
    })?;
    println!("{}", Theme::active().success(&message));
    Ok(())
}

pub(super) fn load_registry(store: &RegistryStore) -> anyhow::Result<MachineRegistry> {
    RegistryStore::load_from(store.path()).map_err(Into::into)
}

pub(super) fn home_dir() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| anyhow!("$HOME must be set to an absolute path"))
}

pub(super) fn render_command_error(error: anyhow::Error) -> anyhow::Error {
    render_command_error_with(error, Theme::active())
}

pub(super) fn render_command_error_with(error: anyhow::Error, theme: Theme) -> anyhow::Error {
    let hint = error
        .downcast_ref::<RegistryError>()
        .and_then(registry_error_hint)
        .or_else(|| {
            error
                .downcast_ref::<MutationDecisionError>()
                .map(|error| decision_error_hint(*error))
        });
    let mut message = theme.error(&format!("Workspace error: {error}"));
    if let Some(hint) = hint {
        message.push_str(&theme.muted(&format!("; {hint}")));
    }
    error.context(message)
}

fn decision_error_hint(error: MutationDecisionError) -> &'static str {
    match error {
        MutationDecisionError::Name(_) => "use a name matching [a-z0-9][a-z0-9_-]*",
        MutationDecisionError::Root(_) => "use a root that resolves to an absolute path",
    }
}

fn registry_error_hint(error: &RegistryError) -> Option<&'static str> {
    match error {
        RegistryError::UnknownSelector { .. }
        | RegistryError::UnknownWorkspace { .. }
        | RegistryError::UnknownAlias { .. } => {
            Some("run `brain workspace list` to see available names and aliases")
        }
        RegistryError::WorkspaceAlreadyExists { .. } => Some("choose a unique canonical name"),
        RegistryError::AliasAlreadyExists { .. } => {
            Some("remove the existing alias or choose a different one")
        }
        RegistryError::DuplicateSelector { .. } => Some("choose a unique canonical name or alias"),
        RegistryError::OverlappingRoots { .. } => {
            Some("choose a root outside every registered workspace")
        }
        RegistryError::EmptyRegistry | RegistryError::MissingDefault { .. } => {
            Some("set another default before removing this workspace")
        }
        RegistryError::UnsupportedSchemaVersion { .. }
        | RegistryError::LockTimeout { .. }
        | RegistryError::DuplicateWorkspaceId { .. }
        | RegistryError::WorkspaceIdentityChanged { .. }
        | RegistryError::RelativeRoot { .. }
        | RegistryError::Json { .. }
        | RegistryError::Io { .. }
        | RegistryError::Manifest(_) => None,
    }
}

#[cfg(test)]
mod tests;

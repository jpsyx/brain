//! Filesystem-aware create and attach transaction shells.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use serde_json::Map;

use super::mutate::Mutation;
use crate::theme::Theme;
use crate::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryError, RegistryOperation, RegistryStore,
    WorkspaceId, WorkspaceName, WorkspaceRecord,
};

pub(super) fn create(store: &RegistryStore, decision: Mutation) -> anyhow::Result<()> {
    let Mutation::Create {
        canonical_name,
        root,
    } = decision
    else {
        bail!("internal workspace create decision mismatch");
    };
    store.transaction(|transaction| -> anyhow::Result<()> {
        let record = fresh_record(root.clone());
        let candidate = match transaction.load() {
            Ok(mut registry) => {
                registry.attach_record(canonical_name.clone(), record)?;
                registry
            }
            Err(error) if is_missing_registry(&error) => {
                let registry = first_registry(canonical_name.clone(), record);
                crate::workspace::validate_registry(&registry)?;
                registry
            }
            Err(error) => return Err(error.into()),
        };
        let created_directories = create_missing_directory_chain(&root)?;
        if let Err(error) = transaction.save(&candidate) {
            return Err(manual_cleanup_required(error.into(), &created_directories));
        }
        Ok(())
    })?;

    let theme = Theme::active();
    println!(
        "{} {} {}",
        theme.success("Registered workspace"),
        theme.accent(canonical_name.as_str()),
        theme.muted(&format!("at {}", root.display()))
    );
    Ok(())
}

fn create_missing_directory_chain(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    create_missing_directory_chain_with(root, |_| {})
}

fn create_missing_directory_chain_with(
    root: &Path,
    mut before_create: impl FnMut(&Path),
) -> anyhow::Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    let mut cursor = root;
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(_) if cursor == root => bail!(
                "{} already exists; use `brain workspace attach {}` instead",
                root.display(),
                root.display()
            ),
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => bail!("{} exists and is not a directory", cursor.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.to_path_buf());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect workspace root {}", cursor.display()));
            }
        }
        cursor = cursor
            .parent()
            .ok_or_else(|| anyhow!("{} has no existing directory ancestor", root.display()))?;
    }
    missing.reverse();

    let mut created = Vec::with_capacity(missing.len());
    for directory in missing {
        before_create(&directory);
        if let Err(error) = std::fs::create_dir(&directory) {
            let failure = if error.kind() == std::io::ErrorKind::AlreadyExists {
                anyhow!(
                    "{} appeared while brain was creating it; use `brain workspace attach {}` if it is the intended root",
                    directory.display(),
                    root.display()
                )
            } else {
                anyhow!(error).context(format!(
                    "create workspace directory {}",
                    directory.display()
                ))
            };
            return Err(manual_cleanup_required(failure, &created));
        }
        created.push(directory);
    }
    Ok(created)
}

#[derive(Debug)]
struct ProvisionCleanupRequiredError {
    failure: anyhow::Error,
    created_directories: Box<[PathBuf]>,
}

impl Display for ProvisionCleanupRequiredError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}; automatic cleanup was skipped to avoid deleting a directory replaced by another process. Inspect these paths and remove only directories you confirm are safe. This command created directories at the following paths before the failure, deepest first:",
            self.failure
        )?;
        for directory in self.created_directories.iter().rev() {
            write!(formatter, "\n  - {}", directory.display())?;
        }
        Ok(())
    }
}

impl Error for ProvisionCleanupRequiredError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.failure.as_ref())
    }
}

fn manual_cleanup_required(
    failure: anyhow::Error,
    created_directories: &[PathBuf],
) -> anyhow::Error {
    if created_directories.is_empty() {
        return failure;
    }
    anyhow::Error::new(ProvisionCleanupRequiredError {
        failure,
        created_directories: created_directories.into(),
    })
}

pub(super) fn attach(store: &RegistryStore, decision: Mutation) -> anyhow::Result<()> {
    let Mutation::Attach {
        canonical_name,
        root,
    } = decision
    else {
        bail!("internal workspace attach decision mismatch");
    };
    store.transaction(|transaction| -> anyhow::Result<()> {
        if !root.is_dir() {
            bail!(
                "{} is not an existing directory; use `brain workspace create --root {}` instead",
                root.display(),
                root.display()
            );
        }
        let record = fresh_record(root.clone());
        match transaction.load() {
            Ok(mut registry) => transaction.update(&mut registry, |candidate| {
                candidate.attach_record(canonical_name.clone(), record)
            })?,
            Err(error) if is_missing_registry(&error) => {
                transaction.save(&first_registry(canonical_name.clone(), record))?;
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    })?;

    let theme = Theme::active();
    println!(
        "{} {} {}",
        theme.success("Attached workspace"),
        theme.accent(canonical_name.as_str()),
        theme.muted(&format!("at {}", root.display()))
    );
    Ok(())
}

fn fresh_record(root: PathBuf) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: WorkspaceId::new(),
        root,
        aliases: BTreeSet::new(),
        local_user_id: String::new(),
        receiver_enabled: false,
        env: Map::new(),
    }
}

fn first_registry(canonical_name: WorkspaceName, record: WorkspaceRecord) -> MachineRegistry {
    MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: BTreeMap::from([(canonical_name, record)]),
    }
}

fn is_missing_registry(error: &RegistryError) -> bool {
    matches!(
        error,
        RegistryError::Io {
            operation: RegistryOperation::ReadRegistry,
            kind: std::io::ErrorKind::NotFound,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use anyhow::Error;

    use super::super::mutate::render_command_error_with;
    use super::{create_missing_directory_chain_with, manual_cleanup_required};
    use crate::theme::Theme;

    #[derive(Debug)]
    struct PersistenceMarker;

    impl std::fmt::Display for PersistenceMarker {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("registry persistence failed")
        }
    }

    impl std::error::Error for PersistenceMarker {}

    #[test]
    fn already_exists_during_creation_is_a_race_and_preserves_the_other_directory() {
        let fixture = tempfile::tempdir().expect("isolated root");
        let root = fixture.path().join("raced-root");

        let error = create_missing_directory_chain_with(&root, |directory| {
            if directory == root {
                std::fs::create_dir(directory).expect("competing creator");
            }
        })
        .unwrap_err();

        assert!(root.is_dir());
        assert!(
            error
                .to_string()
                .contains("appeared while brain was creating it")
        );
        assert!(error.to_string().contains("workspace attach"));
    }

    #[test]
    fn persistence_failure_preserves_every_created_directory_for_manual_cleanup() {
        let fixture = tempfile::tempdir().expect("isolated root");
        let created_parent = fixture.path().join("created");
        let created_root = created_parent.join("family");
        std::fs::create_dir_all(&created_root).expect("created root chain");
        let created = [created_parent.clone(), created_root.clone()];

        let error = manual_cleanup_required(Error::new(PersistenceMarker), &created);

        assert!(created_parent.is_dir());
        assert!(created_root.is_dir());
        let message = error.to_string();
        assert!(message.contains("automatic cleanup was skipped"));
        assert!(message.contains("deepest first"));
        assert!(message.contains(&created_parent.display().to_string()));
        assert!(message.contains(&created_root.display().to_string()));
        assert!(
            error
                .chain()
                .any(<dyn std::error::Error + 'static>::is::<PersistenceMarker>)
        );
    }

    #[test]
    fn persistence_failure_preserves_an_injected_replacement_without_removing_it() {
        let fixture = tempfile::tempdir().expect("isolated root");
        let root = fixture.path().join("replaced-root");
        std::fs::create_dir(&root).expect("original root");
        let created = root.clone();
        std::fs::remove_dir(&root).expect("remove original root");
        std::fs::create_dir(&root).expect("replacement root");
        let sentinel = root.join("replacement.txt");
        std::fs::write(&sentinel, "preserve me").expect("replacement sentinel");

        let error = manual_cleanup_required(Error::new(PersistenceMarker), &[created]);

        assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "preserve me");
        assert!(error.to_string().contains("automatic cleanup was skipped"));
        assert!(
            error
                .chain()
                .any(<dyn std::error::Error + 'static>::is::<PersistenceMarker>)
        );
    }

    #[test]
    fn partial_create_failure_preserves_created_parent_and_lists_only_that_path() {
        let fixture = tempfile::tempdir().expect("isolated root");
        let created_parent = fixture.path().join("created");
        let created_root = created_parent.join("family");
        let error = create_missing_directory_chain_with(&created_root, |directory| {
            if directory == created_root {
                std::fs::create_dir(directory).expect("competing creator");
            }
        })
        .unwrap_err();

        assert!(created_parent.is_dir());
        assert!(created_root.is_dir());
        let message = error.to_string();
        assert!(message.contains("automatic cleanup was skipped"));
        let cleanup = message
            .split_once("deepest first:\n")
            .expect("manual cleanup list")
            .1;
        assert_eq!(cleanup, format!("  - {}", created_parent.display()));
        assert!(error.chain().any(|source| {
            source
                .to_string()
                .contains("appeared while brain was creating it")
        }));
    }

    #[test]
    fn themed_command_rendering_preserves_cleanup_display_and_original_source() {
        let created_parent = std::path::PathBuf::from("/workspaces/created");
        let created_root = created_parent.join("family");
        let error = manual_cleanup_required(
            Error::new(PersistenceMarker),
            &[created_parent.clone(), created_root.clone()],
        );

        let rendered = render_command_error_with(error, Theme::dark(false));

        assert_eq!(
            rendered.to_string(),
            format!(
                "Workspace error: registry persistence failed; automatic cleanup was skipped to avoid deleting a directory replaced by another process. Inspect these paths and remove only directories you confirm are safe. This command created directories at the following paths before the failure, deepest first:\n  - {}\n  - {}",
                created_root.display(),
                created_parent.display()
            )
        );
        assert!(
            rendered
                .chain()
                .any(<dyn std::error::Error + 'static>::is::<PersistenceMarker>)
        );
    }
}

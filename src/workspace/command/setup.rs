//! Prepared setup decisions and their existing registry transaction shells.
use super::{
    mutate::{self, MutationInput, decide_mutation},
    prompt, provision,
};
use crate::{
    cli::{WorkspaceAction, WorkspaceArgs},
    theme::Theme,
    workspace::RegistryStore,
};
use anyhow::{Context, Result, anyhow};
use std::path::PathBuf;

pub(super) const fn is_action(action: &WorkspaceAction) -> bool {
    matches!(
        action,
        WorkspaceAction::Create { .. }
            | WorkspaceAction::Attach { .. }
            | WorkspaceAction::Repair { .. }
    )
}

pub(crate) fn prepared(
    args: &WorkspaceArgs,
    selector: Option<&str>,
    store: &RegistryStore,
) -> Result<String> {
    let answers = prompt::collect(&args.action)?;
    apply(&args.action, selector, store, &answers).map_err(mutate::render_command_error)
}

pub(super) fn apply(
    action: &WorkspaceAction,
    selector: Option<&str>,
    store: &RegistryStore,
    answers: &prompt::Answers,
) -> Result<String> {
    match action {
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
            provision::create(store, decision)
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
            provision::attach(store, decision)
        }
        WorkspaceAction::Repair {
            manifest,
            local_user_id,
        } => {
            let interactive_repair = !manifest && local_user_id.is_none();
            repair(
                store,
                selector,
                *manifest || interactive_repair,
                local_user_id
                    .as_deref()
                    .or_else(|| answers.value(prompt::PromptField::LocalUserId)),
            )
        }
        _ => anyhow::bail!("expected a prepared workspace setup action"),
    }
}

fn repair(
    store: &RegistryStore,
    selector: Option<&str>,
    repair_manifest: bool,
    local_user_id: Option<&str>,
) -> Result<String> {
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
    Ok(Theme::active().success("Workspace setup repaired"))
}

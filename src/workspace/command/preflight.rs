//! Prompt and pure validation before registry-only bootstrap may mutate legacy state.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

use super::mutate::{MutationInput, decide_mutation};
use super::prompt;
use crate::cli::WorkspaceAction;
use crate::theme::Theme;

pub(crate) fn registry_only(cli: &mut crate::cli::Cli) -> Result<()> {
    let Some(crate::cli::Cmd::Workspace(args)) = cli.command.as_mut() else {
        return Ok(());
    };
    if !is_registry_only_action(&args.action) {
        return Ok(());
    }
    let answers = prompt::collect(&args.action)?;
    apply_answers(&mut args.action, &answers)?;
    validate_action(&args.action)
}

pub(crate) fn registry_only_with_io(
    cli: &mut crate::cli::Cli,
    reader: &mut impl std::io::BufRead,
    writer: &mut impl std::io::Write,
    theme: Theme,
) -> Result<()> {
    let Some(crate::cli::Cmd::Workspace(args)) = cli.command.as_mut() else {
        return Ok(());
    };
    if !is_registry_only_action(&args.action) {
        return Ok(());
    }
    let answers = prompt::collect_from(&args.action, reader, writer, theme)?;
    apply_answers(&mut args.action, &answers)?;
    validate_action(&args.action)
}

const fn is_registry_only_action(action: &WorkspaceAction) -> bool {
    matches!(
        action,
        WorkspaceAction::Create { .. }
            | WorkspaceAction::Attach { .. }
            | WorkspaceAction::Remove { .. }
            | WorkspaceAction::Repair { .. }
    )
}

fn apply_answers(action: &mut WorkspaceAction, answers: &prompt::Answers) -> Result<()> {
    match action {
        WorkspaceAction::Create { name, root } => {
            if root.is_none() {
                *root = answers.value(prompt::PromptField::Root).map(PathBuf::from);
            }
            if name.is_none() {
                *name = answers.value(prompt::PromptField::Name).map(str::to_owned);
            }
        }
        WorkspaceAction::Attach { root } => {
            if root.is_none() {
                *root = answers.value(prompt::PromptField::Root).map(PathBuf::from);
            }
        }
        WorkspaceAction::Remove { workspace } => {
            if workspace.is_none() {
                *workspace = answers
                    .value(prompt::PromptField::Workspace)
                    .map(str::to_owned);
            }
        }
        WorkspaceAction::Repair {
            manifest,
            local_user_id,
        } if !*manifest && local_user_id.is_none() => {
            *manifest = true;
            *local_user_id = Some(
                answers
                    .value(prompt::PromptField::LocalUserId)
                    .ok_or_else(|| anyhow!("local user ID was not provided"))?
                    .to_owned(),
            );
        }
        WorkspaceAction::Repair { .. } => {}
        WorkspaceAction::List
        | WorkspaceAction::Rename { .. }
        | WorkspaceAction::Alias(_)
        | WorkspaceAction::Default { .. } => {
            anyhow::bail!("internal workspace preflight received a ready-workspace action")
        }
    }
    Ok(())
}

fn validate_action(action: &WorkspaceAction) -> Result<()> {
    match action {
        WorkspaceAction::Create { name, root } => {
            let home = super::mutate::home_dir()?;
            let current_dir = std::env::current_dir().context("read the current directory")?;
            decide_mutation(
                MutationInput::Create {
                    name: name.as_deref(),
                    root: root
                        .as_deref()
                        .ok_or_else(|| anyhow!("workspace root was not provided"))?,
                },
                &home,
                &current_dir,
            )?;
        }
        WorkspaceAction::Attach { root } => {
            let home = super::mutate::home_dir()?;
            let current_dir = std::env::current_dir().context("read the current directory")?;
            decide_mutation(
                MutationInput::Attach {
                    root: root
                        .as_deref()
                        .ok_or_else(|| anyhow!("workspace root was not provided"))?,
                },
                &home,
                &current_dir,
            )?;
        }
        WorkspaceAction::Remove { workspace } => {
            decide_mutation(
                MutationInput::Remove {
                    selector: workspace
                        .as_deref()
                        .ok_or_else(|| anyhow!("workspace to remove was not provided"))?,
                },
                std::path::Path::new("/"),
                std::path::Path::new("/"),
            )?;
        }
        WorkspaceAction::Repair { local_user_id, .. } => {
            if local_user_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                anyhow::bail!("local user ID cannot be empty");
            }
        }
        WorkspaceAction::List
        | WorkspaceAction::Rename { .. }
        | WorkspaceAction::Alias(_)
        | WorkspaceAction::Default { .. } => {
            anyhow::bail!("internal workspace preflight received a ready-workspace action")
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::registry_only_with_io;
    use crate::cli::{Cmd, WorkspaceAction, try_parse_from};
    use crate::theme::Theme;
    use crate::workspace::WorkspaceManifest;

    fn tree_snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, Option<Vec<u8>>)> {
        let mut entries = walkdir::WalkDir::new(root)
            .into_iter()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.path() != root)
            .map(|entry| {
                let relative = entry.path().strip_prefix(root).unwrap().to_path_buf();
                let bytes = entry
                    .file_type()
                    .is_file()
                    .then(|| std::fs::read(entry.path()).unwrap());
                (relative, bytes)
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }

    #[test]
    fn create_and_attach_eof_preflight_leave_every_legacy_artifact_unchanged() {
        for argv in [
            ["brain", "workspace", "create"],
            ["brain", "workspace", "attach"],
        ] {
            let home = tempfile::tempdir().unwrap();
            let config_home = tempfile::tempdir().unwrap();
            let config_dir = config_home.path().join("brain");
            std::fs::create_dir_all(&config_dir).unwrap();
            let legacy_root = home.path().join("legacy");
            std::fs::create_dir_all(&legacy_root).unwrap();
            std::fs::write(legacy_root.join("keep.txt"), b"keep").unwrap();
            let env_path = config_dir.join("env.json");
            let legacy_env = br#"{"root":"~/legacy","custom":"keep"}"#;
            std::fs::write(&env_path, legacy_env).unwrap();
            let pointer_path = config_home.path().join("brain-root");
            let pointer = b"~/legacy\n";
            std::fs::write(&pointer_path, pointer).unwrap();
            let home_before = tree_snapshot(home.path());
            let config_before = tree_snapshot(config_home.path());
            let mut cli = try_parse_from(argv).unwrap();
            let mut reader = Cursor::new(Vec::<u8>::new());
            let mut writer = Vec::new();

            let error =
                registry_only_with_io(&mut cli, &mut reader, &mut writer, Theme::dark(false))
                    .unwrap_err();

            assert!(
                error
                    .to_string()
                    .contains("cancelled before the registry changed")
            );
            assert_eq!(std::fs::read(&env_path).unwrap(), legacy_env);
            assert_eq!(std::fs::read(&pointer_path).unwrap(), pointer);
            assert_eq!(tree_snapshot(home.path()), home_before);
            assert_eq!(tree_snapshot(config_home.path()), config_before);
            assert!(!WorkspaceManifest::path(&legacy_root).exists());
            assert!(std::fs::read_dir(&config_dir).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("backup")
            }));
        }
    }

    #[test]
    fn complete_registry_only_flags_preflight_without_terminal_io() {
        for argv in [
            vec!["brain", "workspace", "create", "--root", "/tmp/family"],
            vec!["brain", "workspace", "attach", "/tmp/family"],
            vec!["brain", "workspace", "remove", "family"],
            vec![
                "brain",
                "workspace",
                "repair",
                "--manifest",
                "--local-user-id",
                "pablo",
            ],
        ] {
            let mut cli = try_parse_from(argv).unwrap();
            let mut reader = Cursor::new(Vec::<u8>::new());
            let mut writer = Vec::new();

            registry_only_with_io(&mut cli, &mut reader, &mut writer, Theme::dark(false)).unwrap();

            assert!(writer.is_empty());
        }
    }

    #[test]
    fn bare_repair_preflight_prepares_both_required_repairs() {
        let mut cli = try_parse_from(["brain", "workspace", "repair"]).unwrap();
        let mut reader = Cursor::new(b"pablo\n".to_vec());
        let mut writer = Vec::new();

        registry_only_with_io(&mut cli, &mut reader, &mut writer, Theme::dark(false)).unwrap();

        let Some(Cmd::Workspace(args)) = cli.command else {
            panic!("workspace command expected");
        };
        let WorkspaceAction::Repair {
            manifest,
            local_user_id,
        } = args.action
        else {
            panic!("repair action expected");
        };
        assert!(manifest);
        assert_eq!(local_user_id.as_deref(), Some("pablo"));
    }

    #[test]
    fn invalid_complete_values_fail_during_preflight() {
        let mut cli = try_parse_from([
            "brain",
            "workspace",
            "create",
            "--root",
            "/tmp/family",
            "--name",
            "not valid",
        ])
        .unwrap();
        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut writer = Vec::new();

        let error = registry_only_with_io(&mut cli, &mut reader, &mut writer, Theme::dark(false))
            .unwrap_err();

        assert!(
            error.to_string().contains("workspace name must match"),
            "{error:#}"
        );
        assert!(writer.is_empty());
    }
}

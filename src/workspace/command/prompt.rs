//! `/dev/tty` prompts for omitted human-facing command values.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};

use anyhow::{Context, Result, anyhow};

use crate::cli::{WorkspaceAction, WorkspaceAliasAction};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::workspace) enum PromptField {
    Root,
    Name,
    Workspace,
    Alias,
    LocalUserId,
}

#[derive(Debug)]
pub(super) struct Answers(BTreeMap<PromptField, String>);

impl Answers {
    pub(super) fn value(&self, field: PromptField) -> Option<&str> {
        self.0.get(&field).map(String::as_str)
    }
}

pub(super) fn missing_fields(action: &WorkspaceAction) -> Vec<PromptField> {
    match action {
        WorkspaceAction::List => Vec::new(),
        WorkspaceAction::Repair {
            manifest,
            local_user_id,
        } => (!manifest && local_user_id.is_none())
            .then_some(PromptField::LocalUserId)
            .into_iter()
            .collect(),
        WorkspaceAction::Create { name, root } => {
            let mut fields = Vec::new();
            if root.is_none() {
                fields.push(PromptField::Root);
                if name.is_none() {
                    fields.push(PromptField::Name);
                }
            }
            fields
        }
        WorkspaceAction::Attach { root } => root
            .is_none()
            .then_some(PromptField::Root)
            .into_iter()
            .collect(),
        WorkspaceAction::Rename { workspace, name } => [
            workspace.is_none().then_some(PromptField::Workspace),
            name.is_none().then_some(PromptField::Name),
        ]
        .into_iter()
        .flatten()
        .collect(),
        WorkspaceAction::Alias(args) => match &args.action {
            WorkspaceAliasAction::Add { workspace, alias }
            | WorkspaceAliasAction::Remove { workspace, alias } => [
                workspace.is_none().then_some(PromptField::Workspace),
                alias.is_none().then_some(PromptField::Alias),
            ]
            .into_iter()
            .flatten()
            .collect(),
        },
        WorkspaceAction::Default { workspace } | WorkspaceAction::Remove { workspace } => workspace
            .is_none()
            .then_some(PromptField::Workspace)
            .into_iter()
            .collect(),
    }
}

pub(super) fn collect(action: &WorkspaceAction) -> Result<Answers> {
    let fields = missing_fields(action);
    if fields.is_empty() {
        return Ok(Answers(BTreeMap::new()));
    }

    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("open /dev/tty for workspace prompts")?;
    let mut reader = BufReader::new(tty.try_clone().context("clone /dev/tty")?);
    collect_from(action, &mut reader, &mut tty, Theme::active())
}

pub(super) fn collect_from(
    action: &WorkspaceAction,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    theme: Theme,
) -> Result<Answers> {
    let fields = missing_fields(action);
    let mut answers = BTreeMap::new();
    for field in fields {
        let optional =
            field == PromptField::Name && matches!(action, WorkspaceAction::Create { .. });
        if let Some(value) = read_answer(writer, reader, field, optional, theme)? {
            answers.insert(field, value);
        }
    }
    Ok(Answers(answers))
}

fn read_answer(
    writer: &mut impl Write,
    reader: &mut impl BufRead,
    field: PromptField,
    optional: bool,
    theme: Theme,
) -> Result<Option<String>> {
    let label = match field {
        PromptField::Root => "Workspace root:",
        PromptField::Name if optional => "Workspace name (blank uses root basename):",
        PromptField::Name => "New workspace name:",
        PromptField::Workspace => "Workspace name or alias:",
        PromptField::Alias => "Workspace alias:",
        PromptField::LocalUserId => "Local user ID (for example, pablo):",
    };
    loop {
        write!(writer, "{} ", theme.prompt(label)).context("write workspace prompt")?;
        writer.flush().context("flush workspace prompt")?;
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .context("read workspace prompt")?
            == 0
        {
            return Err(anyhow!(
                "workspace command cancelled before the registry changed"
            ));
        }
        let value = line.trim();
        if optional && value.is_empty() {
            return Ok(None);
        }
        if !value.is_empty() {
            return Ok(Some(value.to_owned()));
        }
        writeln!(writer, "{}", theme.warning("A value is required."))
            .context("write workspace prompt validation")?;
    }
}

pub(in crate::workspace) fn read_required(
    writer: &mut impl Write,
    reader: &mut impl BufRead,
    field: PromptField,
    theme: Theme,
) -> Result<String> {
    read_answer(writer, reader, field, false, theme)?.ok_or_else(|| {
        anyhow!("workspace setup was cancelled before required values were provided")
    })
}

#[cfg(test)]
mod tests {
    use std::io::{self, BufRead, Cursor, Read, Write};
    use std::path::PathBuf;

    use super::{PromptField, collect_from, missing_fields};
    use crate::cli::{WorkspaceAction, WorkspaceAliasAction, WorkspaceAliasArgs};
    use crate::theme::Theme;

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected prompt read failure"))
        }
    }

    impl BufRead for FailingReader {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::other("injected prompt read failure"))
        }

        fn consume(&mut self, _amount: usize) {}
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("injected prompt write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("injected prompt flush failure"))
        }
    }

    #[test]
    fn omitted_values_have_complete_prompt_plans_without_prompting_explicit_values() {
        let cases = [
            (
                WorkspaceAction::Create {
                    name: None,
                    root: None,
                },
                vec![PromptField::Root, PromptField::Name],
            ),
            (
                WorkspaceAction::Create {
                    name: None,
                    root: Some(PathBuf::from("/brains/family")),
                },
                vec![],
            ),
            (
                WorkspaceAction::Attach { root: None },
                vec![PromptField::Root],
            ),
            (
                WorkspaceAction::Rename {
                    workspace: None,
                    name: None,
                },
                vec![PromptField::Workspace, PromptField::Name],
            ),
            (
                WorkspaceAction::Alias(WorkspaceAliasArgs {
                    action: WorkspaceAliasAction::Add {
                        workspace: None,
                        alias: None,
                    },
                }),
                vec![PromptField::Workspace, PromptField::Alias],
            ),
            (
                WorkspaceAction::Default { workspace: None },
                vec![PromptField::Workspace],
            ),
            (
                WorkspaceAction::Remove { workspace: None },
                vec![PromptField::Workspace],
            ),
            (
                WorkspaceAction::Repair {
                    manifest: false,
                    local_user_id: None,
                },
                vec![PromptField::LocalUserId],
            ),
        ];

        for (action, expected) in cases {
            assert_eq!(missing_fields(&action), expected);
        }
    }

    #[test]
    fn eof_cancels_before_any_registry_mutation() {
        let action = WorkspaceAction::Attach { root: None };
        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut writer = Vec::new();

        let error =
            collect_from(&action, &mut reader, &mut writer, Theme::dark(false)).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cancelled before the registry changed")
        );
        assert_eq!(String::from_utf8(writer).unwrap(), "Workspace root: ");
    }

    #[test]
    fn blank_required_values_retry_and_multiple_answers_are_collected() {
        let action = WorkspaceAction::Rename {
            workspace: None,
            name: None,
        };
        let mut reader = Cursor::new(b"\nfamily\n\nshared\n".to_vec());
        let mut writer = Vec::new();

        let answers = collect_from(&action, &mut reader, &mut writer, Theme::dark(false)).unwrap();

        assert_eq!(answers.value(PromptField::Workspace), Some("family"));
        assert_eq!(answers.value(PromptField::Name), Some("shared"));
        let output = String::from_utf8(writer).unwrap();
        assert_eq!(output.matches("A value is required.").count(), 2);
        assert!(output.contains("Workspace name or alias:"));
        assert!(output.contains("New workspace name:"));
    }

    #[test]
    fn blank_optional_create_name_uses_the_root_basename() {
        let action = WorkspaceAction::Create {
            name: None,
            root: None,
        };
        let mut reader = Cursor::new(b"/brains/family\n\n".to_vec());
        let mut writer = Vec::new();

        let answers = collect_from(&action, &mut reader, &mut writer, Theme::dark(false)).unwrap();

        assert_eq!(answers.value(PromptField::Root), Some("/brains/family"));
        assert_eq!(answers.value(PromptField::Name), None);
    }

    #[test]
    fn prompt_read_and_write_failures_keep_io_context() {
        let action = WorkspaceAction::Attach { root: None };
        let mut read_failure = FailingReader;
        let mut output = Vec::new();
        let error =
            collect_from(&action, &mut read_failure, &mut output, Theme::dark(false)).unwrap_err();
        assert!(error.to_string().contains("read workspace prompt"));

        let mut input = Cursor::new(b"/brains/family\n".to_vec());
        let mut write_failure = FailingWriter;
        let error =
            collect_from(&action, &mut input, &mut write_failure, Theme::dark(false)).unwrap_err();
        assert!(error.to_string().contains("write workspace prompt"));
    }
}

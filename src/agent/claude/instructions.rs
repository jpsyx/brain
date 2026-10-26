//! Compose configured Claude append content without retaining conflicting flags.

use crate::agent::{AgentError, command_probe::ShellProbeRunner};
use std::{io::Read as _, path::Path};

const DIAGNOSTIC: &str = "cannot preserve Claude launch instructions; use a direct claude_cmd with literal --append-system-prompt or --append-system-prompt-file arguments";
fn error() -> AgentError {
    AgentError::Frontend(DIAGNOSTIC.to_owned())
}

pub(super) fn prepare(
    command: &str,
    root: &Path,
    environment: &[(String, String)],
    payload: &str,
) -> Result<(String, String), AgentError> {
    if !crate::agent::direct_command::accepts_appended_options(command) {
        return Err(error());
    }
    let words = crate::agent::direct_command::parse_words(command).ok_or_else(error)?;
    let help = ShellProbeRunner::default()
        .run_at_with_environment(command, &["--help"], Some(root), Some(environment))
        .map_err(|_| error())?;
    if !help.success
        || !help
            .stdout
            .lines()
            .any(|line| line.contains("--system-prompt-snapshot") && line.contains("off"))
    {
        return Err(AgentError::Frontend("Claude must support --system-prompt-snapshot off to refresh Brain instructions on resume; update Claude Code or select a compatible claude_cmd before launching".to_owned()));
    }
    let mut removed = Vec::new();
    let mut inline = None;
    let mut file = None;
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        let (flag, equals_value) = word
            .value
            .split_once('=')
            .map_or((word.value.as_str(), None), |(flag, value)| {
                (flag, Some(value))
            });
        let target = match flag {
            "--append-system-prompt" => &mut inline,
            "--append-system-prompt-file" => &mut file,
            _ => {
                index += 1;
                continue;
            }
        };
        let mut span = word.span.clone();
        let value = if let Some(value) = equals_value {
            value.to_owned()
        } else {
            index += 1;
            let next = words.get(index).ok_or_else(error)?;
            span.end = next.span.end;
            next.value.clone()
        };
        *target = Some(value);
        removed.push(span);
        index += 1;
    }
    if inline.is_none() && file.is_none() {
        return Ok((command.to_owned(), payload.to_owned()));
    }
    if inline.is_some() && file.is_some() {
        return Err(error());
    }
    let prior = if let Some(path) = file {
        let mut contents = String::new();
        std::fs::File::open(root.join(path))
            .and_then(|file| {
                file.take(crate::agent::frontend::SHELL_INSTRUCTION_BUDGET_BYTES as u64 + 1)
                    .read_to_string(&mut contents)
            })
            .map_err(|_| error())?;
        Some(contents)
    } else {
        inline
    };
    let combined = prior.map_or_else(
        || payload.to_owned(),
        |prior| format!("{prior}\n\n{payload}"),
    );
    crate::agent::instructions::validate_size(&combined)?;
    let mut prefix = command.to_owned();
    for span in removed.into_iter().rev() {
        prefix.replace_range(span, "");
    }
    Ok((prefix.trim_end().to_owned(), combined))
}

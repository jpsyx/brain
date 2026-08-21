use std::io::{BufRead, BufReader, Write};

use anyhow::Result;

use crate::theme::Theme;
use crate::workspace::{WorkspaceId, WorkspaceName};

use super::parse_yes_no;

/// Read one line from `/dev/tty`, prompting with `label` (showing `current` as
/// the default). Empty input keeps `current`; non-empty input is trimmed and
/// used. Same open-the-controlling-terminal pattern as
/// `personalization::onboarding`, so the prompt works even when the TUI owns
/// /dev/tty and regardless of stdin redirection.
///
/// `pub(crate)` so `sync::command`'s interactive `resolve` picker can reuse
/// this rather than reimplementing the /dev/tty dance.
pub(crate) fn prompt(label: &str, current: &str) -> Result<String> {
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
    let mut out = tty.try_clone()?;
    let mut reader = BufReader::new(tty);

    if current.is_empty() {
        write!(out, "  {label}: ")?;
    } else {
        write!(out, "  {label} [{current}]: ")?;
    }
    out.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let trimmed = line.trim();
    Ok(if trimmed.is_empty() {
        current.to_owned()
    } else {
        trimmed.to_owned()
    })
}

/// Ask whether the user already has a bucket. Thin `/dev/tty` shell over
/// [`parse_yes_no`]; a bare Enter means "no" (show the walkthrough).
pub(super) fn ask_has_bucket(theme: Theme) -> Result<bool> {
    let answer = prompt(
        &theme.prompt("Do you already have a Backblaze private bucket to connect to? [y/N]"),
        "",
    )?;
    Ok(parse_yes_no(&answer))
}

pub(super) fn confirm_manifestless_adoption(
    theme: Theme,
    local_name: &WorkspaceName,
    local_workspace_id: WorkspaceId,
) -> Result<bool> {
    let question = theme.prompt(&format!(
        "Adopt this nonempty remote as workspace {local_name} ({local_workspace_id})? [y/N]"
    ));
    let answer = prompt(&question, "")?;
    Ok(parse_yes_no(&answer))
}

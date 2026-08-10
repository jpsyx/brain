//! The workspace selector to echo back inside suggested commands.
//!
//! A message that tells someone to run `brain sync setup` while they are
//! working in `family` sends them to the wrong workspace: with no `-w`, the
//! suggestion targets the default. Every command Brain suggests therefore
//! carries the selector for the workspace the command was actually about.
//!
//! Bootstrap records the selected canonical name once; message builders read it
//! through [`suggest`]. This mirrors [`crate::theme::Theme::active`]: a
//! presentation-only fact about the current process, read wherever output is
//! composed rather than threaded through every intermediate signature. The
//! decision itself is the pure [`with_selector`].

use std::sync::OnceLock;

use super::WorkspaceName;

static SELECTED: OnceLock<String> = OnceLock::new();

/// Record the workspace every later suggestion should point at. First write
/// wins: one process resolves exactly one selected workspace.
pub(crate) fn remember_selected(name: &WorkspaceName) {
    let _ = SELECTED.set(name.as_str().to_owned());
}

/// A `brain` command line that targets the workspace this process selected.
///
/// `command` is the part after `brain`, e.g. `"sync setup"`.
#[must_use]
pub fn suggest(command: &str) -> String {
    with_selector(command, SELECTED.get().map(String::as_str))
}

/// Append the workspace selector to a suggested command. Pure.
///
/// The selector goes last so the command reads the way a user would type it,
/// and is omitted entirely when no workspace has been selected (registry-only
/// commands, `--help`) rather than guessing a name.
#[must_use]
pub(crate) fn with_selector(command: &str, workspace: Option<&str>) -> String {
    let command = command.trim();
    match workspace {
        Some(workspace) if !workspace.trim().is_empty() => {
            format!("brain {command} -w {}", workspace.trim())
        }
        _ => format!("brain {command}"),
    }
}

#[cfg(test)]
mod tests {
    use super::with_selector;

    #[test]
    fn a_selected_workspace_is_echoed_back_in_the_suggestion() {
        // The complaint this exists for: being told to run `brain sync setup`
        // while working in `family` points at the default workspace instead.
        assert_eq!(
            with_selector("sync setup", Some("family")),
            "brain sync setup -w family"
        );
    }

    #[test]
    fn the_selector_lands_where_a_user_would_type_it() {
        assert_eq!(
            with_selector("workspace repair --manifest", Some("family")),
            "brain workspace repair --manifest -w family"
        );
    }

    #[test]
    fn with_no_selected_workspace_the_suggestion_stays_bare() {
        // Registry-only commands and `--help` have no workspace; inventing one
        // would suggest a command about a workspace the user never named.
        assert_eq!(with_selector("sync", None), "brain sync");
        assert_eq!(with_selector("sync", Some("   ")), "brain sync");
    }

    #[test]
    fn surrounding_whitespace_never_reaches_the_rendered_command() {
        assert_eq!(
            with_selector("  sync repair  ", Some(" family ")),
            "brain sync repair -w family"
        );
    }
}

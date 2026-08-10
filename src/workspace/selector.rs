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

/// Set on every child process Brain spawns for its own work, so a code path that
/// forgets `-w` fails loudly instead of silently operating on the default
/// workspace.
pub const STRICT_ENV: &str = "BRAIN_REQUIRE_WORKSPACE";

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

/// The environment variable that names the workspace a Brain-launched process
/// belongs to.
///
/// Exported to every agent panel, hook, and reindex child by
/// [`crate::workspace::WorkspaceContext::integration_env`], so a skill that runs
/// `brain todo` inside a `family` panel operates on `family` without having to
/// remember `-w`.
pub const WORKSPACE_ENV: &str = "BRAIN_WORKSPACE";

/// The workspace an invocation actually selects. Pure.
///
/// An explicit `-w` always wins: it is the user (or Brain) saying so. Otherwise
/// a Brain-launched process inherits the workspace it was launched for. Only a
/// process with neither falls back to the machine default, which is what a
/// person typing `brain` at a prompt wants.
#[must_use]
pub(crate) fn effective_selector(
    explicit: Option<&str>,
    inherited: Option<&str>,
) -> Option<String> {
    explicit
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
        .or_else(|| inherited.map(str::trim).filter(|name| !name.is_empty()))
        .map(str::to_owned)
}

/// The workspace whose root contains `current_dir`, nearest ancestor first. Pure.
///
/// Discovery walks upward from the current directory the way git finds its
/// repository, so working inside `~/family` makes `brain …` act on `family`
/// without a flag. `roots` pairs each canonical workspace name with its
/// registered root; both sides are compared as given, so callers resolve
/// symlinks before calling (a `/tmp` root and a `/private/tmp` cwd are the same
/// directory on macOS and must compare equal).
///
/// The registry forbids overlapping roots, so at most one can match — but
/// nearest-ancestor is still the right rule, and stays correct if that ever
/// changes.
#[must_use]
pub(crate) fn discover_from_ancestors(
    roots: &[(String, std::path::PathBuf)],
    current_dir: &std::path::Path,
) -> Option<String> {
    current_dir.ancestors().find_map(|ancestor| {
        roots
            .iter()
            .find(|(_, root)| root == ancestor)
            .map(|(name, _)| name.clone())
    })
}

/// Whether an invocation must be refused for naming no workspace. Pure.
///
/// Only strict children are affected. An interactive `brain` deliberately falls
/// back to the default workspace; the point of strict mode is that Brain's *own*
/// spawned commands can never do that by accident.
#[must_use]
pub(crate) const fn violates_strict_selector(strict: bool, has_selector: bool) -> bool {
    strict && !has_selector
}

/// Whether this process was spawned by Brain under strict selector rules.
#[must_use]
pub(crate) fn strict_selector_required() -> bool {
    std::env::var_os(STRICT_ENV).is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        discover_from_ancestors, effective_selector, violates_strict_selector, with_selector,
    };

    fn roots() -> Vec<(String, PathBuf)> {
        vec![
            ("brain".to_owned(), PathBuf::from("/home/tester/brain")),
            ("family".to_owned(), PathBuf::from("/home/tester/family")),
        ]
    }

    #[test]
    fn standing_in_a_workspace_root_selects_it() {
        assert_eq!(
            discover_from_ancestors(&roots(), Path::new("/home/tester/family")).as_deref(),
            Some("family")
        );
    }

    #[test]
    fn standing_deep_inside_a_workspace_selects_it() {
        // The git behavior: any depth below the root still finds it.
        assert_eq!(
            discover_from_ancestors(
                &roots(),
                Path::new("/home/tester/family/projects/work__thing/notes")
            )
            .as_deref(),
            Some("family")
        );
    }

    #[test]
    fn standing_outside_every_root_selects_nothing() {
        assert_eq!(
            discover_from_ancestors(&roots(), Path::new("/home/tester/src/other")),
            None
        );
        // A parent of the roots is not inside any of them.
        assert_eq!(
            discover_from_ancestors(&roots(), Path::new("/home/tester")),
            None
        );
    }

    #[test]
    fn the_nearest_ancestor_wins_over_a_shallower_one() {
        // Overlapping roots are rejected by the registry, but the rule must be
        // nearest-ancestor regardless of what the registry allows later.
        let nested = vec![
            ("outer".to_owned(), PathBuf::from("/home/tester")),
            ("inner".to_owned(), PathBuf::from("/home/tester/family")),
        ];

        assert_eq!(
            discover_from_ancestors(&nested, Path::new("/home/tester/family/areas")).as_deref(),
            Some("inner")
        );
    }

    #[test]
    fn an_explicit_selector_always_wins() {
        assert_eq!(
            effective_selector(Some("family"), Some("brain")).as_deref(),
            Some("family")
        );
    }

    #[test]
    fn a_brain_launched_process_inherits_the_workspace_it_was_launched_for() {
        // The skill case: a `family` agent panel runs `brain todo` with no `-w`,
        // and it must operate on `family` rather than the machine default.
        assert_eq!(
            effective_selector(None, Some("family")).as_deref(),
            Some("family")
        );
    }

    #[test]
    fn a_person_at_a_prompt_still_gets_the_machine_default() {
        assert_eq!(effective_selector(None, None), None);
    }

    #[test]
    fn a_blank_selector_on_either_side_is_no_selector() {
        assert_eq!(effective_selector(Some("  "), None), None);
        assert_eq!(effective_selector(None, Some("")).as_deref(), None);
        // A blank explicit value falls through to the inherited one.
        assert_eq!(
            effective_selector(Some(" "), Some("family")).as_deref(),
            Some("family")
        );
    }

    #[test]
    fn a_strict_child_that_names_no_workspace_is_refused() {
        // The bug class this catches: Brain spawning `brain sync` with no `-w`
        // and silently syncing whichever workspace happens to be the default.
        assert!(violates_strict_selector(true, false));
    }

    #[test]
    fn a_strict_child_that_names_one_is_allowed() {
        assert!(!violates_strict_selector(true, true));
    }

    #[test]
    fn an_ordinary_invocation_may_still_use_the_default_workspace() {
        assert!(!violates_strict_selector(false, false));
        assert!(!violates_strict_selector(false, true));
    }

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

//! First-run onboarding: a short, skippable prompt that seeds one persona.
//!
//! Runs on **any** `brain` command when this machine's local person has no
//! persona yet, and on bare `brain persona`. The pure decisions
//! (`from_answers`, `prompts_for_missing_persona`, `missing_persona_notice`) are
//! unit-tested; the `/dev/tty` interaction is a thin shell that never fails a
//! command — with no terminal it prints one actionable line instead of blocking.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};

use anyhow::Result;

use super::persona::Persona;
use super::{command, store};
use crate::workspace::Invocation;

/// The onboarding questions, in order: (field, prompt).
const QUESTIONS: [(&str, &str); 3] = [
    ("name", "Your name"),
    ("role", "Your role (e.g. CEO, engineer, student)"),
    ("works_for", "Who you work for (org, \"myself\", or blank)"),
];

/// Assemble a `Persona` from raw answers; blank answers stay empty
/// (skipped). Pure.
#[must_use]
pub fn from_answers(name: &str, role: &str, works_for: &str) -> Persona {
    Persona {
        name: name.trim().to_owned(),
        role: role.trim().to_owned(),
        works_for: works_for.trim().to_owned(),
        ..Persona::default()
    }
}

/// Bare `brain persona`: run onboarding if the local person has nothing set,
/// else `show` them.
pub fn run_or_show(workspace: &crate::workspace::WorkspaceContext) -> Result<()> {
    if store::local_persona(workspace).is_empty() {
        run_interactive(workspace)
    } else {
        command::run_show(workspace, None)
    }
}

/// Whether an invocation may stop to collect a missing persona.
///
/// Every ordinary command does: a workspace member with no persona is something
/// brain should fix the next time it sees them, not something they must
/// remember to do. The exceptions are the commands already editing personas
/// (`brain persona …`, which would prompt for what it is about to be told) and
/// `brain workspace migrate`, which must not interleave prompts with a
/// transactional schema change. Pure.
#[must_use]
pub const fn prompts_for_missing_persona(invocation: Invocation) -> bool {
    !matches!(
        invocation,
        Invocation::Persona | Invocation::WorkspaceMigrate
    )
}

/// The one-line nudge printed when there is no terminal to prompt on.
///
/// Names the exact command to run, so a scripted or piped invocation still
/// tells the user how to finish setup. Pure.
#[must_use]
pub fn missing_persona_notice(
    user_id: &str,
    workspace: &str,
    theme: crate::theme::Theme,
) -> String {
    theme.warning(&format!(
        "{user_id} has no persona yet — run `brain persona set role=<ROLE> -w {workspace}` (or `brain persona -w {workspace}`) so brain knows who it is assisting."
    ))
}

/// Command hook: collect this machine's person's persona when it is missing.
///
/// Never fails the command that triggered it. With a terminal it runs the same
/// short onboarding as `brain persona`; without one it prints
/// [`missing_persona_notice`] to stderr and continues.
pub fn prompt_for_missing_local_persona(workspace: &crate::workspace::WorkspaceContext) {
    if !store::local_persona(workspace).is_empty() {
        return;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!(
            "{}",
            missing_persona_notice(
                workspace.local_user_id(),
                workspace.name().as_str(),
                crate::theme::Theme::active(),
            )
        );
        return;
    }
    if let Err(error) = run_interactive(workspace) {
        crate::logging::log(format!("persona onboarding skipped: {error:#}"));
    }
}

fn run_interactive(workspace: &crate::workspace::WorkspaceContext) -> Result<()> {
    // Open the controlling terminal directly so the prompt works even when the
    // TUI owns /dev/tty and regardless of stdin redirection. No tty → skip.
    let Ok(tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    else {
        return Ok(());
    };
    let mut out = tty.try_clone()?;
    let mut reader = BufReader::new(tty);

    writeln!(
        out,
        "\nLet's set up {}'s persona (press Enter to skip any).\n",
        workspace.local_user_id()
    )?;
    let mut answers = [String::new(), String::new(), String::new()];
    for (i, (_, prompt)) in QUESTIONS.iter().enumerate() {
        write!(out, "  {prompt}: ")?;
        out.flush()?;
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break; // EOF
        }
        line.trim().clone_into(&mut answers[i]);
    }

    let mut p = from_answers(&answers[0], &answers[1], &answers[2]);

    // Then the two toggle-checklists (all-on by default, `a` to add). Each
    // returns None on cancel or when there's no terminal — then we leave that
    // facet unset (falls back to the generic defaults).
    writeln!(
        out,
        "\nNext, pick your project namespaces and task tags (space toggles, `a` adds).\n"
    )?;
    out.flush()?;
    if let Some(ns) = super::checklist::choose(
        "Project namespaces",
        &super::namespaces::default_namespaces(),
        super::namespaces::normalize,
    )? {
        p.namespaces = ns;
    }
    if let Some(tag_names) = super::checklist::choose(
        "Task tags",
        &super::tags::default_tag_names(),
        super::tags::normalize_tag,
    )? {
        p.tag_styles = super::tags::styles_from_names(&tag_names, &BTreeMap::new());
    }

    if p.is_empty() {
        writeln!(
            out,
            "\nSkipped — run `{}` anytime.\n",
            crate::workspace::suggest("persona")
        )?;
        return Ok(());
    }
    store::save_persona(workspace, workspace.local_user_id(), &p)?;
    crate::skills::resync_skills(workspace);
    writeln!(
        out,
        "\nSaved. Change it anytime with `{}`.\n",
        crate::workspace::suggest("persona")
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_answers_trims_and_builds() {
        let p = from_answers("  Pablo ", "CEO", " Avandar ");
        assert_eq!(p.name, "Pablo");
        assert_eq!(p.role, "CEO");
        assert_eq!(p.works_for, "Avandar");
    }

    #[test]
    fn all_blank_answers_are_an_empty_persona() {
        assert!(from_answers("", "  ", "\t").is_empty());
    }

    #[test]
    fn ordinary_commands_stop_to_collect_a_missing_persona() {
        for invocation in [
            Invocation::Tui,
            Invocation::Tasks,
            Invocation::Config,
            Invocation::Sync,
            Invocation::Habits,
        ] {
            assert!(prompts_for_missing_persona(invocation), "{invocation:?}");
        }
    }

    #[test]
    fn persona_and_migration_commands_never_prompt_first() {
        // `brain persona …` is already collecting it, and a migration must not
        // interleave prompts with a transactional schema change.
        assert!(!prompts_for_missing_persona(Invocation::Persona));
        assert!(!prompts_for_missing_persona(Invocation::WorkspaceMigrate));
    }

    #[test]
    fn the_headless_notice_names_the_person_and_the_exact_command() {
        let notice = missing_persona_notice("pablo", "family", crate::theme::Theme::dark(false));

        assert!(notice.contains("pablo has no persona yet"), "{notice}");
        assert!(
            notice.contains("brain persona set role=<ROLE> -w family"),
            "{notice}"
        );
    }

    #[test]
    fn partial_answers_keep_only_the_provided_field() {
        let p = from_answers("", "student", "");
        assert_eq!(p.role, "student");
        assert!(p.name.is_empty());
        assert!(p.works_for.is_empty());
        assert!(!p.is_empty());
    }
}

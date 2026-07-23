//! First-run onboarding: a short, skippable prompt that seeds personalization.
//!
//! Runs on the first `brain` startup when nothing is set, and on bare
//! `brain personalize`. The pure assembly (`from_answers`) is unit-tested; the
//! `/dev/tty` interaction is a thin shell that silently no-ops without a real
//! terminal (CI, pipes) so it never blocks startup.

use std::io::{BufRead, BufReader, Write};

use anyhow::Result;

use super::model::Personalization;
use super::{command, store};

/// The onboarding questions, in order: (field, prompt).
const QUESTIONS: [(&str, &str); 3] = [
    ("name", "Your name"),
    ("role", "Your role (e.g. CEO, engineer, student)"),
    ("works_for", "Who you work for (org, \"myself\", or blank)"),
];

/// Assemble a `Personalization` from raw answers; blank answers stay empty
/// (skipped). Pure.
#[must_use]
pub fn from_answers(name: &str, role: &str, works_for: &str) -> Personalization {
    Personalization {
        name: name.trim().to_owned(),
        role: role.trim().to_owned(),
        works_for: works_for.trim().to_owned(),
        ..Personalization::default()
    }
}

/// Bare `brain personalize`: run onboarding if nothing is set, else `show`.
pub fn run_or_show() -> Result<()> {
    if store::load().is_empty() {
        run_interactive()
    } else {
        command::run_show();
        Ok(())
    }
}

/// Startup hook: run onboarding only when nothing is set. Never fails a startup
/// — any error (including no terminal) is swallowed.
pub fn maybe_run_first_time() {
    if store::load().is_empty() {
        let _ = run_interactive();
    }
}

fn run_interactive() -> Result<()> {
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

    writeln!(out, "\nLet's personalize brain (press Enter to skip any).\n")?;
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

    let p = from_answers(&answers[0], &answers[1], &answers[2]);
    if p.is_empty() {
        writeln!(out, "\nSkipped — run `brain personalize` anytime.\n")?;
        return Ok(());
    }
    store::save(&p)?;
    crate::skills::resync_skills();
    writeln!(out, "\nSaved. Change it anytime with `brain personalize`.\n")?;
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
    fn all_blank_answers_are_empty_personalization() {
        assert!(from_answers("", "  ", "\t").is_empty());
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

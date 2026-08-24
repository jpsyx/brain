//! `brain tasks sync-agenda` — the one deterministic agenda-sync entry point.
//!
//! Native completion calls the same code in-process; this exposes it to every
//! other mutator (the `/todo` scripts, an agent, a person) so there is exactly
//! one implementation of the section-preserving sync.

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};

use crate::tasks::agenda::{self, Outcome};
use crate::tasks::cli::{AgendaAction, AgendaAppendixArgs, SyncAgendaArgs};
use crate::theme::Theme;

/// The agenda's date: an explicit `YYYY-MM-DD`, else today.
fn parse_date(raw: Option<&str>) -> Result<NaiveDate> {
    raw.map_or_else(
        || Ok(Local::now().date_naive()),
        |raw| {
            NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
                .with_context(|| format!("'{raw}' is not a date (expected YYYY-MM-DD)"))
        },
    )
}

pub(super) fn run(context: &crate::workspace::CommandContext, args: &SyncAgendaArgs) -> Result<()> {
    let date = parse_date(args.date.as_deref())?;
    let id = args
        .id
        .as_deref()
        .map(crate::tasks::complete::normalize_id)
        .transpose()?;
    let action = action_for(args.action, id.is_some());
    let outcome = agenda::sync_after_command_mutation(
        context,
        id.as_deref().unwrap_or_default(),
        action,
        date,
    );
    eprintln!("{}", report(outcome, date, Theme::active()));
    Ok(())
}

/// `brain tasks agenda-appendix --content <file>`.
///
/// Bakes caller-supplied markdown into the day's agenda as one generic marked
/// section. Core never discovers the content: the caller names the file.
pub(super) fn run_appendix(
    context: &crate::workspace::CommandContext,
    args: &AgendaAppendixArgs,
) -> Result<()> {
    let date = parse_date(args.date.as_deref())?;
    let targets = agenda::resolve_targets(&context.registry_store, &context.workspace, date);
    let theme = Theme::active();
    if !targets.markdown.exists() {
        eprintln!(
            "{}",
            theme.muted(&format!("No agenda for {date}; nothing to bake into."))
        );
        return Ok(());
    }
    let appendix = std::fs::read_to_string(&args.content)
        .with_context(|| format!("read appendix content {}", args.content.display()))?;
    let existing = std::fs::read_to_string(&targets.markdown)
        .with_context(|| format!("read agenda {}", targets.markdown.display()))?;
    std::fs::write(
        &targets.markdown,
        crate::tasks::agenda::appendix::bake(&existing, &appendix),
    )
    .with_context(|| format!("write agenda {}", targets.markdown.display()))?;
    eprintln!(
        "{}",
        theme.success(&format!("Baked the appendix into the {date} agenda."))
    );
    Ok(())
}

/// With no id there is nothing to drop, so the plan-editing actions collapse to
/// a snapshot refresh.
pub(super) const fn action_for(requested: AgendaAction, has_id: bool) -> agenda::Action {
    if !has_id {
        return agenda::Action::Touch;
    }
    match requested {
        AgendaAction::Done => agenda::Action::Done,
        AgendaAction::Defer => agenda::Action::Defer,
        AgendaAction::Touch => agenda::Action::Touch,
    }
}

pub(super) fn report(outcome: Outcome, date: NaiveDate, theme: Theme) -> String {
    match outcome {
        Outcome::NoAgenda => theme.muted(&format!("No agenda for {date}; nothing to sync.")),
        Outcome::Unchanged => theme.muted(&format!("The {date} agenda was already accurate.")),
        Outcome::Updated { pdf: false } => theme.success(&format!("Synced the {date} agenda.")),
        Outcome::Updated { pdf: true } => {
            theme.success(&format!("Synced the {date} agenda and its printable."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{action_for, report};
    use crate::tasks::agenda::{Action, Outcome};
    use crate::tasks::cli::AgendaAction;
    use crate::theme::Theme;

    fn date() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date")
    }

    #[test]
    fn an_omitted_id_can_only_refresh_the_snapshots() {
        assert_eq!(action_for(AgendaAction::Done, false), Action::Touch);
        assert_eq!(action_for(AgendaAction::Defer, false), Action::Touch);
    }

    #[test]
    fn a_supplied_id_carries_the_requested_action() {
        assert_eq!(action_for(AgendaAction::Done, true), Action::Done);
        assert_eq!(action_for(AgendaAction::Defer, true), Action::Defer);
        assert_eq!(action_for(AgendaAction::Touch, true), Action::Touch);
    }

    #[test]
    fn every_outcome_reads_plainly() {
        let theme = Theme::dark(false);
        assert_eq!(
            report(Outcome::NoAgenda, date(), theme),
            "No agenda for 2026-08-24; nothing to sync."
        );
        assert_eq!(
            report(Outcome::Unchanged, date(), theme),
            "The 2026-08-24 agenda was already accurate."
        );
        assert_eq!(
            report(Outcome::Updated { pdf: false }, date(), theme),
            "Synced the 2026-08-24 agenda."
        );
        assert_eq!(
            report(Outcome::Updated { pdf: true }, date(), theme),
            "Synced the 2026-08-24 agenda and its printable."
        );
    }
}

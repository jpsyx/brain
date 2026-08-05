//! Tasks, habits, and reindex command grammar.

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct HabitsArgs {
    #[command(subcommand)]
    pub action: Option<HabitsAction>,
}

#[derive(Subcommand, Debug)]
pub enum HabitsAction {
    /// Respawn a lapsed habit by fuzzy name (alias: `fix`). A lapsed habit is
    /// one whose every occurrence is `done` with none pending, so it silently
    /// dropped off the agenda. Multiple words are joined, so quotes are
    /// optional: `brain habits revive send team status update`.
    #[command(alias = "fix")]
    Revive(ReviveArgs),

    /// Skip a habit for today (cadence-aware). Accepts an id (`H43`, `43`) or a
    /// fuzzy name. A daily habit is marked done + respawned; a non-daily habit
    /// is deferred one day. `--until YYYY-MM-DD` defers to that date instead,
    /// for either cadence (never marking done).
    Skip(SkipArgs),

    /// Complete Brain's protected daily or weekly managed triage occurrence
    /// deterministically: mark today's occurrence done and spawn the next, the
    /// exact mutation the daily-triage modal's Skip button performs. No agent,
    /// no judgement. A no-op when managed triage habits are disabled.
    CompleteManagedTriage(CompleteManagedTriageArgs),
}

#[derive(Args, Debug)]
pub struct CompleteManagedTriageArgs {
    /// Which managed triage chain to complete.
    #[arg(value_enum)]
    pub kind: ManagedTriageKindArg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum ManagedTriageKindArg {
    Daily,
    Weekly,
}

impl From<ManagedTriageKindArg> for crate::tasks::triage_habits::ManagedTriageKind {
    fn from(value: ManagedTriageKindArg) -> Self {
        match value {
            ManagedTriageKindArg::Daily => Self::Daily,
            ManagedTriageKindArg::Weekly => Self::Weekly,
        }
    }
}

#[derive(Args, Debug)]
pub struct ReviveArgs {
    /// The fuzzy habit name to match (all trailing words are joined with spaces).
    #[arg(required = true, num_args = 1..)]
    pub query: Vec<String>,
}

#[derive(Args, Debug)]
pub struct SkipArgs {
    /// Habit id (`H43`, `43`) or a fuzzy name fragment.
    pub id: String,

    /// Defer the habit until this day (`YYYY-MM-DD`) instead of applying the
    /// cadence default; never marks it done. Must be strictly after today.
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub until: Option<String>,
}

#[derive(Args, Debug)]
pub struct ReindexArgs {
    /// Rebuild only `projects-lookup.csv`.
    #[arg(long)]
    pub projects: bool,
    /// Rebuild only `zotero-lookup.csv`.
    #[arg(long)]
    pub resources: bool,
    /// Re-apply only the task/habit automation rules.
    #[arg(long)]
    pub tasks: bool,
}

#[derive(Args, Debug)]
pub struct TasksArgs {
    /// Everything after `tasks`, handed to the tasks CLI parser unchanged
    /// (positional view/date/search tokens, filter flags, and the
    /// `complete` / `doctor` / `search` subcommands).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::HabitsAction;
    use crate::cli::{Cli, Cmd};

    #[test]
    fn reindex_flags_parse_and_default_to_all() {
        let bare = Cli::try_parse_from(["brain", "reindex"]).expect("parse");
        assert!(matches!(bare.command, Some(Cmd::Reindex(args))
            if !args.projects && !args.resources && !args.tasks));
        let scoped = Cli::try_parse_from(["brain", "reindex", "--resources"]).expect("parse");
        assert!(matches!(scoped.command, Some(Cmd::Reindex(args))
            if args.resources && !args.projects && !args.tasks));
    }

    #[test]
    fn bare_habits_has_no_action() {
        let cli = Cli::try_parse_from(["brain", "habits"]).expect("parse");
        assert!(matches!(cli.command, Some(Cmd::Habits(args)) if args.action.is_none()));
    }

    #[test]
    fn habits_revive_joins_trailing_words() {
        let cli = Cli::try_parse_from([
            "brain", "habits", "revive", "send", "team", "status", "update",
        ])
        .expect("parse");
        let Some(Cmd::Habits(args)) = cli.command else {
            panic!("expected habits");
        };
        let Some(HabitsAction::Revive(revive)) = args.action else {
            panic!("expected revive");
        };
        assert_eq!(revive.query, vec!["send", "team", "status", "update"]);
    }

    #[test]
    fn habits_fix_is_an_alias_for_revive() {
        let cli = Cli::try_parse_from(["brain", "habits", "fix", "meds"]).expect("parse");
        let Some(Cmd::Habits(args)) = cli.command else {
            panic!("expected habits");
        };
        let Some(HabitsAction::Revive(revive)) = args.action else {
            panic!("expected revive");
        };
        assert_eq!(revive.query, vec!["meds"]);
    }

    #[test]
    fn habits_revive_requires_a_query() {
        assert!(Cli::try_parse_from(["brain", "habits", "revive"]).is_err());
    }

    #[test]
    fn habits_skip_parses_id() {
        let cli = Cli::try_parse_from(["brain", "habits", "skip", "H35"]).expect("parse");
        let Some(Cmd::Habits(args)) = cli.command else {
            panic!("expected habits");
        };
        let Some(HabitsAction::Skip(skip)) = args.action else {
            panic!("expected skip");
        };
        assert_eq!(skip.id, "H35");
        assert!(skip.until.is_none());
    }

    #[test]
    fn habits_skip_parses_until() {
        let cli = Cli::try_parse_from(["brain", "habits", "skip", "H35", "--until", "2026-08-10"])
            .expect("parse");
        let Some(Cmd::Habits(args)) = cli.command else {
            panic!("expected habits");
        };
        let Some(HabitsAction::Skip(skip)) = args.action else {
            panic!("expected skip");
        };
        assert_eq!(skip.until.as_deref(), Some("2026-08-10"));
    }

    #[test]
    fn habits_skip_requires_an_id() {
        assert!(Cli::try_parse_from(["brain", "habits", "skip"]).is_err());
    }

    #[test]
    fn habits_complete_managed_triage_parses_daily_and_weekly() {
        use super::{CompleteManagedTriageArgs, HabitsAction, ManagedTriageKindArg};

        for (word, expected) in [
            ("daily", ManagedTriageKindArg::Daily),
            ("weekly", ManagedTriageKindArg::Weekly),
        ] {
            let cli =
                Cli::try_parse_from(["brain", "habits", "complete-managed-triage", word]).expect("parse");
            let Some(Cmd::Habits(args)) = cli.command else {
                panic!("expected habits");
            };
            let Some(HabitsAction::CompleteManagedTriage(CompleteManagedTriageArgs { kind })) =
                args.action
            else {
                panic!("expected complete-managed-triage");
            };
            assert_eq!(kind, expected);
        }
    }

    #[test]
    fn habits_complete_managed_triage_rejects_unknown_kind() {
        assert!(
            Cli::try_parse_from(["brain", "habits", "complete-managed-triage", "monthly"]).is_err()
        );
        assert!(Cli::try_parse_from(["brain", "habits", "complete-managed-triage"]).is_err());
    }

    #[test]
    fn habits_help_explains_cadence_and_until_constraints() {
        let habits = Cli::try_parse_from(["brain", "habits", "--help"])
            .unwrap_err()
            .to_string();
        assert!(habits.contains("cadence-aware"));
        assert!(habits.contains("daily habit is marked done + respawned"));

        let skip = Cli::try_parse_from(["brain", "habits", "skip", "--help"])
            .unwrap_err()
            .to_string();
        assert!(skip.contains("Must be strictly after today"));
        assert!(skip.contains("never marks it done"));
    }
}

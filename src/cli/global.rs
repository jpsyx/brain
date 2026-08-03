//! Global flags and parser normalization.

use clap::Parser;

use super::Cmd;

pub(super) fn normalize_codex_aliases<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    args.into_iter()
        .map(Into::into)
        .map(|arg| {
            if arg == "-cx" {
                "--codex".to_owned()
            } else {
                arg
            }
        })
        .collect()
}

pub(super) fn normalize_global_args<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    extract_workspace_selectors(normalize_codex_aliases(args))
}

fn extract_workspace_selectors(args: Vec<String>) -> Vec<String> {
    let Some((program, tail)) = args.split_first() else {
        return args;
    };
    let mut selectors = Vec::new();
    let mut delegated = Vec::new();
    let mut index = 0;
    while index < tail.len() {
        let argument = &tail[index];
        if argument == "--" {
            delegated.extend_from_slice(&tail[index..]);
            break;
        }
        if argument == "--brain" || argument == "-b" {
            selectors.push(argument.clone());
            let Some(value) = tail.get(index + 1) else {
                return vec![program.clone(), argument.clone()];
            };
            if value == "--" || value.starts_with('-') {
                return vec![program.clone(), argument.clone()];
            }
            selectors.push(value.clone());
            index += 2;
            continue;
        }
        if argument.starts_with("--brain=") {
            selectors.push(argument.clone());
        } else {
            delegated.push(argument.clone());
        }
        index += 1;
    }

    let mut normalized = Vec::with_capacity(args.len());
    normalized.push(program.clone());
    normalized.extend(selectors);
    normalized.extend(delegated);
    normalized
}

#[derive(Parser, Debug)]
#[command(
    name = "brain",
    version = env!("CARGO_PKG_VERSION"),
    disable_version_flag = true,
    about = "Brain CLI: central terminal dispatch for registered workspaces and tasks.",
    long_about = "Brain CLI: the central terminal dispatch for the user's second\n\
                  brain and task system. Bare `brain` opens a persistent shell\n\
                  with two main views (tasks: management, agenda, triage, the\n\
                  startup default; and a fuzzy search over the selected workspace), plus an\n\
                  app-level brain panel running an interactive agent session.\n\
                  \n\
                  Subcommands:\n\
                  \n\
                    tasks     Open the tasks view, or run a tasks utility\n\
                              (`brain tasks today --no-tui`,\n\
                              `brain tasks complete t123`,\n\
                              `brain tasks doctor`,\n\
                              `brain tasks search lamaze`).\n\
                  \n\
                    config    Read or change persistent config\n\
                              (`brain config`, `brain config get <name>`,\n\
                              `brain config set <name>=<value>`).\n\
                  \n\
                  Inside the shell: Ctrl-L/Ctrl-H cycle views, Ctrl-T/Ctrl-B\n\
                  jump to the tasks / brain-search view, Ctrl-P opens the\n\
                  command palette, and Alt-? shows help."
)]
pub struct Cli {
    /// Print the brain version.
    #[arg(short = 'v', long = "version", action = clap::ArgAction::SetTrue)]
    pub print_version: bool,

    /// Mirror the run log to stdout (the log file is always collected).
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Use Codex instead of Claude for the brain panel. Alias: -cx.
    #[arg(long, global = true)]
    pub codex: bool,

    /// Start the TUI-owned receiver server alongside the brain shell.
    #[arg(long, global = true)]
    pub with_receiver: bool,

    /// Never open the daily-triage startup nudge for this run.
    #[arg(long, global = true)]
    pub no_daily_triage_check: bool,

    /// Select a workspace by canonical name or alias.
    #[arg(short = 'b', long = "brain", global = true, value_name = "WORKSPACE")]
    pub brain: Option<String>,

    #[command(subcommand)]
    pub command: Option<Cmd>,
}

impl Cli {
    /// Selected brain-panel agent frontend.
    #[must_use]
    pub const fn agent_kind(&self) -> crate::session::AgentKind {
        if self.codex {
            crate::session::AgentKind::Codex
        } else {
            crate::session::AgentKind::Claude
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::AgentKind;
    use clap::CommandFactory;

    #[test]
    fn help_describes_the_selected_workspace_instead_of_one_fixed_root() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("selected workspace"), "{help}");
        assert!(!help.contains("over ~/brain"), "{help}");
    }

    #[test]
    fn help_marks_the_workspace_root_as_registry_owned_and_read_only() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("registry-owned, read-only `root`"), "{help}");
    }

    #[test]
    fn codex_flag_selects_codex_frontend() {
        let cli = Cli::try_parse_from(["brain", "--codex"]).expect("parse");
        assert!(cli.codex);
        assert_eq!(cli.agent_kind(), AgentKind::Codex);
    }

    #[test]
    fn cx_alias_selects_codex_frontend() {
        let cli = Cli::try_parse_from(normalize_codex_aliases(["brain", "-cx"])).expect("parse");
        assert!(cli.codex);
        assert_eq!(cli.agent_kind(), AgentKind::Codex);
    }

    #[test]
    fn claude_is_the_default_frontend() {
        let cli = Cli::try_parse_from(["brain"]).expect("parse");
        assert_eq!(cli.agent_kind(), AgentKind::Claude);
    }

    #[test]
    fn with_receiver_is_opt_in() {
        assert!(!Cli::try_parse_from(["brain"]).expect("parse").with_receiver);
        assert!(
            Cli::try_parse_from(["brain", "--with-receiver"])
                .expect("parse")
                .with_receiver
        );
    }

    #[test]
    fn no_daily_triage_check_is_opt_in() {
        assert!(
            !Cli::try_parse_from(["brain"])
                .expect("parse")
                .no_daily_triage_check
        );
        assert!(
            Cli::try_parse_from(["brain", "--no-daily-triage-check"])
                .expect("parse")
                .no_daily_triage_check
        );
    }
}

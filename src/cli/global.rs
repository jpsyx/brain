//! Global flags and parser normalization.

use clap::Parser;

use super::Cmd;

pub(super) fn normalize_global_args<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args = args.into_iter().map(Into::into).collect();
    extract_workspace_selectors(extract_agent_selectors(args))
}

fn extract_agent_selectors(args: Vec<String>) -> Vec<String> {
    let Some((program, tail)) = args.split_first() else {
        return args;
    };
    let mut selectors = Vec::new();
    let mut delegated = Vec::new();
    let mut terminated = false;
    for argument in tail {
        if terminated {
            delegated.push(argument.clone());
            continue;
        }
        match argument.as_str() {
            "--" => {
                terminated = true;
                delegated.push(argument.clone());
            }
            "--codex" | "-cx" => push_unique(&mut selectors, "--codex"),
            "--open-code" | "-oc" => push_unique(&mut selectors, "--open-code"),
            _ => delegated.push(argument.clone()),
        }
    }

    let mut normalized = Vec::with_capacity(args.len());
    normalized.push(program.clone());
    normalized.extend(selectors);
    normalized.extend(delegated);
    normalized
}

fn push_unique(selectors: &mut Vec<String>, selector: &str) {
    if !selectors.iter().any(|existing| existing == selector) {
        selectors.push(selector.to_owned());
    }
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
                  with three main views (tasks: management, agenda, triage, the\n\
                  startup default; search: fuzzy search over the selected workspace;\n\
                  and logs: scrollable diagnostics), plus an\n\
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
                  command palette, and Alt-S shows help."
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

    /// Use OpenCode for the brain panel. Alias: -oc.
    #[arg(long = "open-code", global = true)]
    pub open_code: bool,

    /// Persistently enable receiver ingress before the selected TUI registers.
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
    ///
    /// # Errors
    ///
    /// Returns [`AgentSelectionError::ConflictingFrontends`] when both
    /// non-default frontend flags are present.
    pub const fn selected_agent(&self) -> Result<crate::session::AgentKind, AgentSelectionError> {
        match (self.codex, self.open_code) {
            (true, true) => Err(AgentSelectionError::ConflictingFrontends),
            (true, false) => Ok(crate::session::AgentKind::Codex),
            (false, true) => Ok(crate::session::AgentKind::OpenCode),
            (false, false) => Ok(crate::session::AgentKind::Claude),
        }
    }
}

/// Invalid brain-panel frontend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSelectionError {
    /// More than one mutually exclusive non-default frontend was selected.
    ConflictingFrontends,
}

impl std::fmt::Display for AgentSelectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConflictingFrontends => {
                formatter.write_str("Choose one agent frontend: --codex or --open-code.")
            }
        }
    }
}

impl std::error::Error for AgentSelectionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::try_parse_from;
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
        assert_eq!(cli.selected_agent(), Ok(AgentKind::Codex));
    }

    #[test]
    fn cx_alias_selects_codex_frontend() {
        let cli = try_parse_from(["brain", "-cx"]).expect("parse");
        assert!(cli.codex);
        assert_eq!(cli.selected_agent(), Ok(AgentKind::Codex));
    }

    #[test]
    fn claude_is_the_default_frontend() {
        let cli = Cli::try_parse_from(["brain"]).expect("parse");
        assert_eq!(cli.selected_agent(), Ok(AgentKind::Claude));
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

    #[test]
    fn frontend_selectors_are_recognized_in_every_supported_task_position() {
        for (arguments, expected) in [
            (vec!["brain", "--codex"], AgentKind::Codex),
            (vec!["brain", "-cx"], AgentKind::Codex),
            (vec!["brain", "--open-code"], AgentKind::OpenCode),
            (vec!["brain", "-oc"], AgentKind::OpenCode),
            (vec!["brain", "tasks", "--open-code"], AgentKind::OpenCode),
            (
                vec!["brain", "tasks", "--open-code", "today"],
                AgentKind::OpenCode,
            ),
            (
                vec!["brain", "tasks", "today", "--open-code"],
                AgentKind::OpenCode,
            ),
            (
                vec!["brain", "tasks", "today", "-oc", "--no-tui"],
                AgentKind::OpenCode,
            ),
        ] {
            let cli = try_parse_from(arguments.clone()).expect("selector parse");
            assert_eq!(cli.selected_agent(), Ok(expected), "{arguments:?}");
            if let Some(crate::cli::Cmd::Tasks(tasks)) = cli.command {
                assert!(
                    tasks.rest.iter().all(|argument| !matches!(
                        argument.as_str(),
                        "--codex" | "-cx" | "--open-code" | "-oc"
                    )),
                    "selector leaked into delegated tasks arguments: {arguments:?}"
                );
            }
        }
    }

    #[test]
    fn every_cross_frontend_selector_combination_is_a_typed_conflict() {
        for arguments in [
            vec!["brain", "--codex", "--open-code"],
            vec!["brain", "-cx", "-oc"],
            vec!["brain", "--codex", "tasks", "today", "-oc"],
            vec!["brain", "tasks", "--open-code", "today", "--codex"],
            vec!["brain", "tasks", "today", "-cx", "--open-code"],
            vec!["brain", "--codex", "-cx", "tasks", "today", "-oc"],
        ] {
            let cli = try_parse_from(arguments.clone()).expect("selectors parse before validation");
            assert_eq!(
                cli.selected_agent(),
                Err(AgentSelectionError::ConflictingFrontends),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn duplicate_same_frontend_selectors_are_idempotent() {
        for (arguments, expected) in [
            (vec!["brain", "--codex", "-cx"], AgentKind::Codex),
            (
                vec!["brain", "tasks", "--open-code", "today", "-oc"],
                AgentKind::OpenCode,
            ),
        ] {
            let cli = try_parse_from(arguments.clone()).expect("duplicate selector");
            assert_eq!(cli.selected_agent(), Ok(expected), "{arguments:?}");
        }
    }

    #[test]
    fn option_terminator_preserves_selector_looking_task_values() {
        let cli = try_parse_from(["brain", "tasks", "search", "--", "--open-code", "-cx"])
            .expect("terminated task values");

        assert_eq!(cli.selected_agent(), Ok(AgentKind::Claude));
        let Some(crate::cli::Cmd::Tasks(tasks)) = cli.command else {
            panic!("tasks command");
        };
        assert_eq!(tasks.rest, ["search", "--", "--open-code", "-cx"]);
    }
}

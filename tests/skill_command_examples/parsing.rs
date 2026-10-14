use clap::Parser as _;

pub const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Templates in command references have concrete counterparts in this table.
pub const EXAMPLES: &[&[&str]] = &[
    &["brain", "tasks", "remove", "T17", "--dry-run", "--json"],
    &["brain", "tasks", "remove", "T17", "--yes", "--json"],
    &[
        "brain",
        "tasks",
        "remove",
        "T17",
        "T18",
        "--dry-run",
        "--json",
    ],
    &[
        "brain",
        "tasks",
        "remove",
        "T17",
        "T18",
        "--dependents",
        "rewire",
        "--yes",
    ],
    &[
        "brain",
        "tasks",
        "remove",
        "T17",
        "T18",
        "--dependents",
        "rewire",
        "--yes",
        "--json",
    ],
    &["brain", "tasks", "remove", "H42", "--habit", "--yes"],
    &[
        "brain",
        "tasks",
        "remove",
        "T17",
        "--linked-issue",
        "keep",
        "--yes",
    ],
    &[
        "brain",
        "-w",
        "sample",
        "tasks",
        "remove",
        "T17",
        "--yes",
        "--expect-plan",
        HASH,
        "--json",
    ],
    &[
        "brain",
        "-w",
        "sample",
        "tasks",
        "remove",
        "--operation-status",
        "operation-1",
        "--json",
    ],
    &[
        "brain",
        "-w",
        "sample",
        "tasks",
        "remove",
        "--resume",
        "operation-1",
        "--json",
    ],
    &[
        "brain",
        "-w",
        "sample",
        "tasks",
        "remove",
        "--abandon",
        "operation-1",
        "--yes",
        "--json",
    ],
    &[
        "brain",
        "tasks",
        "remove",
        "T17",
        "--dependents",
        "unlink",
        "--yes",
        "-w",
        "sample",
    ],
    &[
        "brain",
        "tasks",
        "rm",
        "T17",
        "--dry-run",
        "--json",
        "-w",
        "sample",
    ],
    &["brain", "-w", "sample", "tasks", "drop", "T17", "--yes"],
];

pub fn parse(argv: &[&str]) -> Result<brain::tasks::cli::Cli, clap::Error> {
    let outer = brain::cli::try_parse_from(argv.iter().copied())?;
    assert!(outer.selected_agent().is_ok());
    if argv.contains(&"sample") {
        assert_eq!(outer.workspace_selector.as_deref(), Some("sample"));
    }
    let Some(brain::cli::Cmd::Tasks(tasks)) = outer.command else {
        panic!("expected tasks: {argv:?}")
    };
    let delegated = std::iter::once("tasks".to_owned())
        .chain(tasks.rest)
        .collect();
    brain::tasks::cli::Cli::try_parse_from(brain::command::tasks::rewrite_mark_grammar(delegated))
}

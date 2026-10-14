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
        "--linked-issue",
        "keep",
        "--yes",
        "--json",
    ],
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

/// Validate complete commands in rendered or source Markdown with the same argv table.
pub fn removal_examples(markdown: &str) -> (std::collections::BTreeSet<String>, usize) {
    let mut examples = std::collections::BTreeSet::new();
    let mut templates = 0;
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let chunks = if fenced || line.starts_with("    ") || line.starts_with('\t') {
            vec![line.trim()]
        } else {
            code_spans(line)
        };
        for chunk in chunks {
            let words: Vec<_> = chunk.split_whitespace().collect();
            if words.first() != Some(&"brain")
                || !words.windows(2).any(|pair| pair == ["tasks", "remove"])
                || words.len() < 4
                || words
                    .iter()
                    .any(|word| word.contains('<') || word.contains('…'))
            {
                continue;
            }
            let is_template = words.iter().any(|word| word.contains('$'));
            let concrete: Vec<_> = words
                .iter()
                .map(|word| match *word {
                    "\"$fingerprint\"" => HASH,
                    "\"$operation_id\"" => "operation-1",
                    other => {
                        assert!(!other.contains('$'), "undocumented template: {chunk}");
                        other
                    }
                })
                .collect();
            parse(&concrete).unwrap_or_else(|error| panic!("{chunk}: {error}"));
            assert!(
                EXAMPLES.contains(&concrete.as_slice()),
                "example needs a matching concrete argv table row: {chunk}"
            );
            templates += usize::from(is_template);
            examples.insert(concrete.join(" "));
        }
    }
    (examples, templates)
}

/// The contents of every `` `…` `` span on one line.
pub fn code_spans(line: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('`') else {
            break;
        };
        spans.push(&rest[..close]);
        rest = &rest[close + 1..];
    }
    spans
}

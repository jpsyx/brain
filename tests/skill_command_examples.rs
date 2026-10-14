//! Concrete skill examples traverse the same two parsers as the binary.

#[path = "skill_command_examples/parsing.rs"]
mod parsing;
#[path = "skill_command_examples/workspace.rs"]
mod workspace;

use brain::tasks::cli::{Command, Dependents, LinkedIssue};

#[test]
fn bundled_removal_guidance_uses_the_native_preview_and_consent_contract() {
    let todo = include_str!("../skills/todo/SKILL.md");
    for required in [
        "### Remove tasks",
        "brain tasks remove T17 --dry-run --json",
        "pass its `fingerprint` as `--expect-plan`",
        "A stale plan\nrequires a fresh preview and consent.",
        "Do not edit CSV rows or project arrays\nyourself.",
        "`reject` is the default dependency policy.",
        "These options\ndo not delete dependent tasks.",
        "Managed\ntriage habits remain protected.",
        "Preserve completed habit history unless it\nwas explicitly selected.",
        "Never substitute an agent-tool cancellation",
        "committed deletion with an agenda warning is still a\ncommitted deletion.",
    ] {
        assert!(
            todo.contains(required),
            "missing removal guidance: {required}"
        );
    }
    for obsolete in [
        "status=dropped",
        "Delete the original task row.",
        "run sync).",
    ] {
        for source in [
            todo,
            include_str!("../skills/todo/references/commands.md"),
            include_str!("../skills/todo/references/task-project-link.md"),
        ] {
            assert!(
                !source.contains(obsolete),
                "obsolete removal advice: {obsolete}"
            );
        }
    }
    let conversion = include_str!("../skills/todo/references/task-project-link.md");
    assert!(conversion.contains("report incomplete source removal"));
    let triage = include_str!("../skills/triage/SKILL.md");
    assert!(triage.contains("all consented IDs in one command"));
    assert!(!triage.contains("remove from CSV (confirms first)"));
}

#[test]
fn documented_removal_options_parse_through_both_real_parsers() {
    for example in parsing::EXAMPLES {
        let parsed = parsing::parse(example).unwrap_or_else(|error| panic!("{example:?}: {error}"));
        assert!(matches!(parsed.command, Some(Command::Remove(_))));
    }
    let parsed = parsing::parse(&[
        "brain",
        "tasks",
        "remove",
        "T17",
        "T18",
        "--dependents",
        "rewire",
        "--linked-issue",
        "keep",
        "--yes",
        "-w",
        "sample",
    ])
    .unwrap();
    let Some(Command::Remove(args)) = parsed.command else {
        panic!("remove")
    };
    assert_eq!(args.ids, ["T17", "T18"]);
    assert_eq!(args.dependents, Some(Dependents::Rewire));
    assert_eq!(args.linked_issue, Some(LinkedIssue::Keep));
    assert!(args.yes);
}

#[test]
fn removal_illegal_argv_is_rejected_by_the_real_delegated_parser() {
    for arguments in [
        vec!["remove"],
        vec!["remove", "T17", "--dependents", "cascade"],
        vec!["remove", "T17", "--linked-issue", "delete"],
        vec!["remove", "T17", "--resume", "operation-1"],
        vec![
            "remove",
            "--resume",
            "operation-1",
            "--abandon",
            "operation-1",
        ],
        vec!["remove", "T17", "--dry-run", "--expect-plan", parsing::HASH],
        vec!["remove", "T17", "--yes", "--unknown-option"],
    ] {
        let mut example = vec!["brain", "-w", "sample", "tasks"];
        example.extend(arguments);
        assert!(parsing::parse(&example).is_err(), "accepted {example:?}");
    }
}

#[test]
fn removal_mode_examples_use_the_binarys_private_typed_validation() {
    let fixture = workspace::Fixture::new();
    let before = fixture.canonical_bytes();
    for (flags, expected) in [
        (
            vec!["--resume", "operation-1", "--yes"],
            "invalid_removal_mode",
        ),
        (
            vec!["--operation-status", "operation-1", "--habit"],
            "invalid_removal_mode",
        ),
        (
            vec!["--resume", "operation-1", "--dry-run"],
            "invalid_removal_mode",
        ),
        (
            vec![
                "--operation-status",
                "operation-1",
                "--dependents",
                "unlink",
            ],
            "invalid_removal_mode",
        ),
        (
            vec!["--resume", "operation-1", "--linked-issue", "keep"],
            "invalid_removal_mode",
        ),
        (vec!["--abandon", "operation-1"], "invalid_removal_mode"),
        (
            vec![
                "--abandon",
                "operation-1",
                "--yes",
                "--expect-plan",
                parsing::HASH,
            ],
            "invalid_removal_mode",
        ),
        (
            vec!["T17", "--yes", "--status", "done"],
            "removal_filters_not_supported",
        ),
    ] {
        let mut example = vec!["brain", "-w", "sample", "tasks", "remove", "--json"];
        example.extend(flags);
        parsing::parse(&example).expect("clap accepts before typed mode validation");
        let value = fixture.json(&example, 2);
        assert_eq!(value["kind"], "error", "{example:?}: {value}");
        assert_eq!(value["code"], expected, "{example:?}: {value}");
        assert_eq!(fixture.canonical_bytes(), before);
    }
    let alternate_csv = [
        "brain",
        "-w",
        "sample",
        "tasks",
        "--csv",
        "other.csv",
        "remove",
        "T17",
        "--yes",
        "--json",
    ];
    parsing::parse(&alternate_csv).unwrap();
    assert_eq!(
        fixture.json(&alternate_csv, 2)["code"],
        "removal_filters_not_supported"
    );
    assert_eq!(fixture.canonical_bytes(), before);
    for mode in ["--operation-status", "--resume", "--abandon"] {
        let mut example = vec![
            "brain",
            "tasks",
            "remove",
            mode,
            "operation-1",
            "--json",
            "-w",
            "sample",
        ];
        if mode == "--abandon" {
            example.push("--yes");
        }
        parsing::parse(&example).unwrap();
        assert_eq!(fixture.json(&example, 1)["code"], "operation_unavailable");
        assert_eq!(fixture.canonical_bytes(), before);
    }
    let example = [
        "brain",
        "-w",
        "sample",
        "tasks",
        "remove",
        "T17",
        "--dry-run",
        "--json",
    ];
    parsing::parse(&example).unwrap();
    assert_eq!(fixture.json(&example, 0)["kind"], "preview");
    assert_eq!(fixture.canonical_bytes(), before);
}

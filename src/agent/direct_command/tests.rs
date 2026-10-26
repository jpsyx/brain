use super::is_direct_invocation;

const CLAUDE_FLAGS: [&str; 7] = [
    "--",
    "--mcp-config",
    "--strict-mcp-config",
    "--append-system-prompt",
    "--session-id",
    "--resume",
    "--bare",
];

const PI_FLAGS: [&str; 6] = [
    "--",
    "--session-id",
    "--append-system-prompt",
    "--skill",
    "--no-skills",
    "--extension",
];

#[test]
fn a_plain_invocation_of_the_named_executable_is_direct() {
    assert!(is_direct_invocation("claude", "claude", &CLAUDE_FLAGS));
    assert!(is_direct_invocation(
        "/opt/homebrew/bin/claude --dangerously-skip-permissions",
        "claude",
        &CLAUDE_FLAGS
    ));
    assert!(is_direct_invocation("pi", "pi", &PI_FLAGS));
    assert!(is_direct_invocation(
        "pi --model 'anthropic/claude-opus-5'",
        "pi",
        &PI_FLAGS
    ));
}

#[test]
fn each_frontend_owns_its_own_flag_set() {
    // `--skill` is Brain's to supply for pi, but means nothing for Claude.
    assert!(!is_direct_invocation("pi --skill /skills", "pi", &PI_FLAGS));
    assert!(is_direct_invocation(
        "claude --skill /skills",
        "claude",
        &CLAUDE_FLAGS
    ));
}

#[test]
fn a_command_that_already_passes_an_owned_flag_is_not_direct() {
    for command in [
        "pi --session-id abc",
        "pi --session-id=abc",
        "pi --no-skills",
        "pi -- hello",
    ] {
        assert!(
            !is_direct_invocation(command, "pi", &PI_FLAGS),
            "{command} passes a flag Brain owns"
        );
    }
}

#[test]
fn a_different_executable_is_never_direct() {
    assert!(!is_direct_invocation("pi-wrapper", "pi", &PI_FLAGS));
    assert!(!is_direct_invocation("npx pi", "pi", &PI_FLAGS));
}

#[test]
fn anything_a_shell_would_expand_or_chain_is_not_direct() {
    for command in [
        "sh -c 'exec pi'",
        "pi && echo done",
        "pi $EXTRA",
        "pi `hostname`",
        "pi > /tmp/out",
        "pi 'unterminated",
    ] {
        assert!(
            !is_direct_invocation(command, "pi", &PI_FLAGS),
            "{command} is not a plain word list"
        );
    }
}

#[test]
fn an_empty_command_is_not_direct() {
    assert!(!is_direct_invocation("   ", "pi", &PI_FLAGS));
}

#[test]
fn double_quoted_backslashes_follow_shell_literal_rules() {
    let command = r#"claude "ordinary\q literal\n slash\\ quote\" dollar\$ tick\` Unicodeé\🙂" outside\q 'single\q'"#;
    assert_eq!(
        super::parse(command).unwrap(),
        [
            "claude",
            r#"ordinary\q literal\n slash\ quote" dollar$ tick` Unicodeé\🙂"#,
            "outsideq",
            r"single\q",
        ]
    );
    for command in [
        r#"claude "expand$HOME""#,
        r#"claude "substitute`hostname`""#,
        r#"claude "literal\q" && echo unsafe"#,
        "claude \\\ncontinued",
    ] {
        assert!(super::parse(command).is_none(), "{command}");
    }
}

#[test]
fn unquoted_tilde_expansion_is_not_a_literal_instruction() {
    assert!(super::parse("claude --append-system-prompt ~").is_none());
    assert_eq!(
        super::parse("claude --append-system-prompt '~'").unwrap(),
        ["claude", "--append-system-prompt", "~"]
    );
}

#[test]
fn dynamic_shell_classes_are_refused_and_quoted_literals_remain_literal() {
    for dynamic in [
        "$HOME",
        "${HOME}",
        "$(hostname)",
        "`hostname`",
        "$((1+1))",
        "*.md",
        "file?.md",
        "[ab].md",
        "{a,b}.md",
        "~",
        "~/rules",
        "~other/rules",
        "foo\\\nbar",
        "\"foo\\\nbar\"",
    ] {
        assert!(
            super::parse(&format!("claude {dynamic}")).is_none(),
            "{dynamic}"
        );
    }
    assert_eq!(
        super::parse(
            r#"claude '$HOME ${HOME} $(hostname) `hostname` *.md {a,b} ~' "*.md {a,b} ~""#
        )
        .unwrap(),
        [
            "claude",
            "$HOME ${HOME} $(hostname) `hostname` *.md {a,b} ~",
            "*.md {a,b} ~"
        ]
    );
}

use std::path::{Path, PathBuf};

use super::collect_sources;
use super::task_three_assertions::{
    private_whole_value_assertion_violation_lines, private_whole_value_assertion_violations,
};

#[test]
fn cumulative_task_three_tests_never_print_private_whole_values_on_failure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = discover_task_three_privacy_tests(root);
    assert!(
        sources.len() >= 12,
        "Task 3 private assertion discovery unexpectedly narrowed"
    );
    let failing_cases = task_three_test_violations(root);
    let total_violations = failing_cases
        .iter()
        .map(|(_, _, lines)| lines.len())
        .sum::<usize>();
    let failing_summary = failing_cases
        .iter()
        .map(|(case_index, path, lines)| {
            let lines = lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",");
            format!("{case_index}:{path}:{lines}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        total_violations == 0,
        "receiver delivery tests contain {total_violations} private whole-value diagnostics at {failing_summary}"
    );
}

pub(super) fn task_three_test_violations(root: &Path) -> Vec<(usize, String, Vec<usize>)> {
    discover_task_three_privacy_tests(root)
        .into_iter()
        .enumerate()
        .filter_map(|(case_index, path)| {
            let source = std::fs::read_to_string(&path).expect("receiver delivery privacy source");
            let violation_lines = private_whole_value_assertion_violation_lines(&source);
            (!violation_lines.is_empty()).then(|| {
                (
                    case_index,
                    path.strip_prefix(root)
                        .expect("receiver delivery test below repository root")
                        .to_string_lossy()
                        .into_owned(),
                    violation_lines,
                )
            })
        })
        .collect()
}

#[test]
fn task_three_private_assertion_policy_rejects_raw_values_but_allows_safe_proofs() {
    for (case_index, mutation) in [
        "assert_eq!(conversation.transcript_markdown(), expected);",
        "assert_eq!(persisted.inbound(), expected);",
        "assert_eq!(email.sender(), expected);",
        "assert_eq!(email.recipients(), expected);",
        "assert_eq!(recipients, expected);",
        "assert_eq!(assistant_answer, expected);",
        "assert_eq!(decoded_envelope, expected);",
        "assert_eq!(payload, expected);",
        "assert_eq!(completion_evidence_json, expected);",
        "assert_eq!((sender, envelope.len()), expected);",
        "assert_eq!((payload.len(), receiving_address), expected);",
        "let alias = sender; assert_eq!(alias, expected);",
        "let fixture = sender; debug_assert_eq!(fixture, expected);",
        "let alias = prompt; assert_ne!(alias, expected);",
        "assert_eq!(receiving_address, expected);",
        "assert_eq!(prompt, expected);",
        "assert!(condition, \"private sender: {sender}\");",
        concat!(
            "let alias = response_sender; assert!(condition, \"private: {",
            "alias:?}\");"
        ),
        "assert!(condition, \"private prompt: {}\", prompt);",
        concat!("assert_eq!(format!(\"{", "sender:?}\"), expected);"),
        "let mut alias = String::new(); alias = sender; assert_eq!(alias, expected);",
        "debug_assert_eq!(sender, expected);",
        "debug_assert_ne!(prompt, expected);",
        concat!(
            "debug_assert!(condition, \"private sender: {",
            "sender:?}\");"
        ),
        "let (alias, safe) = (sender, 1); debug_assert_eq!(alias, expected);",
        concat!(
            "let Envelope { sender: alias, safe } = envelope; ",
            "assert_ne!(alias, expected);"
        ),
        "let has_private_sender = sender; assert_eq!(has_private_sender, expected);",
        "let is_private_prompt = prompt; assert!(condition, \"private: {is_private_prompt}\");",
        "debug_assert_eq!(envelope.has_private_sender(), expected);",
        "assert!(condition, \"private: {alias}\", alias = sender);",
        concat!(
            "let alias; if condition { alias = sender; } ",
            "assert_eq!(alias, expected);"
        ),
        "match sender { alias => assert_eq!(alias, expected) };",
        "for alias in sender { assert_ne!(alias, expected); }",
        concat!(
            "let (first, Envelope { sender: second, .. }) = (sender, envelope); ",
            "assert_eq!(second, expected);"
        ),
        "fixture.value = sender; assert_eq!(fixture.value, expected);",
        "fixture[0] = prompt; assert_eq!(fixture[0], expected);",
        "assert_eq!(sender.is_private_value(), expected);",
        "assert_eq!(prompt.has_private_value(), expected);",
        "dbg!(sender);",
        concat!("format_args!(\"private: {", "sender:?}\");"),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            private_whole_value_assertion_violations(mutation) > 0,
            "private whole-value assertion mutation was accepted at case index {case_index}"
        );
    }
    for (case_index, proof) in [
        "assert!(private_text_proof(transcript) == expected_proof);",
        "assert!(transcript.matches(heading).count() == 1);",
        "assert!(email.sender() == canonical_sender);",
        "assert!(answer.len() == expected_len);",
        "assert!(envelope_count == 1);",
        "assert!(sender.len() == 1 && payload.len() == 2);",
        "assert!(receiving_address == expected, \"fixed routing assertion\");",
        concat!(
            "let status = build_status(provider_reference); ",
            "assert!(status.state() == expected_state);"
        ),
        concat!(
            "*STORE.lock().expect(\"store\") = Some(sender); ",
            "let marker = load().expect(\"marker\"); assert_eq!(marker, 1);"
        ),
        "fn first() { let result = sender; assert!(result.len() == 1); } \
         fn second() { let result = 1; assert_eq!(result, 1); }",
        "panic!(\"fixed receiver delivery failure\");",
        "println!(\"fixed receiver delivery progress\");",
        "eprintln!(\"fixed receiver delivery diagnostic\");",
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            private_whole_value_assertion_violations(proof) == 0,
            "safe private-value proof was rejected at case index {case_index}"
        );
    }
}

#[test]
fn future_task_three_delivery_tests_are_discovered_for_private_assertion_audit() {
    let temporary = tempfile::tempdir().expect("temporary repository");
    for relative in [
        "src/server/delivery/tests.rs",
        "src/state/receiver/tests/delivery_future.rs",
        "src/state/receiver/tests/completion/future.rs",
        "src/state/receiver/tests/acceptance.rs",
        "src/state/receiver/tests/unrelated.rs",
        "src/server/delivery/tests/future.rs",
        "src/server/receiver/http/email/tests.rs",
        "src/server/receiver/http/sms/tests.rs",
        "src/server/receiver/http/sms.rs",
        "src/tui/app_brain/tests/receiver_durable_answer_commit.rs",
        "src/tui/app_brain/tests/receiver_durable_future.rs",
        "src/tui/state/services/receiver_delivery_future.rs",
        "src/server/provider/tests/future.rs",
        "src/tui/app_brain/tests/future_receiver.rs",
        "src/tui/app_brain/receiver/tests/future.rs",
        "src/tui/receiver/tests/provider_future.rs",
        "src/tui/state/services/receiver/tests/future.rs",
        "src/tui/app_brain/tests/future/receiver/delivery.rs",
        "src/tui/app_brain/tests/future/provider/delivery.rs",
        "src/tui/app_brain/tests/future/app/delivery.rs",
    ] {
        let path = temporary.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
        std::fs::write(path, "#[test] fn fixture() {}\n").expect("fixture source");
    }

    let discovered = discover_task_three_privacy_tests(temporary.path());

    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("state/receiver/tests/delivery_future.rs")),
        "future receiver delivery test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("server/delivery/tests.rs")),
        "provider delivery root test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("server/delivery/tests/future.rs")),
        "future provider delivery test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("state/receiver/tests/acceptance.rs")),
        "Task 3 acceptance test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("state/receiver/tests/completion/future.rs")),
        "future completion state test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("server/receiver/http/email/tests.rs")),
        "authenticated email HTTP test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("server/receiver/http/sms.rs")),
        "authenticated inline SMS HTTP test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("receiver_durable_answer_commit.rs")),
        "Task 3 composed answer test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("receiver_durable_future.rs")),
        "future composed App test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("services/receiver_delivery_future.rs")),
        "future App delivery service test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("server/provider/tests/future.rs")),
        "future provider test path was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("app_brain/tests/future_receiver.rs")),
        "future receiver App test path was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("app_brain/receiver/tests/future.rs")),
        "future App receiver coordinator test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("tui/receiver/tests/provider_future.rs")),
        "future receiver provider test was not audited"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("services/receiver/tests/future.rs")),
        "future App receiver service test was not audited"
    );
    for suffix in [
        "app_brain/tests/future/receiver/delivery.rs",
        "app_brain/tests/future/provider/delivery.rs",
        "app_brain/tests/future/app/delivery.rs",
    ] {
        assert!(
            discovered.iter().any(|path| path.ends_with(suffix)),
            "future nested receiver/provider/App test was not audited"
        );
    }
    assert!(
        discovered
            .iter()
            .all(|path| !path.ends_with("state/receiver/tests/unrelated.rs")),
        "unrelated receiver test entered the Task 3 privacy audit"
    );
}

pub(super) fn discover_task_three_privacy_tests(root: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let source_root = root.join("src");
    if source_root.is_dir() {
        collect_sources(&source_root, &mut candidates);
    }
    candidates.retain(|path| {
        let relative = path.strip_prefix(root).expect("repository test source");
        let filename = relative
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let relative_text = relative.to_string_lossy();
        relative == Path::new("src/server/delivery.rs")
            || relative.starts_with("src/server/delivery")
            || relative.starts_with("src/server/provider")
            || relative.starts_with("src/server/receiver/http")
            || relative == Path::new("src/state/receiver/tests/acceptance.rs")
            || relative == Path::new("src/state/receiver/tests/binding.rs")
            || relative == Path::new("src/state/receiver/tests/support.rs")
            || relative == Path::new("src/state/receiver/tests/completion_answer.rs")
            || relative.starts_with("src/state/receiver/tests/completion")
            || relative.starts_with("src/state/receiver/tests/schema_sections")
                && filename.starts_with("delivery")
            || relative.starts_with("src/state/receiver/tests") && filename.starts_with("delivery_")
            || relative.starts_with("src/tui/app_brain/tests")
                && (filename.contains("receiver")
                    || filename.contains("provider")
                    || filename.starts_with("receiver_durable_answer")
                    || filename.starts_with("receiver_durable_delivery")
                    || filename.starts_with("receiver_durable_future"))
            || relative.starts_with("src/tui/app_brain/receiver")
            || relative.starts_with("src/tui/receiver")
            || relative_text.starts_with("src/tui/state/services/receiver")
            || nested_receiver_privacy_test(relative)
    });
    candidates.sort();
    candidates.dedup();
    candidates
}

fn nested_receiver_privacy_test(relative: &Path) -> bool {
    let components = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    let Some(tests) = components
        .iter()
        .position(|component| *component == "tests")
    else {
        return false;
    };
    components[tests + 1..].iter().any(|component| {
        matches!(*component, "receiver" | "provider" | "app")
            || component.starts_with("receiver_")
            || component.starts_with("provider_")
            || component.starts_with("app_")
    })
}

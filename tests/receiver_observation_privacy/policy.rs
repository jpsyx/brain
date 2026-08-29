use std::path::{Path, PathBuf};

#[path = "policy/debug_impl.rs"]
mod debug_impl;
#[path = "policy/debug_tests.rs"]
mod debug_tests;
#[path = "policy/diagnostics.rs"]
mod diagnostics;
#[path = "policy/literals.rs"]
mod literals;
#[path = "policy/task_three_adversarial.rs"]
mod task_three_adversarial;
#[path = "policy/task_three_assertions/mod.rs"]
mod task_three_assertions;

use diagnostics::privacy_diagnostic_violations;
use literals::source_privacy_violations;
use task_three_assertions::{
    private_whole_value_assertion_violation_lines, private_whole_value_assertion_violations,
};

#[test]
fn privacy_failure_messages_cannot_interpolate_private_surfaces() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut privacy_tests = Vec::new();
    collect_sources(
        &root.join("tests/receiver_observation_privacy"),
        &mut privacy_tests,
    );
    privacy_tests.push(root.join("tests/receiver_observation_privacy.rs"));
    privacy_tests.push(root.join("tests/workspace_capabilities/frontend_redaction.rs"));
    for (case_index, path) in privacy_tests.into_iter().enumerate() {
        let source = std::fs::read_to_string(&path).expect("privacy harness source");
        assert!(
            privacy_diagnostic_violations(&source).is_empty(),
            "privacy test contains unsafe diagnostics at case index {case_index}"
        );
    }
}

#[test]
fn cumulative_task_three_tests_never_print_private_whole_values_on_failure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = discover_task_three_privacy_tests(root);
    assert!(
        sources.len() >= 12,
        "Task 3 private assertion discovery unexpectedly narrowed"
    );
    let mut failing_cases = Vec::new();
    let mut total_violations = 0;
    for (case_index, path) in sources.into_iter().enumerate() {
        let source = std::fs::read_to_string(&path).expect("receiver delivery privacy source");
        let violation_lines = private_whole_value_assertion_violation_lines(&source);
        if !violation_lines.is_empty() {
            total_violations += violation_lines.len();
            failing_cases.push((
                case_index,
                path.strip_prefix(root)
                    .expect("receiver delivery test below repository root")
                    .to_string_lossy()
                    .into_owned(),
                violation_lines,
            ));
        }
    }
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
        "assert_eq!(transcript.matches(heading).count(), 1);",
        "assert!(email.sender() == canonical_sender);",
        "assert!(answer.len() == expected_len);",
        "assert!(envelope_count == 1);",
        "assert_eq!((sender.len(), payload.len()), (1, 2));",
        "assert!(receiving_address == expected, \"fixed routing assertion\");",
        concat!(
            "let status = build_status(provider_reference); ",
            "assert_eq!(status.state(), expected_state);"
        ),
        concat!(
            "*STORE.lock().expect(\"store\") = Some(sender); ",
            "let marker = load().expect(\"marker\"); assert_eq!(marker, 1);"
        ),
        "fn first() { let result = sender; assert!(result.len() == 1); } \
         fn second() { let result = 1; assert_eq!(result, 1); }",
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

fn discover_task_three_privacy_tests(root: &Path) -> Vec<PathBuf> {
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

#[test]
fn diagnostic_policy_rejects_private_echo_forms() {
    let mutations = [
        mutation(&[
            "assert_eq!(format!(\"",
            "{",
            "request",
            ":?",
            "}",
            "\"), expected);",
        ]),
        mutation(&["assert_eq!(child[\"session_id\"], expected);"]),
        mutation(&[
            "panic!(\"producer failed: ",
            "{",
            "output",
            ":?",
            "}",
            "\");",
        ]),
        mutation(&["panic!(\"stdout: {}\", String::from_utf8_lossy(&output.stdout));"]),
        mutation(&["panic!(\"stderr: {}\", String::from_utf8_lossy(&output.stderr));"]),
        interpolation_mutation("canary"),
        interpolation_mutation("token"),
        interpolation_mutation("secret"),
        interpolation_mutation("literal"),
        interpolation_mutation("rendered"),
        mutation(&["assert_eq!(rendered, expected); assert_private_absent(\"shape\", &rendered);"]),
    ];

    for (case_index, mutation) in mutations.iter().enumerate() {
        assert!(
            !privacy_diagnostic_violations(mutation).is_empty(),
            "privacy diagnostic mutation was accepted at case index {case_index}"
        );
    }
}

fn interpolation_mutation(identifier: &str) -> String {
    mutation(&[
        "assert!(!value.contains(",
        identifier,
        "), \"leaked ",
        "{",
        identifier,
        "}",
        "\");",
    ])
}

fn mutation(parts: &[&str]) -> String {
    parts.concat()
}

#[test]
fn every_semantically_relevant_observation_and_completion_source_is_audited() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let audited = discover_relevant_sources(root);
    for required in [
        "scripts/receiver_observation_bridge.py",
        "scripts/agent_session_stop_hook.py",
        "scripts/opencode_brain_plugin.js",
        "src/agent/observation.rs",
        "src/agent/observation/snapshot.rs",
        "src/agent/observation/snapshot/file.rs",
        "src/state/receiver/store/completion/authorization.rs",
        "src/state/receiver/store/completion/duplicate.rs",
        "src/state/receiver/store/completion/lifecycle.rs",
        "src/state/receiver/store/completion/transaction.rs",
        "src/state/receiver/store/observation.rs",
        "src/state/session_store.rs",
        "src/tui/state/services.rs",
        "src/tui/app_brain/receiver/active.rs",
        "src/tui/app_brain/receiver/artifact.rs",
        "src/tui/app_brain/receiver/diagnostic.rs",
        "src/tui/receiver/runtime.rs",
        "tests/fixtures/opencode/plugin_harness.js",
        "src/tui/app_brain/tests/receiver_durable_observation_composed.rs",
        "src/tui/app_brain/tests/receiver_durable_observation_replacement.rs",
        "src/tui/app_brain/tests/receiver_durable_producer_support.rs",
    ] {
        assert!(
            audited.contains(&PathBuf::from(required)),
            "missing audit surface {required}"
        );
    }
    assert!(
        audited.len() >= 50,
        "semantic privacy discovery unexpectedly narrowed"
    );

    for relative in audited {
        let violations = source_privacy_violations_for_path(root, &relative);
        assert!(
            violations.is_empty(),
            "{} contains private literals",
            relative.display()
        );
    }
}

#[test]
fn newly_discovered_observation_sources_reject_private_home_email_and_host_literals() {
    for (case, binding, literal) in [
        (
            "mac-home",
            "PRIVATE_LITERAL",
            r"/Users/private-owner/receiver-secret",
        ),
        (
            "unix-home",
            "PRIVATE_LITERAL",
            r"/home/private-owner/receiver-secret",
        ),
        (
            "windows-home",
            "PRIVATE_LITERAL",
            r"C:\\Users\\private-owner\\receiver-secret",
        ),
        (
            "generic-prefix-home",
            "PRIVATE_LITERAL",
            r"/Users/example-private-owner/receiver-secret",
        ),
        ("email", "PRIVATE_LITERAL", r"sender@private.corp"),
        ("host", "VALUE", r"receiver.private.lan"),
        ("host-port", "VALUE", r"receiver.private.lan:8443"),
        ("host-root-dot", "VALUE", r"receiver.private.lan."),
        ("host-root-dot-port", "VALUE", r"receiver.private.lan.:8443"),
        (
            "host-file-tld-rs-port",
            "VALUE",
            r"receiver.private.rs:8443",
        ),
        (
            "host-file-tld-sh-port",
            "VALUE",
            r"receiver.private.sh:8443",
        ),
        (
            "host-file-tld-md-port",
            "VALUE",
            r"receiver.private.md:8443",
        ),
        (
            "host-file-tld-py-port",
            "VALUE",
            r"receiver.private.py:8443",
        ),
        ("bare-private-ipv6", "VALUE", r"fd12:3456:789a::1"),
        (
            "scoped-private-ipv6-url",
            "VALUE",
            r"http://[fe80::1%25en0]:8080/callback",
        ),
    ] {
        let temporary = tempfile::tempdir().expect("temporary repository");
        for directory in ["src", "scripts", "tests"] {
            std::fs::create_dir_all(temporary.path().join(directory))
                .expect("privacy source directory");
        }
        let relative = PathBuf::from(format!("src/{case}_receiver_observation.rs"));
        std::fs::write(
            temporary.path().join(&relative),
            format!(r#"const {binding}: &str = "{literal}"; // ReceiverObservation"#),
        )
        .expect("privacy mutation source");

        assert!(
            discover_relevant_sources(temporary.path()).contains(&relative),
            "mutation source must be discovered for {case}"
        );
        assert!(
            !source_privacy_violations_for_path(temporary.path(), &relative).is_empty(),
            "privacy policy accepted a private {case} literal"
        );
    }
}

#[test]
fn generic_home_email_and_host_literals_remain_allowed() {
    let source = r#"
        const MAC_HOME: &str = "/Users/example/workspace";
        const UNIX_HOME: &str = "/home/tester/workspace";
        const WINDOWS_HOME: &str = r"C:\Users\<user>\workspace";
        const EMAIL: &str = "sender@nested.example.org";
        const URL: &str = "https://receiver.example.test/callback";
        const LOCAL_URL: &str = "http://localhost:8080/callback";
        const LOOPBACK_URL: &str = "http://127.0.0.1:8080/callback";
        const IPV6_LOOPBACK_URL: &str = "http://[::1]:8080/callback";
        const IPV6_DOCUMENTATION: &str = "2001:db8::1";
    "#;

    assert!(
        source_privacy_violations(source, true).is_empty(),
        "generic privacy literals were rejected"
    );
}

#[test]
fn future_observation_and_completion_modules_are_discovered_by_path() {
    let temporary = tempfile::tempdir().expect("temporary repository");
    for directory in ["src/agent", "src/state/receiver", "scripts", "tests"] {
        std::fs::create_dir_all(temporary.path().join(directory))
            .expect("privacy source directory");
    }
    let expected = [
        PathBuf::from("src/agent/future_observation.rs"),
        PathBuf::from("src/state/receiver/future_completion.rs"),
    ];
    for relative in &expected {
        std::fs::write(temporary.path().join(relative), "const VALUE: u8 = 1;")
            .expect("future privacy surface");
    }

    let discovered = discover_relevant_sources(temporary.path());
    for relative in expected {
        assert!(
            discovered.contains(&relative),
            "future surface was not discovered: {}",
            relative.display()
        );
    }
}

pub(super) fn discover_relevant_sources(root: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    collect_sources(&root.join("src"), &mut candidates);
    collect_sources(&root.join("scripts"), &mut candidates);
    collect_sources(&root.join("tests"), &mut candidates);
    candidates
        .into_iter()
        .filter_map(|path| {
            let relative = path
                .strip_prefix(root)
                .expect("repository source")
                .to_path_buf();
            if relative == Path::new("tests/receiver_observation_privacy.rs")
                || relative.starts_with("tests/receiver_observation_privacy")
            {
                return None;
            }
            let source = std::fs::read_to_string(&path).ok()?;
            let relevant_path = is_path_based_producer(&relative);
            let relevant_marker = [
                "AgentObservation",
                "ReceiverObservation",
                "receiver_observation",
                "BRAIN_RECEIVER_",
                "completion_status",
                "ReceiverCompletion",
                "complete_receiver_job_with_binding",
                "agent_session_stop_hook",
            ]
            .into_iter()
            .any(|marker| source.contains(marker));
            (relevant_path || relevant_marker).then_some(relative)
        })
        .collect()
}

fn source_privacy_violations_for_path(root: &Path, relative: &Path) -> Vec<&'static str> {
    let source = std::fs::read_to_string(root.join(relative))
        .unwrap_or_else(|error| panic!("read privacy source: {error}"));
    source_privacy_violations(&source, is_path_based_producer(relative))
}

fn is_path_based_producer(relative: &Path) -> bool {
    let surface_path = relative.to_string_lossy().to_ascii_lowercase();
    surface_path.contains("observation")
        || surface_path.contains("receiver") && surface_path.contains("completion")
}

fn collect_sources(directory: &Path, output: &mut Vec<PathBuf>) {
    let mut entries = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read privacy source directory: {error}"))
        .collect::<Result<Vec<_>, _>>()
        .expect("source directory entries");
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, output);
        } else if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("rs" | "py" | "js")
        ) {
            output.push(path);
        }
    }
}

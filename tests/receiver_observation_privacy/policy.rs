use std::path::{Path, PathBuf};

#[path = "policy/literals.rs"]
mod literals;

use literals::source_privacy_violations;

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
        "src/state/receiver/store/completion.rs",
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
        "semantic discovery unexpectedly narrowed: {audited:?}"
    );

    for relative in audited {
        let violations = source_privacy_violations_for_path(root, &relative);
        assert!(
            violations.is_empty(),
            "{} contains private literals: {violations:?}",
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

    assert_eq!(source_privacy_violations(source, true), Vec::<&str>::new());
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
        .unwrap_or_else(|error| panic!("read {}: {error}", relative.display()));
    source_privacy_violations(&source, is_path_based_producer(relative))
}

fn is_path_based_producer(relative: &Path) -> bool {
    let surface_path = relative.to_string_lossy().to_ascii_lowercase();
    surface_path.contains("observation")
        || surface_path.contains("receiver") && surface_path.contains("completion")
}

fn collect_sources(directory: &Path, output: &mut Vec<PathBuf>) {
    let mut entries = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
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

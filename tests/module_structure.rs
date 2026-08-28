use std::path::Path;
use std::process::Command;

#[test]
fn tracked_rust_test_locations_use_behavior_owned_filenames() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("git")
        .args(["ls-files", "src", "tests"])
        .current_dir(manifest_dir)
        .output()
        .expect("list tracked Rust files");

    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tracked_files = String::from_utf8_lossy(&output.stdout);
    let numbered_fragments: Vec<_> = tracked_files
        .lines()
        .filter(|path| {
            Path::new(path)
                .extension()
                .is_some_and(|extension| extension == "rs")
        })
        .filter(|path| {
            Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_numbered_fragment)
        })
        .collect();

    assert!(
        numbered_fragments.is_empty(),
        "numbered test fragments must use behavior-owned filenames:\n{}",
        numbered_fragments.join("\n")
    );
}

fn is_numbered_fragment(filename: &str) -> bool {
    filename
        .strip_prefix("part_")
        .and_then(|suffix| suffix.strip_suffix(".rs"))
        .is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn receiver_module_line_count(source: &str) -> usize {
    source.lines().count()
}

#[test]
fn receiver_module_guard_counts_production_after_test_only_items() {
    let source = "pub fn before() {}\n#[cfg(test)]\npub use tests::fixture;\npub fn after() {}\n";

    assert_eq!(receiver_module_line_count(source), 4);
}

#[test]
fn receiver_recovery_model_and_schema_use_cohesive_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "src/state/receiver/model.rs",
        "src/state/receiver/model/claim.rs",
        "src/state/receiver/model/conversation.rs",
        "src/state/receiver/model/delivery/mod.rs",
        "src/state/receiver/model/delivery/decode.rs",
        "src/state/receiver/model/delivery/envelope.rs",
        "src/state/receiver/model/delivery/identity.rs",
        "src/state/receiver/model/delivery/status.rs",
        "src/state/receiver/model/effect.rs",
        "src/state/receiver/model/identity.rs",
        "src/state/receiver/model/job.rs",
        "src/state/receiver/model/observation.rs",
        "src/state/receiver/delivery_policy.rs",
        "src/state/receiver/schema.rs",
        "src/state/receiver/schema/delivery.rs",
        "src/state/receiver/schema/delivery/cleanup_schema.rs",
        "src/state/receiver/schema/downgrade.rs",
        "src/state/receiver/schema/notice.rs",
        "src/state/receiver/store/completion/mod.rs",
        "src/state/receiver/store/completion/authorization.rs",
        "src/state/receiver/store/completion/duplicate.rs",
        "src/state/receiver/store/completion/lifecycle.rs",
        "src/state/receiver/store/completion/transaction.rs",
        "src/state/receiver/store/delivery/mod.rs",
        "src/state/receiver/store/delivery/claim.rs",
        "src/state/receiver/store/delivery/decode.rs",
        "src/state/receiver/store/delivery/reconciliation.rs",
        "src/state/receiver/store/delivery/result.rs",
        "src/tui/app_brain/tests/receiver_durable_answer_commit.rs",
        "src/tui/app_brain/tests/receiver_durable_producer_matrix.rs",
        "src/tui/app_brain/tests/receiver_durable_producer_support.rs",
        "src/tui/app_brain/tests/receiver_recovery_native_cleanup.rs",
        "src/tui/app_brain/tests/receiver_recovery_native_cleanup_support.rs",
    ] {
        let source = std::fs::read_to_string(root.join(relative)).expect("receiver module source");
        let module_lines = receiver_module_line_count(&source);
        assert!(
            module_lines <= 400,
            "{relative} has {module_lines} module lines"
        );
    }
}

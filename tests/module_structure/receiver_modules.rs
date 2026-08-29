use std::path::Path;

use super::receiver_counter::receiver_production_line_count;

fn discover_receiver_production_modules(root: &Path) -> Vec<std::path::PathBuf> {
    let mut modules = rust_modules_below(&root.join("src"));
    modules.retain(|path| {
        let relative = path.strip_prefix(root).expect("repository module");
        let text = relative.to_string_lossy();
        let is_test_source = text.split('/').any(|component| component == "tests")
            || relative
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "tests.rs");
        !is_test_source
            && (text.starts_with("src/state/receiver/")
                || text == "src/state/receiver.rs"
                || text.starts_with("src/server/delivery/")
                || text == "src/server/delivery.rs"
                || text.starts_with("src/tui/state/services/receiver_")
                || text == "src/tui/state/services.rs"
                || text.starts_with("src/tui/app_brain/receiver/")
                || text == "src/tui/app_brain/receiver.rs")
    });
    modules.dedup();
    modules
}

fn rust_modules_below(directory: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut modules = entries
        .collect::<Result<Vec<_>, _>>()
        .expect("receiver module directory entries")
        .into_iter()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                rust_modules_below(&path)
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>();
    modules.sort();
    modules
}

fn receiver_privacy_module_budget_violations(root: &Path) -> Vec<(std::path::PathBuf, usize)> {
    rust_modules_below(&root.join("tests/receiver_observation_privacy"))
        .into_iter()
        .filter(|path| {
            !path
                .strip_prefix(root)
                .expect("repository privacy module")
                .components()
                .any(|component| component.as_os_str() == "fixtures")
        })
        .filter_map(|path| {
            let lines = std::fs::read_to_string(&path)
                .expect("receiver privacy module")
                .lines()
                .count();
            (lines > 400).then_some((path, lines))
        })
        .collect()
}

#[test]
fn receiver_module_guard_discovers_nested_br17_production_modules() {
    let temporary = tempfile::tempdir().expect("temporary repository");
    for relative in [
        "src/state/receiver/schema/delivery/nested.rs",
        "src/state/receiver/store/completion/preparation.rs",
        "src/state/receiver/future_delivery.rs",
        "src/server/delivery/future.rs",
        "src/tui/state/services/receiver_delivery_future.rs",
        "src/tui/state/services/receiver_recovery.rs",
        "src/tui/state/services.rs",
        "src/tui/app_brain/receiver.rs",
        "src/tui/app_brain/receiver/answer_cleanup.rs",
        "src/state/receiver/tests/unrelated.rs",
    ] {
        let path = temporary.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
        std::fs::write(path, "pub fn fixture() {}\n").expect("fixture module");
    }

    let discovered = discover_receiver_production_modules(temporary.path());

    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("schema/delivery/nested.rs")),
        "nested delivery schema module was not discovered"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("store/completion/preparation.rs")),
        "nested completion store module was not discovered"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("state/receiver/future_delivery.rs")),
        "future receiver production module was not discovered"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("server/delivery/future.rs")),
        "future provider delivery module was not discovered"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("services/receiver_delivery_future.rs")),
        "future App delivery service module was not discovered"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("services/receiver_recovery.rs")),
        "App recovery coordinator was not discovered"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("tui/state/services.rs")),
        "App services coordinator root was not discovered"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("app_brain/receiver.rs")),
        "App receiver coordinator was not discovered"
    );
    assert!(
        discovered
            .iter()
            .any(|path| path.ends_with("receiver/answer_cleanup.rs")),
        "nested App receiver coordinator was not discovered"
    );
    assert!(
        discovered
            .iter()
            .all(|path| !path.ends_with("tests/unrelated.rs")),
        "unrelated receiver test module entered the production budget"
    );
}

#[test]
fn receiver_recovery_model_and_schema_use_cohesive_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut modules = discover_receiver_production_modules(root);
    modules.extend([
        root.join("src/tui/app_brain/tests/receiver_durable_answer_commit.rs"),
        root.join("src/tui/app_brain/tests/receiver_durable_producer_matrix.rs"),
        root.join("src/tui/app_brain/tests/receiver_durable_producer_support.rs"),
        root.join("src/tui/app_brain/tests/receiver_recovery_native_cleanup.rs"),
        root.join("src/tui/app_brain/tests/receiver_recovery_native_cleanup_support.rs"),
    ]);
    modules.sort();
    modules.dedup();
    for path in modules {
        let source = std::fs::read_to_string(&path).expect("receiver module source");
        let module_lines = receiver_production_line_count(&source);
        let relative = path.strip_prefix(root).expect("repository module");
        assert!(
            module_lines <= 400,
            "{} has {module_lines} module lines",
            relative.display()
        );
    }
}

#[test]
fn receiver_delivery_schema_root_stays_thin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join("src/state/receiver/schema/delivery.rs"))
        .expect("receiver delivery schema root");
    let nonblank_lines = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    assert!(
        nonblank_lines <= 80,
        "receiver delivery schema root has {nonblank_lines} nonblank lines"
    );
}

#[test]
fn receiver_completion_and_privacy_suites_stay_cohesive() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let completion_root = root.join("src/state/receiver/tests/completion_answer.rs");
    let completion_source =
        std::fs::read_to_string(&completion_root).expect("receiver completion test root");
    assert!(
        completion_source
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            <= 10,
        "receiver completion test root is no longer thin"
    );
    let completion_parts =
        rust_modules_below(&root.join("src/state/receiver/tests/completion_answer"));
    assert!(
        completion_parts.len() >= 5,
        "receiver completion behavior modules unexpectedly narrowed"
    );
    for path in completion_parts {
        let lines = std::fs::read_to_string(&path)
            .expect("receiver completion behavior module")
            .lines()
            .count();
        assert!(
            lines <= 400,
            "{} has {lines} test lines",
            path.strip_prefix(root)
                .expect("repository test module")
                .display()
        );
    }

    if let Some((path, lines)) = receiver_privacy_module_budget_violations(root)
        .into_iter()
        .next()
    {
        let relative = path.strip_prefix(root).expect("repository privacy module");
        panic!("{} has {lines} test lines", relative.display());
    }
}

#[test]
fn receiver_privacy_guard_recurses_into_every_split_part_and_rejects_a_401_line_mod_root() {
    let temporary = tempfile::tempdir().expect("temporary repository");
    let assertion_root = temporary
        .path()
        .join("tests/receiver_observation_privacy/policy/task_three_assertions");
    std::fs::create_dir_all(&assertion_root).expect("privacy assertion directory");
    std::fs::write(
        assertion_root.join("mod.rs"),
        "// guarded module\n".repeat(401),
    )
    .expect("oversized privacy root");
    std::fs::write(assertion_root.join("syntax.rs"), "// syntax\n").expect("privacy syntax module");
    std::fs::write(assertion_root.join("taint.rs"), "// taint\n").expect("privacy taint module");
    let fixture = temporary
        .path()
        .join("tests/receiver_observation_privacy/fixtures/generated.rs");
    std::fs::create_dir_all(fixture.parent().expect("privacy fixture directory"))
        .expect("privacy fixture directory");
    std::fs::write(&fixture, "// generated fixture\n".repeat(401))
        .expect("generated privacy fixture");

    let violations = receiver_privacy_module_budget_violations(temporary.path());

    assert!(
        violations.len() == 1
            && violations[0].0.ends_with("task_three_assertions/mod.rs")
            && violations[0].1 == 401,
        "recursive privacy guard did not isolate the oversized split module"
    );
}

#[test]
fn nonterminal_receiver_observation_has_no_answerless_completion_path() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let services = std::fs::read_to_string(root.join("src/tui/state/services.rs"))
        .expect("App services source");
    let active = std::fs::read_to_string(root.join("src/tui/app_brain/receiver/active.rs"))
        .expect("active receiver source");
    let terminal =
        std::fs::read_to_string(root.join("src/tui/app_brain/receiver/active/terminal.rs"))
            .expect("receiver terminal source");

    assert!(
        !services.contains("pub(crate) completed: bool") && !services.contains("completed: false"),
        "nonterminal observation still exposes an always-false completion flag"
    );
    assert!(
        !active.contains("outcome.completed")
            && !active.contains("finish_observation_only_receiver_run"),
        "active receiver still branches into answerless completion"
    );
    assert!(
        !terminal.contains("finish_observation_only_receiver_run"),
        "terminal receiver still contains unreachable answerless cleanup"
    );
}

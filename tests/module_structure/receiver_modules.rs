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

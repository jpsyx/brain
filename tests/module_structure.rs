use std::path::Path;
use std::process::Command;

#[path = "module_structure/receiver_counter.rs"]
mod receiver_counter;
#[path = "module_structure/receiver_modules.rs"]
mod receiver_modules;

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

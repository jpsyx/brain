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

fn receiver_production_line_count(source: &str) -> usize {
    let masked = mask_rust_non_code(source);
    let mut production_lines = 0;
    let mut skipping_test_item = false;
    let mut test_item_has_body = false;
    let mut test_item_brace_depth = 0_i64;

    for line in masked.lines() {
        let trimmed = line.trim_start();
        let item_source = if skipping_test_item {
            line
        } else if let Some(remainder) = trimmed.strip_prefix("#[cfg(test)]") {
            skipping_test_item = true;
            remainder
        } else {
            production_lines += 1;
            continue;
        };

        let opens = i64::try_from(item_source.bytes().filter(|byte| *byte == b'{').count())
            .expect("test-only item brace count fits i64");
        let closes = i64::try_from(item_source.bytes().filter(|byte| *byte == b'}').count())
            .expect("test-only item brace count fits i64");
        if opens > 0 {
            test_item_has_body = true;
        }
        test_item_brace_depth += opens - closes;
        let item_finished = if test_item_has_body {
            test_item_brace_depth == 0
        } else {
            item_source.contains(';')
        };
        if item_finished {
            skipping_test_item = false;
            test_item_has_body = false;
            test_item_brace_depth = 0;
        }
    }

    production_lines
}

fn mask_rust_non_code(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            let end = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |relative| index + relative);
            mask_range(&mut masked, index, end);
            index = end;
        } else if bytes[index..].starts_with(b"/*") {
            let end = block_comment_end(bytes, index).unwrap_or(bytes.len());
            mask_range(&mut masked, index, end);
            index = end;
        } else if let Some(end) = raw_string_end(bytes, index) {
            mask_range(&mut masked, index, end);
            index = end;
        } else if bytes[index] == b'"' {
            let end = quoted_end(bytes, index, b'"');
            mask_range(&mut masked, index, end);
            index = end;
        } else if bytes[index] == b'\'' && looks_like_character_literal(bytes, index) {
            let end = quoted_end(bytes, index, b'\'');
            mask_range(&mut masked, index, end);
            index = end;
        } else {
            index += 1;
        }
    }
    String::from_utf8(masked).expect("masked Rust source remains UTF-8")
}

fn mask_range(masked: &mut [u8], start: usize, end: usize) {
    for byte in &mut masked[start..end] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
        } else if bytes[index] == quote {
            return index + 1;
        } else {
            index += 1;
        }
    }
    bytes.len()
}

fn looks_like_character_literal(bytes: &[u8], start: usize) -> bool {
    let end = quoted_end(bytes, start, b'\'');
    end <= bytes.len() && end.saturating_sub(start) <= 6 && bytes.get(end - 1) == Some(&b'\'')
}

fn raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hash_start = cursor;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    let hashes = cursor - hash_start;
    cursor += 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes
                .get(cursor + 1..cursor + 1 + hashes)
                .is_some_and(|candidate| candidate.iter().all(|byte| *byte == b'#'))
        {
            return Some(cursor + 1 + hashes);
        }
        cursor += 1;
    }
    Some(bytes.len())
}

fn block_comment_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1_usize;
    let mut index = start + 2;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"/*") {
            depth += 1;
            index += 2;
        } else if bytes[index..].starts_with(b"*/") {
            depth = depth.checked_sub(1)?;
            index += 2;
            if depth == 0 {
                return Some(index);
            }
        } else {
            index += 1;
        }
    }
    None
}

fn discover_receiver_production_modules(root: &Path) -> Vec<std::path::PathBuf> {
    let mut modules = [
        "src/state/receiver/model",
        "src/state/receiver/schema",
        "src/state/receiver/store/completion",
        "src/state/receiver/store/delivery",
    ]
    .into_iter()
    .flat_map(|relative| rust_modules_below(&root.join(relative)))
    .collect::<Vec<_>>();
    for relative in [
        "src/state/receiver/model.rs",
        "src/state/receiver/schema.rs",
        "src/state/receiver/delivery_policy.rs",
    ] {
        let path = root.join(relative);
        if path.is_file() {
            modules.push(path);
        }
    }
    modules.sort();
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
fn receiver_module_guard_counts_production_after_test_only_items() {
    let source = "pub fn before() {}\n#[cfg(test)]\npub use tests::fixture;\npub fn after() {}\n";

    assert_eq!(receiver_production_line_count(source), 2);
}

#[test]
fn receiver_module_guard_excludes_a_large_inline_test_module() {
    let mut source = String::from("pub fn before() {}\n#[cfg(test)]\nmod tests {\n");
    source.push_str("    const CLOSING_BRACES: &str = \"}}\";\n");
    source.extend(std::iter::repeat_n("    #[test] fn case() {}\n", 450));
    source.push_str("}\npub fn after() {}\n");

    assert_eq!(receiver_production_line_count(&source), 2);
}

#[test]
fn receiver_module_guard_counts_large_production_after_inline_tests() {
    let mut source = String::from("#[cfg(test)]\nmod tests {\n    #[test] fn case() {}\n}\n");
    source.extend(std::iter::repeat_n("pub fn production() {}\n", 401));

    assert_eq!(receiver_production_line_count(&source), 401);
}

#[test]
fn receiver_module_guard_discovers_nested_br17_production_modules() {
    let temporary = tempfile::tempdir().expect("temporary repository");
    for relative in [
        "src/state/receiver/schema/delivery/nested.rs",
        "src/state/receiver/store/completion/preparation.rs",
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

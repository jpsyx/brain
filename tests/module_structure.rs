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
    let mut excluded = vec![false; masked.len()];
    for range in test_only_target_ranges(&masked) {
        excluded[range].fill(true);
    }
    masked
        .as_bytes()
        .split_inclusive(|byte| *byte == b'\n')
        .scan(0_usize, |start, line| {
            let line_start = *start;
            *start += line.len();
            Some((line_start, line))
        })
        .filter(|(line_start, line)| {
            line.iter()
                .enumerate()
                .any(|(offset, byte)| !byte.is_ascii_whitespace() && !excluded[line_start + offset])
        })
        .count()
}

fn test_only_target_ranges(source: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'#' {
            cursor += 1;
            continue;
        }
        let attribute_start = cursor;
        let mut next = cursor;
        let mut test_only = false;
        let mut found_attribute = false;
        loop {
            let hash = skip_ascii_whitespace(bytes, next);
            if bytes.get(hash) != Some(&b'#') {
                next = hash;
                break;
            }
            let bracket = skip_ascii_whitespace(bytes, hash + 1);
            if bytes.get(bracket) != Some(&b'[') {
                break;
            }
            let Some(end) = matching_byte_delimiter(bytes, bracket, b'[', b']') else {
                break;
            };
            found_attribute = true;
            test_only |= cfg_attribute_is_test_only(&source[bracket + 1..end]);
            next = end + 1;
        }
        if !found_attribute {
            cursor += 1;
        } else if test_only {
            let target_end = attributed_target_end(bytes, next);
            ranges.push(attribute_start..target_end);
            cursor = target_end.max(attribute_start + 1);
        } else {
            cursor = next.max(attribute_start + 1);
        }
    }
    ranges
}

fn cfg_attribute_is_test_only(attribute: &str) -> bool {
    let mut parser = CfgParser::new(attribute);
    parser.identifier().as_deref() == Some("cfg")
        && parser.punctuation(b'(')
        && parser.predicate()
        && parser.punctuation(b')')
}

struct CfgParser<'source> {
    source: &'source [u8],
    cursor: usize,
}

impl<'source> CfgParser<'source> {
    const fn new(source: &'source str) -> Self {
        Self {
            source: source.as_bytes(),
            cursor: 0,
        }
    }

    fn predicate(&mut self) -> bool {
        let Some(name) = self.identifier() else {
            return false;
        };
        if name == "test" {
            return true;
        }
        if !self.punctuation(b'(') {
            while self.cursor < self.source.len()
                && !matches!(self.source[self.cursor], b',' | b')')
            {
                self.cursor += 1;
            }
            return false;
        }
        let mut predicates = Vec::new();
        while self.peek() != Some(b')') && self.peek().is_some() {
            predicates.push(self.predicate());
            if !self.punctuation(b',') {
                break;
            }
        }
        let _ = self.punctuation(b')');
        match name.as_str() {
            "all" => predicates.into_iter().any(std::convert::identity),
            "any" => !predicates.is_empty() && predicates.into_iter().all(std::convert::identity),
            _ => false,
        }
    }

    fn identifier(&mut self) -> Option<String> {
        self.skip_whitespace();
        let start = self.cursor;
        while self
            .source
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.cursor += 1;
        }
        (self.cursor > start)
            .then(|| String::from_utf8_lossy(&self.source[start..self.cursor]).into_owned())
    }

    fn punctuation(&mut self, expected: u8) -> bool {
        self.skip_whitespace();
        if self.source.get(self.cursor) == Some(&expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_whitespace();
        self.source.get(self.cursor).copied()
    }

    fn skip_whitespace(&mut self) {
        self.cursor = skip_ascii_whitespace(self.source, self.cursor);
    }
}

fn attributed_target_end(bytes: &[u8], start: usize) -> usize {
    let mut cursor = skip_ascii_whitespace(bytes, start);
    let mut delimiters = Vec::new();
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'(' | b'[' => delimiters.push(bytes[cursor]),
            b'{' if delimiters.is_empty() => {
                let Some(close) = matching_byte_delimiter(bytes, cursor, b'{', b'}') else {
                    return bytes.len();
                };
                let trailing = skip_ascii_whitespace(bytes, close + 1);
                return if matches!(bytes.get(trailing), Some(b',' | b';')) {
                    trailing + 1
                } else {
                    close + 1
                };
            }
            b'{' => delimiters.push(b'{'),
            b')' if delimiters.last() == Some(&b'(') => {
                delimiters.pop();
            }
            b']' if delimiters.last() == Some(&b'[') => {
                delimiters.pop();
            }
            b'}' if delimiters.last() == Some(&b'{') => {
                delimiters.pop();
            }
            b',' | b';' if delimiters.is_empty() => return cursor + 1,
            _ => {}
        }
        cursor += 1;
    }
    bytes.len()
}

fn matching_byte_delimiter(
    bytes: &[u8],
    opening_index: usize,
    opening: u8,
    closing: u8,
) -> Option<usize> {
    let mut depth = 0_usize;
    for (offset, byte) in bytes.iter().copied().enumerate().skip(opening_index) {
        if byte == opening {
            depth += 1;
        } else if byte == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(offset);
            }
        }
    }
    None
}

fn skip_ascii_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
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
                || text.starts_with("src/tui/state/services/receiver_delivery"))
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
fn receiver_module_guard_excludes_comma_terminated_test_fields_and_variants() {
    for source in [
        "pub struct Record {\n#[cfg(test)]\ntest_field: String,\nproduction_field: String,\n}\n",
        "pub enum Choice {\n#[cfg(test)]\nTestOnly,\nProduction,\n}\n",
    ] {
        assert_eq!(receiver_production_line_count(source), 3);
    }
}

#[test]
fn receiver_module_guard_parses_composed_and_stacked_test_attributes() {
    let source = r#"
pub enum Choice {
    #[cfg(
        all(
            test,
            feature = "fixture"
        )
    )]
    #[allow(dead_code)]
    TestOnly {
        value: &'static str,
    },
    Production,
}
pub fn after() {}
"#;

    assert_eq!(receiver_production_line_count(source), 4);
}

#[test]
fn receiver_module_guard_lexes_every_test_target_shape_and_resumes_production() {
    let source = r##"
pub fn before() {}
#[cfg(test)]
const TEST_TEXT: &str = r#"}; /* not code */"#;
#[cfg(test)]
const TEST_CHARACTER: char = '}';
#[cfg(test)]
mod tests {
    const VALUE: &str = "{";
    /* outer { /* nested } */ } */
}
pub fn after() {}
"##;

    assert_eq!(receiver_production_line_count(source), 2);
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

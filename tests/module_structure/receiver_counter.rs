#[path = "receiver_lexer.rs"]
mod receiver_lexer;

use receiver_lexer::mask_rust_non_code;

pub(super) fn receiver_production_line_count(source: &str) -> usize {
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
            b'<' if bytes.get(cursor.wrapping_sub(1)).is_some_and(|byte| {
                byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b':' | b'>')
            }) =>
            {
                delimiters.push(b'<');
            }
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
            b'>' if delimiters.last() == Some(&b'<') => {
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
fn receiver_production_line_counter_ignores_angle_delimited_generic_fields() {
    let source = "pub struct Record {\n#[cfg(test)]\ntest_field: std::collections::BTreeMap<String, Vec<(u8, u8)>>,\nproduction_field: String,\n}\n";

    assert_eq!(receiver_production_line_count(source), 3);
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

pub(super) fn privacy_diagnostic_violations(source: &str) -> Vec<&'static str> {
    let masked = mask_non_code(source);
    let mut violations = Vec::new();
    for forbidden in ["assert_eq!", "assert_ne!"] {
        if masked.contains(forbidden) {
            violations.push("whole-value assertion");
        }
    }
    for macro_name in ["assert!", "panic!", "eprintln!", "println!"] {
        for body in macro_bodies(&masked, macro_name) {
            let diagnostic = if macro_name == "assert!" {
                diagnostic_arguments(body, source, &masked)
            } else {
                &source[body]
            };
            if contains_private_diagnostic_value(diagnostic) {
                violations.push("private diagnostic interpolation");
            }
        }
    }
    for body in macro_bodies(&masked, "format!") {
        let diagnostic = &source[body];
        if ["output", "stdout", "stderr"]
            .into_iter()
            .any(|identifier| identifier_present(diagnostic, identifier))
        {
            violations.push("captured process formatting");
        }
    }
    violations
}

fn contains_private_diagnostic_value(diagnostic: &str) -> bool {
    let masked = mask_non_code(diagnostic);
    diagnostic.contains(":?")
        || [
            "output", "stdout", "stderr", "canary", "token", "secret", "literal", "rendered",
            "debug",
        ]
        .into_iter()
        .any(|identifier| {
            identifier_present(&masked, identifier)
                || format_placeholder_mentions(diagnostic, identifier)
        })
}

fn format_placeholder_mentions(source: &str, identifier: &str) -> bool {
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find('{') {
        let start = cursor + relative + 1;
        if source.as_bytes().get(start) == Some(&b'{') {
            cursor = start + 1;
            continue;
        }
        let Some(relative_end) = source[start..].find('}') else {
            return false;
        };
        let end = start + relative_end;
        let expression = source[start..end]
            .split_once(':')
            .map_or(&source[start..end], |(value, _)| value);
        if identifier_present(expression, identifier) {
            return true;
        }
        cursor = end + 1;
    }
    false
}

fn diagnostic_arguments<'source>(
    body: std::ops::Range<usize>,
    source: &'source str,
    masked: &str,
) -> &'source str {
    let masked_body = &masked[body.clone()];
    let Some(comma) = top_level_comma(masked_body) else {
        return "";
    };
    &source[body.start + comma + 1..body.end]
}

fn macro_bodies(masked: &str, macro_name: &str) -> Vec<std::ops::Range<usize>> {
    let mut bodies = Vec::new();
    let mut search_start = 0;
    while let Some(relative) = masked[search_start..].find(macro_name) {
        let macro_start = search_start + relative;
        let after_name = macro_start + macro_name.len();
        if masked[..macro_start]
            .chars()
            .next_back()
            .is_some_and(is_identifier_character)
        {
            search_start = after_name;
            continue;
        }
        let Some((opening_index, opening)) = masked[after_name..]
            .char_indices()
            .find(|(_, character)| !character.is_whitespace())
            .map(|(index, character)| (after_name + index, character))
        else {
            break;
        };
        let closing = match opening {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            _ => {
                search_start = after_name;
                continue;
            }
        };
        if let Some(end) = matching_delimiter(masked, opening_index, opening, closing) {
            bodies.push(opening_index + 1..end);
            search_start = end + closing.len_utf8();
        } else {
            search_start = opening_index + opening.len_utf8();
        }
    }
    bodies
}

fn top_level_comma(source: &str) -> Option<usize> {
    let mut delimiters = Vec::new();
    for (index, character) in source.char_indices() {
        match character {
            '(' | '[' | '{' => delimiters.push(character),
            ')' | ']' | '}' => {
                delimiters.pop();
            }
            ',' if delimiters.is_empty() => return Some(index),
            _ => {}
        }
    }
    None
}

fn matching_delimiter(
    source: &str,
    opening_index: usize,
    opening: char,
    closing: char,
) -> Option<usize> {
    let mut depth = 0_usize;
    for (relative, character) in source[opening_index..].char_indices() {
        if character == opening {
            depth += 1;
        } else if character == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(opening_index + relative);
            }
        }
    }
    None
}

fn mask_non_code(source: &str) -> String {
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

fn identifier_present(source: &str, identifier: &str) -> bool {
    source.match_indices(identifier).any(|(index, value)| {
        let before = source[..index].chars().next_back();
        let after = source[index + value.len()..].chars().next();
        !before.is_some_and(is_identifier_character) && !after.is_some_and(is_identifier_character)
    })
}

const fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

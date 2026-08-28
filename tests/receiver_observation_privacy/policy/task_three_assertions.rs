use std::collections::BTreeSet;

pub(super) fn private_whole_value_assertion_violations(source: &str) -> usize {
    let masked = mask_non_code(source);
    let mut violations = 0;
    for macro_name in ["assert_eq!", "assert_ne!"] {
        for body in macro_bodies(&masked, macro_name) {
            let private = private_identifiers_at(&masked, body.start);
            let arguments = top_level_arguments(&masked[body.clone()]);
            for argument in arguments.iter().take(2) {
                let absolute = body.start + argument.start..body.start + argument.end;
                if expression_exposes_private(&masked[absolute.clone()], &private)
                    || format_placeholder_exposes_private(&source[absolute.clone()], &private)
                {
                    violations += 1;
                }
            }
            if diagnostics_expose_private(source, &masked, body, &arguments[2..], &private) {
                violations += 1;
            }
        }
    }
    for body in macro_bodies(&masked, "assert!") {
        let private = private_identifiers_at(&masked, body.start);
        let arguments = top_level_arguments(&masked[body.clone()]);
        if diagnostics_expose_private(source, &masked, body, &arguments[1..], &private) {
            violations += 1;
        }
    }
    violations
}

fn private_identifiers_at(masked: &str, offset: usize) -> BTreeSet<String> {
    let mut private = identifiers(masked)
        .filter(|identifier| identifier_is_private(identifier))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let scope_start = enclosing_function_body_start(masked, offset).unwrap_or(0);
    let scope = &masked[scope_start..offset];
    loop {
        let before = private.len();
        for statement in scope.split(';') {
            let Some((left, right)) = assignment(statement) else {
                continue;
            };
            if !expression_exposes_private(right, &private) {
                continue;
            }
            if let Some(alias) = identifiers(left).last() {
                private.insert(alias.to_owned());
            }
        }
        if private.len() == before {
            return private;
        }
    }
}

fn enclosing_function_body_start(masked: &str, offset: usize) -> Option<usize> {
    let bytes = masked.as_bytes();
    let mut cursor = 0;
    let mut body_start = None;
    while cursor + 2 <= offset {
        let Some(relative) = masked[cursor..offset].find("fn") else {
            break;
        };
        let function = cursor + relative;
        let bounded_before = function == 0 || !is_identifier_character(bytes[function - 1] as char);
        let bounded_after = bytes.get(function + 2).is_some_and(u8::is_ascii_whitespace);
        if bounded_before && bounded_after {
            let Some(open_relative) = masked[function + 2..offset].find('{') else {
                break;
            };
            let open = function + 2 + open_relative;
            if matching_delimiter(masked, open, '{', '}').is_some_and(|close| close >= offset) {
                body_start = Some(open + 1);
            }
        }
        cursor = function + 2;
    }
    body_start
}

fn assignment(statement: &str) -> Option<(&str, &str)> {
    let bytes = statement.as_bytes();
    let mut delimiters = Vec::new();
    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => delimiters.push(byte),
            b')' | b']' | b'}' => {
                delimiters.pop();
            }
            b'=' if delimiters.is_empty()
                && bytes.get(index.wrapping_sub(1)) != Some(&b'=')
                && bytes.get(index.wrapping_sub(1)) != Some(&b'!')
                && bytes.get(index.wrapping_sub(1)) != Some(&b'<')
                && bytes.get(index.wrapping_sub(1)) != Some(&b'>')
                && bytes.get(index + 1) != Some(&b'=') =>
            {
                return Some((&statement[..index], &statement[index + 1..]));
            }
            _ => {}
        }
    }
    None
}

fn identifier_is_private(identifier: &str) -> bool {
    let components = identifier.split('_').collect::<Vec<_>>();
    let content_free = components
        .iter()
        .any(|component| matches!(*component, "is" | "has"))
        || components.last().is_some_and(|component| {
            matches!(
                *component,
                "count"
                    | "len"
                    | "length"
                    | "size"
                    | "proof"
                    | "digest"
                    | "hash"
                    | "index"
                    | "present"
                    | "exists"
                    | "valid"
                    | "empty"
                    | "state"
                    | "status"
                    | "kind"
                    | "category"
            )
        });
    !content_free
        && components.iter().any(|component| {
            matches!(
                *component,
                "private"
                    | "prompt"
                    | "answer"
                    | "inbound"
                    | "transcript"
                    | "envelope"
                    | "evidence"
                    | "payload"
                    | "sender"
                    | "recipient"
                    | "recipients"
                    | "address"
                    | "body"
                    | "text"
                    | "html"
                    | "reference"
            )
        })
}

fn expression_exposes_private(expression: &str, private: &BTreeSet<String>) -> bool {
    if !identifiers(expression).any(|identifier| private.contains(identifier)) {
        return false;
    }
    !expression_is_content_free(expression, private)
}

fn expression_is_content_free(expression: &str, private: &BTreeSet<String>) -> bool {
    let expression = expression.trim();
    if let Some(inner) = strip_outer_parentheses(expression) {
        let elements = top_level_arguments(inner);
        if elements.len() > 1 {
            return elements
                .iter()
                .all(|element| !expression_exposes_private(&inner[element.clone()], private));
        }
        return expression_is_content_free(inner, private);
    }
    has_top_level_boolean_operator(expression)
        || is_exact_content_proof_call(expression)
        || ends_with_content_free_method(expression)
        || expression.trim_start().starts_with("matches!")
}

fn strip_outer_parentheses(source: &str) -> Option<&str> {
    let source = source.trim();
    if !source.starts_with('(') {
        return None;
    }
    let end = matching_delimiter(source, 0, '(', ')')?;
    (end + 1 == source.len()).then_some(&source[1..end])
}

fn has_top_level_boolean_operator(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut delimiters = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'(' | b'[' | b'{' => delimiters.push(bytes[index]),
            b')' | b']' | b'}' => {
                delimiters.pop();
            }
            b'=' | b'!' | b'<' | b'>' if delimiters.is_empty() => return true,
            b'&' if delimiters.is_empty() && bytes.get(index + 1) == Some(&b'&') => return true,
            b'|' if delimiters.is_empty() && bytes.get(index + 1) == Some(&b'|') => return true,
            _ => {}
        }
        index += 1;
    }
    false
}

fn is_exact_content_proof_call(source: &str) -> bool {
    let Some(open) = source.find('(') else {
        return false;
    };
    let name = source[..open].trim();
    let Some(close) = matching_delimiter(source, open, '(', ')') else {
        return false;
    };
    close + 1 == source.len()
        && matches!(
            name,
            "private_text_proof"
                | "private_bytes_proof"
                | "sha256_proof"
                | "classify_provider_http_response"
                | "classify_provider_process_failure"
                | "classify_provider_process_output"
        )
}

fn ends_with_content_free_method(source: &str) -> bool {
    let source = source.trim();
    let Some(close) = source
        .len()
        .checked_sub(1)
        .filter(|index| source.as_bytes()[*index] == b')')
    else {
        return false;
    };
    let Some(open) = matching_delimiter_backwards(source, close, '(', ')') else {
        return false;
    };
    let prefix = source[..open].trim_end();
    let Some(dot) = prefix.rfind('.') else {
        return false;
    };
    let method = prefix[dot + 1..].trim();
    matches!(
        method,
        "len"
            | "count"
            | "is_empty"
            | "is_some"
            | "is_none"
            | "is_ok"
            | "is_err"
            | "is_some_and"
            | "contains"
            | "starts_with"
            | "ends_with"
    ) || method.ends_with("_count")
        || method.ends_with("_len")
        || method.ends_with("_length")
        || method.starts_with("is_")
        || method.starts_with("has_")
        || method.starts_with("uses_")
}

fn diagnostics_expose_private(
    source: &str,
    masked: &str,
    body: std::ops::Range<usize>,
    arguments: &[std::ops::Range<usize>],
    private: &BTreeSet<String>,
) -> bool {
    arguments.iter().any(|argument| {
        let absolute = body.start + argument.start..body.start + argument.end;
        expression_exposes_private(&masked[absolute.clone()], private)
            || format_placeholder_exposes_private(&source[absolute], private)
    })
}

fn format_placeholder_exposes_private(source: &str, private: &BTreeSet<String>) -> bool {
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
        if identifiers(expression)
            .any(|identifier| private.contains(identifier) || identifier_is_private(identifier))
        {
            return true;
        }
        cursor = end + 1;
    }
    false
}

fn identifiers(source: &str) -> impl Iterator<Item = &str> {
    IdentifierIter { source, cursor: 0 }
}

struct IdentifierIter<'source> {
    source: &'source str,
    cursor: usize,
}

impl<'source> Iterator for IdentifierIter<'source> {
    type Item = &'source str;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.source.as_bytes();
        while bytes
            .get(self.cursor)
            .is_some_and(|byte| !byte.is_ascii_alphabetic() && *byte != b'_')
        {
            self.cursor += 1;
        }
        let start = self.cursor;
        while bytes
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.cursor += 1;
        }
        (self.cursor > start).then_some(&self.source[start..self.cursor])
    }
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

fn top_level_arguments(source: &str) -> Vec<std::ops::Range<usize>> {
    let mut arguments = Vec::new();
    let mut delimiters = Vec::new();
    let mut start = 0;
    for (index, character) in source.char_indices() {
        match character {
            '(' | '[' | '{' => delimiters.push(character),
            ')' | ']' | '}' => {
                delimiters.pop();
            }
            ',' if delimiters.is_empty() => {
                arguments.push(start..index);
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < source.len() {
        arguments.push(start..source.len());
    }
    arguments
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

fn matching_delimiter_backwards(
    source: &str,
    closing_index: usize,
    opening: char,
    closing: char,
) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, character) in source[..=closing_index].char_indices().rev() {
        if character == closing {
            depth += 1;
        } else if character == opening {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
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

const fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

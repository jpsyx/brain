pub(crate) fn code_tokens(source: &str) -> Vec<&str> {
    let masked = mask_non_code(source);
    let bytes = masked.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        } else if bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_' {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len()
                && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
            {
                cursor += 1;
            }
            tokens.push(&source[start..cursor]);
        } else {
            let start = cursor;
            cursor += 1;
            tokens.push(&source[start..cursor]);
        }
    }
    tokens
}

pub(crate) fn declares_function(source: &str, name: &str) -> bool {
    let masked = mask_non_code(source);
    masked.match_indices("fn").any(|(start, _)| {
        if !token_at(&masked, start, "fn") {
            return false;
        }
        let rest = masked[start + 2..].trim_start();
        rest.strip_prefix(name).is_some_and(|after| {
            after
                .chars()
                .next()
                .is_some_and(|character| character == '(' || character == '<')
        })
    })
}

pub(crate) fn impl_app_ranges(masked: &str) -> Vec<(usize, usize)> {
    impl_ranges(masked, "App")
}

pub(crate) fn impl_ranges(masked: &str, type_name: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for (start, _) in masked.match_indices("impl") {
        if !token_at(masked, start, "impl") {
            continue;
        }
        let rest_start = start + 4;
        let rest = masked[rest_start..].trim_start();
        if !rest.starts_with(type_name)
            || rest[type_name.len()..]
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            continue;
        }
        let open = rest_start + (masked[rest_start..].len() - rest.len()) + rest.find('{').unwrap();
        if let Some(close) = matching_brace(masked, open) {
            ranges.push((open, close));
        }
    }
    ranges
}

pub(crate) fn method_ranges(
    masked: &str,
    impl_open: usize,
    impl_close: usize,
) -> Vec<(usize, usize, usize)> {
    let mut ranges = Vec::new();
    let mut cursor = impl_open + 1;
    let mut depth = 0_usize;
    while cursor < impl_close {
        match masked.as_bytes()[cursor] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'f' if depth == 0 && token_at(masked, cursor, "fn") => {
                let Some(relative_open) = masked[cursor + 2..impl_close].find('{') else {
                    break;
                };
                let open = cursor + 2 + relative_open;
                if let Some(close) = matching_brace(masked, open) {
                    ranges.push((cursor, open, close));
                    cursor = close;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    ranges
}

pub(crate) fn matching_brace(masked: &str, open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (offset, byte) in masked.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn token_at(source: &str, start: usize, token: &str) -> bool {
    source[start..].starts_with(token)
        && source[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
        && source[start + token.len()..]
            .chars()
            .next()
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
}

pub(crate) fn is_identifier(candidate: &str) -> bool {
    let mut characters = candidate.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(crate) fn mask_non_code(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"//") {
            let end = bytes[cursor..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |length| cursor + length);
            masked[cursor..end].fill(b' ');
            cursor = end;
        } else if bytes[cursor..].starts_with(b"/*") {
            let mut end = cursor + 2;
            let mut depth = 1_usize;
            while end < bytes.len() && depth > 0 {
                if bytes[end..].starts_with(b"/*") {
                    depth += 1;
                    end += 2;
                } else if bytes[end..].starts_with(b"*/") {
                    depth -= 1;
                    end += 2;
                } else {
                    end += 1;
                }
            }
            for byte in &mut masked[cursor..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            cursor = end;
        } else if bytes[cursor] == b'"' {
            let mut end = cursor + 1;
            while end < bytes.len() {
                if bytes[end] == b'\\' {
                    end = (end + 2).min(bytes.len());
                } else if bytes[end] == b'"' {
                    end += 1;
                    break;
                } else {
                    end += 1;
                }
            }
            for byte in &mut masked[cursor..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    String::from_utf8(masked).expect("mask preserves UTF-8 bytes")
}

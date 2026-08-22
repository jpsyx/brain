#[derive(Debug)]
pub(super) struct Token {
    pub(super) text: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) fn contains_token_sequence(source: &str, expected: &[&str]) -> bool {
    rust_tokens(source).windows(expected.len()).any(|window| {
        window
            .iter()
            .map(|token| token.text.as_str())
            .zip(expected.iter().copied())
            .all(|(actual, expected)| actual == expected)
    })
}

pub(super) fn named_braced_body<'a>(source: &'a str, kind: &str, name: &str) -> Option<&'a str> {
    let tokens = rust_tokens(source);
    for index in 0..tokens.len().saturating_sub(2) {
        if tokens[index].text != kind || tokens[index + 1].text != name {
            continue;
        }
        let open = tokens[index + 2..]
            .iter()
            .position(|token| token.text == "{")
            .map(|relative| index + 2 + relative)?;
        let close = matching_token(&tokens, open, "{", "}")?;
        return Some(&source[tokens[open].end..tokens[close].start]);
    }
    None
}

pub(super) fn field_type_count(source: &str, expected: &[&str]) -> usize {
    let tokens = rust_tokens(source);
    tokens
        .windows(expected.len() + 2)
        .filter(|window| {
            window[1].text == ":"
                && window[2..]
                    .iter()
                    .map(|token| token.text.as_str())
                    .zip(expected.iter().copied())
                    .all(|(actual, expected)| actual == expected)
        })
        .count()
}

pub(super) fn function_parameter_counts(source: &str, name: &str) -> Vec<usize> {
    let tokens = rust_tokens(source);
    let mut counts = Vec::new();
    for index in 0..tokens.len().saturating_sub(3) {
        if tokens[index].text != "fn" || tokens[index + 1].text != name {
            continue;
        }
        let Some(open) = tokens[index + 2..]
            .iter()
            .position(|token| token.text == "(")
            .map(|relative| index + 2 + relative)
        else {
            continue;
        };
        let Some(close) = matching_token(&tokens, open, "(", ")") else {
            continue;
        };
        let mut depth = 0_usize;
        let commas = tokens[open + 1..close]
            .iter()
            .filter(|token| match token.text.as_str() {
                "(" | "[" | "<" | "{" => {
                    depth += 1;
                    false
                }
                ")" | "]" | ">" | "}" => {
                    depth = depth.saturating_sub(1);
                    false
                }
                "," => depth == 0,
                _ => false,
            })
            .count();
        counts.push(usize::from(open + 1 != close) + commas);
    }
    counts
}

pub(super) fn matching_token(
    tokens: &[Token],
    open: usize,
    opening: &str,
    closing: &str,
) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.text == opening {
            depth += 1;
        } else if token.text == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

pub(super) fn rust_tokens(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0_usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"//") {
            index = source[index..]
                .find('\n')
                .map_or(bytes.len(), |relative| index + relative + 1);
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index = skip_block_comment(bytes, index);
            continue;
        }
        if bytes[index] == b'r'
            && bytes
                .get(index + 1)
                .is_some_and(|byte| *byte == b'"' || *byte == b'#')
            && let Some(end) = skip_raw_string(bytes, index)
        {
            index = end;
            continue;
        }
        if bytes[index] == b'"' {
            index = skip_quoted(bytes, index, bytes[index]);
            continue;
        }
        if bytes[index] == b'\'' {
            if let Some(end) = lifetime_end(bytes, index) {
                tokens.push(Token {
                    text: "'".to_owned(),
                    start: index,
                    end: index + 1,
                });
                tokens.push(Token {
                    text: source[index + 1..end].to_owned(),
                    start: index + 1,
                    end,
                });
                index = end;
            } else {
                index = skip_quoted(bytes, index, bytes[index]);
            }
            continue;
        }
        let start = index;
        if bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' {
            index += 1;
            while bytes
                .get(index)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                index += 1;
            }
        } else if bytes[index..].starts_with(b"::") {
            index += 2;
        } else {
            index += 1;
        }
        tokens.push(Token {
            text: source[start..index].to_owned(),
            start,
            end: index,
        });
    }
    tokens
}

fn lifetime_end(bytes: &[u8], start: usize) -> Option<usize> {
    let first = *bytes.get(start + 1)?;
    if !first.is_ascii_alphabetic() && first != b'_' {
        return None;
    }
    let mut end = start + 2;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    (bytes.get(end) != Some(&b'\'')).then_some(end)
}

fn skip_block_comment(bytes: &[u8], mut index: usize) -> usize {
    let mut depth = 0_usize;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"/*") {
            depth += 1;
            index += 2;
        } else if bytes[index..].starts_with(b"*/") {
            depth = depth.saturating_sub(1);
            index += 2;
            if depth == 0 {
                return index;
            }
        } else {
            index += 1;
        }
    }
    bytes.len()
}

fn skip_raw_string(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote = start + 1;
    while bytes.get(quote) == Some(&b'#') {
        quote += 1;
    }
    if bytes.get(quote) != Some(&b'"') {
        return None;
    }
    let hashes = quote - start - 1;
    let mut index = quote + 1;
    while index < bytes.len() {
        if bytes[index] == b'"'
            && bytes.get(index + 1..index + 1 + hashes) == Some(&bytes[start + 1..quote])
        {
            return Some(index + 1 + hashes);
        }
        index += 1;
    }
    Some(bytes.len())
}

fn skip_quoted(bytes: &[u8], start: usize, quote: u8) -> usize {
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

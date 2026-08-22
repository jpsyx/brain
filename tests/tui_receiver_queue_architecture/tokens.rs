pub(super) fn rust_tokens(source: &str) -> Vec<String> {
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
            index = bytes[index + 2..]
                .windows(2)
                .position(|window| window == b"*/")
                .map_or(bytes.len(), |relative| index + relative + 4);
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
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'"' {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'\'' {
            if let Some(end) = lifetime_end(bytes, index) {
                tokens.push("'".to_owned());
                tokens.push(source[index + 1..end].to_owned());
                index = end;
            } else {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == b'\'' {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
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
        tokens.push(source[start..index].to_owned());
    }
    tokens
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

fn lifetime_end(bytes: &[u8], apostrophe: usize) -> Option<usize> {
    let mut index = apostrophe + 1;
    if !bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    index += 1;
    while bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        index += 1;
    }
    (bytes.get(index) != Some(&b'\'')).then_some(index)
}

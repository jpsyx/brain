pub(super) fn mask_rust_non_code(source: &str) -> String {
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

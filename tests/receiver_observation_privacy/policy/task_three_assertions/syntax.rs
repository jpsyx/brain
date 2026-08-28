pub(super) fn identifiers(source: &str) -> impl Iterator<Item = &str> {
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

pub(super) fn macro_bodies(masked: &str, macro_name: &str) -> Vec<std::ops::Range<usize>> {
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

pub(super) fn top_level_arguments(source: &str) -> Vec<std::ops::Range<usize>> {
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

pub(super) fn matching_delimiter(
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

pub(super) fn matching_delimiter_backwards(
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

pub(super) fn mask_non_code(source: &str) -> String {
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

pub(super) const fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

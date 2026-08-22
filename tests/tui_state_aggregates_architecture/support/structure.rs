use super::syntax::{mask_non_code, matching_brace, token_at};

pub(crate) fn directly_accesses_field(source: &str, aggregate: &str, field: &str) -> bool {
    let compact: String = mask_non_code(source)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let needle = format!(".{aggregate}.{field}");
    compact.match_indices(&needle).any(|(at, _)| {
        compact[at + needle.len()..]
            .chars()
            .next()
            .is_some_and(|next| next != '(' && !next.is_ascii_alphanumeric() && next != '_')
    })
}

pub(crate) fn extract_struct_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    extract_named_body(source, "struct", name)
}

pub(crate) fn extract_named_body<'a>(source: &'a str, kind: &str, name: &str) -> Option<&'a str> {
    let masked = mask_non_code(source);
    let mut offset = 0;
    while let Some(relative) = masked[offset..].find(kind) {
        let start = offset + relative;
        if !token_at(&masked, start, kind) {
            offset = start + kind.len();
            continue;
        }
        let mut cursor = start + kind.len();
        cursor += masked[cursor..].len() - masked[cursor..].trim_start().len();
        let end = masked[cursor..]
            .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .map_or(masked.len(), |length| cursor + length);
        if &masked[cursor..end] == name {
            let open = masked[end..].find('{').map(|relative| end + relative)?;
            let close = matching_brace(&masked, open)?;
            return Some(&source[open + 1..close]);
        }
        offset = end;
    }
    None
}

pub(crate) fn field_declaration_count(body: &str, field: &str) -> usize {
    field_declarations(body, field).len()
}

pub(crate) fn struct_field_names(body: &str) -> Vec<String> {
    let masked = mask_non_code(body);
    body.lines()
        .zip(masked.lines())
        .filter_map(|(_, code)| {
            let code = code.trim();
            if code.is_empty() || code.starts_with('#') {
                return None;
            }
            let colon = field_separator(code)?;
            code[..colon]
                .split_whitespace()
                .next_back()
                .map(str::to_owned)
        })
        .collect()
}

pub(crate) fn field_is_private(body: &str, field: &str) -> bool {
    let declarations = field_declarations(body, field);
    declarations.len() == 1 && declarations[0] == field
}

pub(crate) fn field_type(body: &str, field: &str) -> Option<String> {
    let masked = mask_non_code(body);
    body.lines()
        .zip(masked.lines())
        .find_map(|(original, code)| {
            let code = code.trim();
            let colon = field_separator(code)?;
            let left = &code[..colon];
            if left.split_whitespace().next_back() != Some(field) {
                return None;
            }
            let original = original.trim();
            let original_colon = field_separator(original)?;
            Some(
                original[original_colon + 1..]
                    .trim()
                    .trim_end_matches(',')
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect(),
            )
        })
}

pub(crate) fn field_declarations<'a>(body: &'a str, field: &str) -> Vec<&'a str> {
    let masked = mask_non_code(body);
    body.lines()
        .zip(masked.lines())
        .filter_map(|(original, code)| {
            let code = code.trim();
            if code.is_empty() || code.starts_with('#') {
                return None;
            }
            let colon = field_separator(code)?;
            let left = &code[..colon];
            (left.split_whitespace().next_back() == Some(field))
                .then(|| original.trim()[..colon].trim())
        })
        .collect()
}

pub(crate) fn field_separator(code: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut paren_depth = 0_usize;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b':' if paren_depth == 0
                && bytes.get(index.wrapping_sub(1)) != Some(&b':')
                && bytes.get(index + 1) != Some(&b':') =>
            {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

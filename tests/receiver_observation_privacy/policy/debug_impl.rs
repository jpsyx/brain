pub(super) fn item_automatically_derives_debug(source: &str, type_name: &str) -> bool {
    let struct_marker = format!("struct {type_name}");
    let enum_marker = format!("enum {type_name}");
    let item_index = [struct_marker, enum_marker]
        .iter()
        .flat_map(|marker| source.match_indices(marker))
        .filter(|(index, marker)| {
            source[index + marker.len()..]
                .chars()
                .next()
                .is_some_and(|character| {
                    character.is_whitespace() || matches!(character, '(' | '<' | '{')
                })
        })
        .map(|(index, _)| index)
        .min()
        .expect("receiver content-bearing type");
    let prefix = &source[..item_index];
    let Some(derive_start) = prefix.rfind("#[derive") else {
        return false;
    };
    let after_derive = &prefix[derive_start..];
    if after_derive.contains("struct ") || after_derive.contains("enum ") {
        return false;
    }
    let Some(derive_end) = matching_delimiter(after_derive, 1, b'[', b']') else {
        return false;
    };
    identifier_present(&after_derive[..=derive_end], "Debug")
}

pub(super) fn manual_debug_delegates_content(source: &str, type_name: &str) -> bool {
    let markers = [
        format!("impl std::fmt::Debug for {type_name}"),
        format!("impl Debug for {type_name}"),
    ];
    let Some((implementation_start, marker)) = markers
        .iter()
        .filter_map(|marker| source.find(marker).map(|index| (index, marker)))
        .min_by_key(|(index, _)| *index)
    else {
        return false;
    };
    let implementation = &source[implementation_start + marker.len()..];
    let Some(body_start) = implementation.find('{') else {
        return true;
    };
    let Some(body_end) = matching_delimiter(implementation, body_start, b'{', b'}') else {
        return true;
    };
    let body = &implementation[body_start..=body_end];
    [
        ".field(",
        ".debug_struct(",
        ".debug_tuple(",
        ".debug_list(",
        ".debug_map(",
        ".debug_set(",
        "format_args!(",
        "write!(",
        "writeln!(",
    ]
    .into_iter()
    .any(|pattern| body.contains(pattern))
}

fn matching_delimiter(
    source: &str,
    opening_index: usize,
    opening: u8,
    closing: u8,
) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(opening_index) != Some(&opening) {
        return None;
    }
    let mut depth = 0_usize;
    for (index, byte) in bytes.iter().copied().enumerate().skip(opening_index) {
        if byte == opening {
            depth += 1;
        } else if byte == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
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

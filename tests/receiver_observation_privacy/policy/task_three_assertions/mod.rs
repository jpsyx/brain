use std::collections::BTreeSet;

mod syntax;
mod taint;

use syntax::{
    identifiers, is_identifier_character, macro_bodies, mask_non_code, matching_delimiter,
    matching_delimiter_backwards, top_level_arguments,
};

pub(super) fn private_whole_value_assertion_violations(source: &str) -> usize {
    let masked = mask_non_code(source);
    let mut violations = 0;
    for macro_name in [
        "assert_eq!",
        "assert_ne!",
        "debug_assert_eq!",
        "debug_assert_ne!",
    ] {
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
    for macro_name in ["assert!", "debug_assert!"] {
        for body in macro_bodies(&masked, macro_name) {
            let private = private_identifiers_at(&masked, body.start);
            let arguments = top_level_arguments(&masked[body.clone()]);
            if diagnostics_expose_private(source, &masked, body, &arguments[1..], &private) {
                violations += 1;
            }
        }
    }
    for macro_name in ["dbg!", "format_args!"] {
        for body in macro_bodies(&masked, macro_name) {
            let private = private_identifiers_at(&masked, body.start);
            let arguments = top_level_arguments(&masked[body.clone()]);
            if diagnostics_expose_private(source, &masked, body, &arguments, &private) {
                violations += 1;
            }
        }
    }
    violations
}

fn private_identifiers_at(masked: &str, offset: usize) -> BTreeSet<String> {
    let private = identifiers(masked)
        .filter(|identifier| identifier_is_private(identifier))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let scope_start = enclosing_function_body_start(masked, offset).unwrap_or(0);
    let scope = &masked[scope_start..offset];
    let analysis_scope = scope_before_open_macro(scope);
    let mut private = taint::inferred_private_identifiers(analysis_scope, &private);
    loop {
        let before = private.len();
        for statement in analysis_scope.split(';') {
            let Some((left, right)) = assignment(statement) else {
                continue;
            };
            if !expression_exposes_private(right, &private) {
                continue;
            }
            for alias in assignment_aliases(left) {
                private.insert(alias.to_owned());
            }
        }
        taint::propagate_control_flow_aliases(analysis_scope, &mut private);
        if private.len() == before {
            return private;
        }
    }
}

fn scope_before_open_macro(scope: &str) -> &str {
    let trimmed = scope.trim_end();
    let Some(opening) = trimmed.chars().next_back() else {
        return scope;
    };
    if !matches!(opening, '(' | '[' | '{') {
        return scope;
    }
    let before_opening = trimmed[..trimmed.len() - opening.len_utf8()].trim_end();
    let Some(bang) = before_opening.strip_suffix('!') else {
        return scope;
    };
    let macro_start = bang
        .char_indices()
        .rev()
        .find(|(_, character)| !is_identifier_character(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    &bang[..macro_start]
}

fn assignment_aliases(left: &str) -> Vec<&str> {
    let left = left.trim();
    let is_binding = identifiers(left).next() == Some("let");
    if is_binding {
        return identifiers(left)
            .filter(|identifier| {
                !matches!(*identifier, "let" | "mut" | "ref")
                    && identifier
                        .as_bytes()
                        .first()
                        .is_some_and(|byte| byte.is_ascii_lowercase() || *byte == b'_')
            })
            .collect();
    }
    if left.chars().all(|character| {
        is_identifier_character(character)
            || character.is_whitespace()
            || matches!(character, '(' | ')' | ',')
    }) {
        return identifiers(left).collect();
    }
    Vec::new()
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
    let content_free = components.last().is_some_and(|component| {
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
            b'=' if delimiters.is_empty() && bytes.get(index + 1) == Some(&b'=') => return true,
            b'!' | b'<' | b'>' if delimiters.is_empty() && bytes.get(index + 1) == Some(&b'=') => {
                return true;
            }
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
            | "state"
            | "status"
            | "kind"
            | "category"
            | "error_category"
            | "response_kind"
            | "has_provider_reference"
    ) || method.ends_with("_count")
        || method.ends_with("_len")
        || method.ends_with("_length")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_assignment_alias_is_private_at_assertion() {
        let source = "let alias; if condition { alias = sender; } assert_eq!(alias, expected);";
        let masked = mask_non_code(source);
        let offset = masked.find("assert_eq!").expect("assertion offset");
        let private = private_identifiers_at(&masked, offset);

        assert!(private.contains("alias"), "nested alias was not private");
        assert!(
            private_whole_value_assertion_violations(source) > 0,
            "nested assertion was not rejected"
        );
    }
}

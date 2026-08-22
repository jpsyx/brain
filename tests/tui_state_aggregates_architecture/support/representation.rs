use super::structure::extract_named_body;
use super::syntax::{
    code_tokens, impl_ranges, is_identifier, mask_non_code, method_ranges, token_at,
};

pub(crate) fn representation_like_return(source: &str, returned: &str) -> bool {
    let aliases = representation_type_aliases(source);
    type_exposes_representation(&code_tokens(returned), &aliases)
}

pub(crate) fn representation_type_aliases(source: &str) -> Vec<&str> {
    let tokens = code_tokens(source);
    let declarations = type_alias_declarations(&tokens);
    let mut aliases = Vec::new();
    loop {
        let mut added = false;
        for (name, value) in &declarations {
            if !aliases.contains(name) && type_exposes_representation(value, &aliases) {
                aliases.push(*name);
                added = true;
            }
        }
        if !added {
            return aliases;
        }
    }
}

pub(crate) fn type_alias_declarations<'tokens, 'source>(
    tokens: &'tokens [&'source str],
) -> Vec<(&'source str, &'tokens [&'source str])> {
    let mut declarations = Vec::new();
    let mut cursor = 0_usize;
    while cursor < tokens.len() {
        if tokens[cursor] != "type" || tokens.get(cursor.wrapping_sub(1)) == Some(&".") {
            cursor += 1;
            continue;
        }
        let Some(name) = tokens
            .get(cursor + 1)
            .copied()
            .filter(|name| is_identifier(name))
        else {
            cursor += 1;
            continue;
        };
        let end = tokens[cursor + 2..]
            .iter()
            .position(|token| *token == ";")
            .map_or(tokens.len(), |relative| cursor + 2 + relative);
        let Some(equals) = tokens[cursor + 2..end]
            .iter()
            .position(|token| *token == "=")
            .map(|relative| cursor + 2 + relative)
        else {
            cursor = end.saturating_add(1);
            continue;
        };
        declarations.push((name, &tokens[equals + 1..end]));
        cursor = end.saturating_add(1);
    }
    declarations
}

pub(crate) fn type_exposes_representation(tokens: &[&str], aliases: &[&str]) -> bool {
    tokens.iter().any(|token| {
        *token == "&"
            || aliases.contains(token)
            || matches!(
                *token,
                "Task"
                    | "Habit"
                    | "Line"
                    | "TasksRenderState"
                    | "TaskRowsSnapshot"
                    | "TaskTriageSnapshot"
            )
    })
}

pub(crate) fn aggregate_aliases<'a>(tokens: &[&'a str], aggregate: &str) -> Vec<&'a str> {
    let mut aliases = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if *token != "let" {
            continue;
        }
        let end = tokens[index..]
            .iter()
            .position(|candidate| *candidate == ";")
            .map_or(tokens.len(), |relative| index + relative);
        if let Some(alias) = aggregate_alias_from_let(&tokens[index..end], aggregate) {
            aliases.push(alias);
        }
    }
    aliases
}

pub(crate) fn aggregate_alias_from_let<'a>(
    statement: &[&'a str],
    aggregate: &str,
) -> Option<&'a str> {
    if statement.first() != Some(&"let") {
        return None;
    }
    let mut alias_index = 1;
    if statement.get(alias_index) == Some(&"mut") {
        alias_index += 1;
    }
    let alias = *statement.get(alias_index)?;
    if !is_identifier(alias) {
        return None;
    }
    let equals = statement.iter().position(|token| *token == "=")?;
    member_reference(&statement[equals + 1..], aggregate).then_some(alias)
}

pub(crate) fn tainted_alias_from_let<'a>(
    statement: &[&'a str],
    aggregate: &str,
    aliases: &[&str],
) -> Option<&'a str> {
    if statement.first() != Some(&"let") {
        return None;
    }
    let mut alias_index = 1;
    if statement.get(alias_index) == Some(&"mut") {
        alias_index += 1;
    }
    let alias = *statement.get(alias_index)?;
    if !is_identifier(alias) {
        return None;
    }
    let equals = statement.iter().position(|token| *token == "=")?;
    forwarded_expression(&statement[equals + 1..], aggregate, aliases).then_some(alias)
}

pub(crate) fn member_reference(tokens: &[&str], aggregate: &str) -> bool {
    tokens.windows(2).any(|window| window == [".", aggregate])
}

pub(crate) fn top_level_statements<'a>(tokens: &'a [&'a str]) -> Vec<&'a [&'a str]> {
    let mut statements = Vec::new();
    let mut start = 0_usize;
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match *token {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            ";" if depth == 0 => {
                if start < index {
                    statements.push(&tokens[start..index]);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < tokens.len() {
        statements.push(&tokens[start..]);
    }
    statements
}

pub(crate) fn forwarded_expression(tokens: &[&str], aggregate: &str, aliases: &[&str]) -> bool {
    let mut start = 0_usize;
    if tokens.get(start) == Some(&"return") {
        start += 1;
    }
    while matches!(tokens.get(start), Some(&"(" | &"&" | &"mut")) {
        start += 1;
    }
    (tokens.get(start) == Some(&"self")
        && tokens.get(start + 1) == Some(&".")
        && tokens.get(start + 2) == Some(&aggregate))
        || tokens
            .get(start)
            .is_some_and(|candidate| aliases.contains(candidate))
}

pub(crate) fn function_signature<'a>(source: &'a str, name: &str) -> &'a str {
    let masked = mask_non_code(source);
    for (start, _) in masked.match_indices("fn") {
        if !token_at(&masked, start, "fn") {
            continue;
        }
        let after_fn = start + 2;
        let rest = masked[after_fn..].trim_start();
        let name_start = after_fn + (masked[after_fn..].len() - rest.len());
        let Some(after_name) = rest.strip_prefix(name) else {
            continue;
        };
        if !after_name
            .chars()
            .next()
            .is_some_and(|character| character == '(' || character == '<')
        {
            continue;
        }
        let end = masked[name_start + name.len()..]
            .find('{')
            .map(|relative| name_start + name.len() + relative)
            .expect("function body");
        return &source[start..end];
    }
    panic!("function declaration: {name}")
}

pub(crate) fn compact_signature(signature: &str) -> String {
    let mut compact = signature
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    while compact.contains(",)") {
        compact = compact.replace(",)", ")");
    }
    compact
        .strip_prefix("pub(crate)")
        .unwrap_or(&compact)
        .to_owned()
}

pub(crate) fn compact_tokens(source: &str) -> String {
    code_tokens(source).concat()
}

pub(crate) fn has_exact_named_shape(source: &str, kind: &str, name: &str, expected: &str) -> bool {
    extract_named_body(source, kind, name).is_some_and(|body| compact_tokens(body) == expected)
}

pub(crate) fn expected_links_plan_shape() -> String {
    [
        "None,Open",
        "{",
        "url:String",
        "},Choose",
        "{",
        "task_id:String,links:Vec<Link>",
        "},",
    ]
    .concat()
}

pub(crate) fn public_impl_method_names<'a>(source: &'a str, type_name: &str) -> Vec<&'a str> {
    public_impl_method_signatures_with_names(source, type_name)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

pub(crate) fn public_impl_method_signatures(source: &str, type_name: &str) -> Vec<String> {
    public_impl_method_signatures_with_names(source, type_name)
        .into_iter()
        .map(|(_, signature)| compact_signature(signature))
        .collect()
}

pub(crate) fn public_impl_method_signatures_with_names<'a>(
    source: &'a str,
    type_name: &str,
) -> Vec<(&'a str, &'a str)> {
    let masked = mask_non_code(source);
    let mut methods = Vec::new();
    for (impl_open, impl_close) in impl_ranges(&masked, type_name) {
        let mut declaration_start = impl_open + 1;
        for (fn_start, body_open, body_close) in method_ranges(&masked, impl_open, impl_close) {
            let visibility: String = masked[declaration_start..fn_start]
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            declaration_start = body_close + 1;
            if !visibility.contains("pub(crate)") {
                continue;
            }
            let after_fn = &masked[fn_start + 2..body_open];
            let trimmed = after_fn.trim_start();
            let name_len = trimmed
                .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .expect("method name terminator");
            let name_offset = source[fn_start + 2..body_open].find(trimmed).unwrap();
            let name_start = fn_start + 2 + name_offset;
            methods.push((
                &source[name_start..name_start + name_len],
                &source[fn_start..body_open],
            ));
        }
    }
    methods
}

pub(crate) fn public_state_type_names(source: &str) -> Vec<&str> {
    let tokens = code_tokens(source);
    let mut names = Vec::new();
    for window in tokens.windows(6) {
        if window[0] == "pub"
            && window[1] == "("
            && window[2] == "crate"
            && window[3] == ")"
            && matches!(window[4], "struct" | "enum")
        {
            names.push(window[5]);
        }
    }
    names
}

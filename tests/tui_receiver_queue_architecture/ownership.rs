use std::collections::HashSet;
use std::path::Path;

use super::tokens::rust_tokens;

pub(super) fn queue_boundary_violations(source: &str) -> Vec<&'static str> {
    queue_boundary_violations_at(Path::new("src/tui/unrelated.rs"), source)
}

pub(super) fn queue_boundary_violations_at(path: &Path, source: &str) -> Vec<&'static str> {
    let tokens = rust_tokens(source);
    let owns_receiver_effect_boundary = path.ends_with("src/tui/receiver/effect.rs");
    if declares_raw_inbound_job_storage(&tokens, owns_receiver_effect_boundary) {
        vec!["persistent item owns raw InboundJob storage"]
    } else {
        Vec::new()
    }
}

fn declares_raw_inbound_job_storage(
    tokens: &[String],
    owns_receiver_effect_boundary: bool,
) -> bool {
    let (job_aliases, storage_aliases) = classify_job_aliases(tokens);
    if !storage_aliases.is_empty()
        || declares_persistent_item_storage(tokens, &job_aliases, &storage_aliases)
    {
        return true;
    }

    tokens.iter().enumerate().any(|(index, token)| {
        if !matches!(token.as_str(), "struct" | "enum" | "union") {
            return false;
        }
        let Some(open) = tokens[index + 2..]
            .iter()
            .position(|candidate| matches!(candidate.as_str(), "{" | "(" | ";"))
            .map(|relative| index + 2 + relative)
        else {
            return false;
        };
        if tokens[open] == ";" {
            return false;
        }
        let closing = if tokens[open] == "{" { "}" } else { ")" };
        matching_index(tokens, open, &tokens[open], closing).is_some_and(|close| {
            let body = &tokens[open + 1..close];
            if !contains_job_reference(body, &job_aliases, &storage_aliases) {
                return false;
            }
            let declaration_name = tokens.get(index + 1).map(String::as_str);
            !(owns_receiver_effect_boundary
                && declaration_name == Some("ReceiverEffect")
                && receiver_effect_payloads_are_one_shot(body, &job_aliases))
        })
    })
}

fn declares_persistent_item_storage(
    tokens: &[String],
    job_aliases: &HashSet<String>,
    storage_aliases: &HashSet<String>,
) -> bool {
    tokens.iter().enumerate().any(|(index, token)| {
        let mut name = index + 1;
        if token == "static"
            && index
                .checked_sub(1)
                .and_then(|previous| tokens.get(previous))
                .map(String::as_str)
                != Some("'")
        {
            if tokens.get(name).map(String::as_str) == Some("mut") {
                name += 1;
            }
        } else if token != "const" {
            return false;
        }
        if !tokens.get(name).is_some_and(|token| is_identifier(token))
            || tokens.get(name + 1).map(String::as_str) != Some(":")
        {
            return false;
        }

        let type_start = name + 2;
        let type_end = top_level_type_end(tokens, type_start);
        contains_job_reference(&tokens[type_start..type_end], job_aliases, storage_aliases)
    })
}

fn top_level_type_end(tokens: &[String], start: usize) -> usize {
    let mut closings = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.as_str() {
            "(" => closings.push(")"),
            "[" => closings.push("]"),
            "{" => closings.push("}"),
            "<" => closings.push(">"),
            ")" | "]" | "}" | ">" if closings.last() == Some(&token.as_str()) => {
                closings.pop();
            }
            "=" | ";" if closings.is_empty() => return index,
            _ => {}
        }
    }
    tokens.len()
}

fn receiver_effect_payloads_are_one_shot(tokens: &[String], job_aliases: &HashSet<String>) -> bool {
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| job_aliases.contains(*token))
        .all(|(job_index, _)| {
            let wrappers = tokens
                .iter()
                .enumerate()
                .filter_map(|(open, token)| {
                    if token != "<" {
                        return None;
                    }
                    let close = matching_index(tokens, open, "<", ">")?;
                    (open < job_index && job_index < close).then(|| {
                        tokens[..open]
                            .iter()
                            .rev()
                            .find(|candidate| is_identifier(candidate))
                            .map(String::as_str)
                    })
                })
                .collect::<Option<Vec<_>>>();
            if !matches!(wrappers.as_deref(), Some(["Box"] | ["Box", "RestartPlan"])) {
                return false;
            }
            let Some((box_open, box_close)) =
                tokens.iter().enumerate().find_map(|(open, token)| {
                    if token != "<" {
                        return None;
                    }
                    let close = matching_index(tokens, open, "<", ">")?;
                    let outer = tokens[..open]
                        .iter()
                        .rev()
                        .find(|candidate| is_identifier(candidate))?;
                    (outer == "Box" && open < job_index && job_index < close)
                        .then_some((open, close))
                })
            else {
                return false;
            };
            let payload = &tokens[box_open + 1..box_close];
            !payload
                .iter()
                .any(|token| matches!(token.as_str(), "[" | "]" | "(" | ")" | "," | ";"))
                && payload
                    .iter()
                    .filter(|token| job_aliases.contains(*token))
                    .count()
                    == 1
        })
}

fn classify_job_aliases(tokens: &[String]) -> (HashSet<String>, HashSet<String>) {
    let mut job_aliases = HashSet::from(["InboundJob".to_owned()]);
    let mut storage_aliases = HashSet::new();
    loop {
        let mut changed = classify_imported_job_aliases(tokens, &mut job_aliases);
        for (index, token) in tokens.iter().enumerate() {
            if token != "type" {
                continue;
            }
            let Some(name) = tokens.get(index + 1) else {
                continue;
            };
            let Some(equals) = tokens[index + 2..]
                .iter()
                .position(|candidate| candidate == "=")
                .map(|relative| index + 2 + relative)
            else {
                continue;
            };
            let end = tokens[equals + 1..]
                .iter()
                .position(|candidate| candidate == ";")
                .map_or(tokens.len(), |relative| equals + 1 + relative);
            let target = &tokens[equals + 1..end];
            if contains_raw_job_storage(target, &job_aliases, &storage_aliases) {
                changed |= storage_aliases.insert(name.clone());
            } else if target
                .iter()
                .any(|candidate| job_aliases.contains(candidate))
            {
                changed |= job_aliases.insert(name.clone());
            }
        }
        if !changed {
            return (job_aliases, storage_aliases);
        }
    }
}

fn classify_imported_job_aliases(tokens: &[String], job_aliases: &mut HashSet<String>) -> bool {
    let known_aliases = job_aliases.clone();
    let mut changed = false;
    for alias in tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.as_str() == "use")
        .flat_map(|(use_index, _)| {
            let end = tokens[use_index + 1..]
                .iter()
                .position(|token| token == ";")
                .map_or(tokens.len(), |relative| use_index + 1 + relative);
            tokens[use_index + 1..end].windows(3)
        })
        .filter(|window| {
            known_aliases.contains(&window[0])
                && window[1] == "as"
                && window[2] != "_"
                && is_identifier(&window[2])
        })
        .map(|window| window[2].clone())
    {
        changed |= job_aliases.insert(alias);
    }
    changed
}

fn contains_raw_job_storage(
    tokens: &[String],
    job_aliases: &HashSet<String>,
    storage_aliases: &HashSet<String>,
) -> bool {
    if tokens.iter().any(|token| storage_aliases.contains(token)) {
        return true;
    }

    for (open, token) in tokens.iter().enumerate() {
        if token == "["
            && matching_index(tokens, open, "[", "]").is_some_and(|close| {
                contains_job_reference(&tokens[open + 1..close], job_aliases, storage_aliases)
            })
        {
            return true;
        }
        if token != "<" {
            continue;
        }
        let Some(close) = matching_index(tokens, open, "<", ">") else {
            continue;
        };
        let arguments = &tokens[open + 1..close];
        if !contains_job_reference(arguments, job_aliases, storage_aliases) {
            continue;
        }
        let outer = tokens[..open]
            .iter()
            .rev()
            .find(|candidate| is_identifier(candidate))
            .map(String::as_str);
        if !matches!(outer, Some("Box" | "Option" | "RestartPlan"))
            || contains_raw_job_storage(arguments, job_aliases, storage_aliases)
        {
            return true;
        }
    }
    false
}

fn contains_job_reference(
    tokens: &[String],
    job_aliases: &HashSet<String>,
    storage_aliases: &HashSet<String>,
) -> bool {
    tokens
        .iter()
        .any(|token| job_aliases.contains(token) || storage_aliases.contains(token))
}

fn matching_index(tokens: &[String], open: usize, opening: &str, closing: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token == opening {
            depth += 1;
        } else if token == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn is_identifier(token: &str) -> bool {
    token
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

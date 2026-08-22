use super::representation::{
    aggregate_aliases, compact_signature, forwarded_expression, representation_like_return,
    tainted_alias_from_let, top_level_statements,
};
use super::syntax::{code_tokens, impl_app_ranges, is_identifier, mask_non_code, method_ranges};

pub(crate) fn has_aliased_field_access(source: &str, aggregate: &str, fields: &[&str]) -> bool {
    let tokens = code_tokens(source);
    let aliases = aggregate_aliases(&tokens, aggregate);
    aliases.iter().any(|alias| {
        tokens.windows(3).enumerate().any(|(index, window)| {
            window[0] == *alias
                && window[1] == "."
                && fields.contains(&window[2])
                && tokens.get(index + 3) != Some(&"(")
        })
    })
}

pub(crate) fn has_raw_aggregate_forwarder(source: &str) -> bool {
    let masked = mask_non_code(source);
    impl_app_ranges(&masked).into_iter().any(|(open, close)| {
        method_ranges(&masked, open, close)
            .into_iter()
            .any(|(fn_start, body_open, body_close)| {
                let signature = compact_signature(&source[fn_start..body_open]);
                let Some((_, returned)) = signature.split_once("->") else {
                    return false;
                };
                if !representation_like_return(source, returned) {
                    return false;
                }
                let body = &source[body_open + 1..body_close];
                ["tasks", "shell"].into_iter().any(|aggregate| {
                    let tokens = code_tokens(body);
                    let statements = top_level_statements(&tokens);
                    let Some((last, preceding)) = statements.split_last() else {
                        return false;
                    };
                    let mut aliases = Vec::new();
                    for statement in preceding {
                        let Some(alias) = tainted_alias_from_let(statement, aggregate, &aliases)
                        else {
                            return false;
                        };
                        aliases.push(alias);
                    }
                    forwarded_expression(last, aggregate, &aliases)
                })
            })
    })
}

pub(crate) fn has_pure_direct_aggregate_forwarder(source: &str) -> bool {
    const APP_OWNERS: &[&str] = &[
        "context", "tasks", "brain", "shell", "overlay", "services", "status", "receiver",
    ];
    const GUARDED_OWNERS: &[&str] = &["context", "brain"];

    let masked = mask_non_code(source);
    impl_app_ranges(&masked).into_iter().any(|(open, close)| {
        method_ranges(&masked, open, close)
            .into_iter()
            .any(|(fn_start, body_open, body_close)| {
                let signature = code_tokens(&source[fn_start..body_open]);
                if !signature.windows(2).any(|window| window == ["&", "self"])
                    || signature
                        .windows(3)
                        .any(|window| window == ["&", "mut", "self"])
                {
                    return false;
                }
                let body_tokens = code_tokens(&source[body_open + 1..body_close]);
                let statements = top_level_statements(&body_tokens);
                let Some((expression, preceding)) = statements.split_last() else {
                    return false;
                };
                let mut aliases = Vec::new();
                for statement in preceding {
                    let Some((alias, taint)) =
                        aggregate_tainted_alias_from_let(statement, APP_OWNERS, &aliases)
                    else {
                        return false;
                    };
                    aliases.push((alias, taint));
                }
                if !simple_aggregate_forward_expression(expression, APP_OWNERS, &aliases) {
                    return false;
                }
                let expression_taint = aggregate_owner_taint(expression, APP_OWNERS, &aliases);
                expression_taint.is_power_of_two()
                    && GUARDED_OWNERS.iter().any(|owner| {
                        APP_OWNERS
                            .iter()
                            .position(|field| field == owner)
                            .and_then(owner_bit)
                            == Some(expression_taint)
                    })
            })
    })
}

pub(crate) fn aggregate_tainted_alias_from_let<'a>(
    statement: &[&'a str],
    owners: &[&str],
    aliases: &[(&str, u16)],
) -> Option<(&'a str, u16)> {
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
    let expression = &statement[equals + 1..];
    if !simple_aggregate_forward_expression(expression, owners, aliases) {
        return None;
    }
    Some((alias, aggregate_owner_taint(expression, owners, aliases)))
}

pub(crate) fn simple_aggregate_forward_expression(
    tokens: &[&str],
    owners: &[&str],
    aliases: &[(&str, u16)],
) -> bool {
    let mut start = usize::from(tokens.first() == Some(&"return"));
    while matches!(tokens.get(start), Some(&"(" | &"&" | &"mut")) {
        start += 1;
    }
    let rooted_in_owner = (tokens.get(start) == Some(&"self")
        && tokens.get(start + 1) == Some(&".")
        && tokens
            .get(start + 2)
            .is_some_and(|owner| owners.contains(owner)))
        || tokens
            .get(start)
            .is_some_and(|candidate| aliases.iter().any(|(alias, _)| alias == candidate));
    rooted_in_owner
        && tokens.iter().all(|token| {
            is_identifier(token)
                || token.chars().all(|character| character.is_ascii_digit())
                || matches!(*token, "." | "(" | ")" | "," | "&")
        })
}

pub(crate) fn aggregate_owner_taint(
    tokens: &[&str],
    owners: &[&str],
    aliases: &[(&str, u16)],
) -> u16 {
    let direct = tokens.windows(3).fold(0_u16, |taint, window| {
        if window[0] != "self" || window[1] != "." {
            return taint;
        }
        owners
            .iter()
            .position(|owner| *owner == window[2])
            .and_then(owner_bit)
            .map_or(taint, |owner| taint | owner)
    });
    tokens.iter().fold(direct, |taint, token| {
        aliases
            .iter()
            .rev()
            .find(|(alias, _)| alias == token)
            .map_or(taint, |(_, owner)| taint | owner)
    })
}

pub(crate) fn owner_bit(index: usize) -> Option<u16> {
    u32::try_from(index)
        .ok()
        .and_then(|shift| 1_u16.checked_shl(shift))
}

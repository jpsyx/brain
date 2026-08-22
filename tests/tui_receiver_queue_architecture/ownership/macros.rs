use std::collections::HashSet;

use proc_macro2::{TokenStream, TokenTree};

use super::identifiers::canonical_ident;

pub(super) fn tokens_mention_alias(tokens: &TokenStream, aliases: &HashSet<String>) -> bool {
    tokens.clone().into_iter().any(|token| match token {
        TokenTree::Ident(ident) => aliases.contains(&canonical_ident(&ident)),
        TokenTree::Group(group) => tokens_mention_alias(&group.stream(), aliases),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

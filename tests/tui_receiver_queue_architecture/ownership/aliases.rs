use std::collections::HashSet;

use syn::{ImplItem, Item, TraitItem, UseTree};

use super::cfg::{is_cfg_test, item_is_cfg_test};
use super::visitors::{
    impl_item_type_mentions_alias, item_type_mentions_alias, trait_item_type_mentions_alias,
};

pub(super) fn resolve_aliases(items: &[Item], inherited: &HashSet<String>) -> HashSet<String> {
    let items = items.iter().collect::<Vec<_>>();
    resolve_item_aliases(&items, inherited)
}

pub(super) fn resolve_item_aliases(
    items: &[&Item],
    inherited: &HashSet<String>,
) -> HashSet<String> {
    let mut aliases = inherited.clone();
    loop {
        let before = aliases.len();
        for item in items.iter().copied().filter(|item| !item_is_cfg_test(item)) {
            match item {
                Item::Type(item) if item_type_mentions_alias(item, &aliases) => {
                    aliases.insert(item.ident.to_string());
                }
                Item::Use(item) => collect_import_aliases(&item.tree, &[], &mut aliases),
                _ => {}
            }
        }
        if aliases.len() == before {
            return aliases;
        }
    }
}

pub(super) fn resolve_impl_aliases(
    items: &[ImplItem],
    inherited: &HashSet<String>,
) -> HashSet<String> {
    let mut aliases = inherited.clone();
    loop {
        let before = aliases.len();
        for item in items {
            if let ImplItem::Type(item) = item
                && !is_cfg_test(&item.attrs)
                && impl_item_type_mentions_alias(item, &aliases)
            {
                aliases.insert(item.ident.to_string());
            }
        }
        if aliases.len() == before {
            return aliases;
        }
    }
}

pub(super) fn resolve_trait_aliases(
    items: &[TraitItem],
    inherited: &HashSet<String>,
) -> HashSet<String> {
    let mut aliases = inherited.clone();
    loop {
        let before = aliases.len();
        for item in items {
            if let TraitItem::Type(item) = item
                && !is_cfg_test(&item.attrs)
                && trait_item_type_mentions_alias(item, &aliases)
            {
                aliases.insert(item.ident.to_string());
            }
        }
        if aliases.len() == before {
            return aliases;
        }
    }
}

fn collect_import_aliases(tree: &UseTree, prefix: &[String], aliases: &mut HashSet<String>) {
    match tree {
        UseTree::Path(path) => {
            let mut nested_prefix = prefix.to_vec();
            nested_prefix.push(path.ident.to_string());
            collect_import_aliases(&path.tree, &nested_prefix, aliases);
        }
        UseTree::Rename(rename) => {
            let source = if rename.ident == "self" {
                prefix.last().cloned()
            } else {
                Some(rename.ident.to_string())
            };
            if source.is_some_and(|source| aliases.contains(&source)) && rename.rename != "_" {
                aliases.insert(rename.rename.to_string());
            }
        }
        UseTree::Group(group) => {
            for nested in &group.items {
                collect_import_aliases(nested, prefix, aliases);
            }
        }
        UseTree::Name(_) | UseTree::Glob(_) => {}
    }
}

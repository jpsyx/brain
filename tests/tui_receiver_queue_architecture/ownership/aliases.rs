use std::collections::HashSet;

use syn::{ImplItem, Item, ItemUse, TraitItem, UseTree, Visibility};

use super::cfg::{is_cfg_test, item_is_cfg_test};
use super::identifiers::{canonical_ident, ident_is};
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
                    aliases.insert(canonical_ident(&item.ident));
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
                aliases.insert(canonical_ident(&item.ident));
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
                aliases.insert(canonical_ident(&item.ident));
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
            nested_prefix.push(canonical_ident(&path.ident));
            collect_import_aliases(&path.tree, &nested_prefix, aliases);
        }
        UseTree::Rename(rename) => {
            let source = if ident_is(&rename.ident, "self") {
                prefix.last().cloned()
            } else {
                Some(canonical_ident(&rename.ident))
            };
            if source.is_some_and(|source| aliases.contains(&source))
                && !ident_is(&rename.rename, "_")
            {
                aliases.insert(canonical_ident(&rename.rename));
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

pub(super) fn visible_job_reexport_renames(
    item: &ItemUse,
    aliases: &HashSet<String>,
) -> Vec<(String, String)> {
    if matches!(item.vis, Visibility::Inherited) {
        return Vec::new();
    }
    let mut renames = Vec::new();
    collect_visible_renames(&item.tree, &[], aliases, &mut renames);
    renames
}

fn collect_visible_renames(
    tree: &UseTree,
    prefix: &[String],
    aliases: &HashSet<String>,
    renames: &mut Vec<(String, String)>,
) {
    match tree {
        UseTree::Path(path) => {
            let mut nested_prefix = prefix.to_vec();
            nested_prefix.push(canonical_ident(&path.ident));
            collect_visible_renames(&path.tree, &nested_prefix, aliases, renames);
        }
        UseTree::Rename(rename) => {
            let source = if ident_is(&rename.ident, "self") {
                prefix.last().cloned()
            } else {
                Some(canonical_ident(&rename.ident))
            };
            if let Some(source) = source.filter(|source| aliases.contains(source))
                && !ident_is(&rename.rename, "_")
            {
                renames.push((source, canonical_ident(&rename.rename)));
            }
        }
        UseTree::Group(group) => {
            for nested in &group.items {
                collect_visible_renames(nested, prefix, aliases, renames);
            }
        }
        UseTree::Name(_) | UseTree::Glob(_) => {}
    }
}

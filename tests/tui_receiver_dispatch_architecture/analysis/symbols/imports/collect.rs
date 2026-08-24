use std::collections::HashMap;

#[derive(Default)]
pub(super) struct CollectedUse {
    pub(super) named: HashMap<String, Vec<String>>,
    pub(super) globs: Vec<Vec<String>>,
}

pub(super) fn collect_use_tree(
    tree: &syn::UseTree,
    prefix: Vec<String>,
    collected: &mut CollectedUse,
    module: &[String],
) {
    match tree {
        syn::UseTree::Path(path) => {
            let mut prefix = prefix;
            prefix.push(path.ident.to_string());
            collect_use_tree(&path.tree, prefix, collected, module);
        }
        syn::UseTree::Name(name) => {
            let mut path = prefix;
            path.push(name.ident.to_string());
            collected
                .named
                .insert(name.ident.to_string(), resolve_raw_path(module, &path));
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix;
            path.push(rename.ident.to_string());
            collected
                .named
                .insert(rename.rename.to_string(), resolve_raw_path(module, &path));
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree(item, prefix.clone(), collected, module);
            }
        }
        syn::UseTree::Glob(_) => collected.globs.push(resolve_raw_path(module, &prefix)),
    }
}

pub(super) fn item_declaration(item: &syn::Item) -> Option<(String, &syn::Visibility)> {
    match item {
        syn::Item::Const(item) => Some((item.ident.to_string(), &item.vis)),
        syn::Item::Enum(item) => Some((item.ident.to_string(), &item.vis)),
        syn::Item::Fn(item) => Some((item.sig.ident.to_string(), &item.vis)),
        syn::Item::Mod(item) => Some((item.ident.to_string(), &item.vis)),
        syn::Item::Static(item) => Some((item.ident.to_string(), &item.vis)),
        syn::Item::Struct(item) => Some((item.ident.to_string(), &item.vis)),
        syn::Item::Trait(item) => Some((item.ident.to_string(), &item.vis)),
        syn::Item::TraitAlias(item) => Some((item.ident.to_string(), &item.vis)),
        syn::Item::Type(item) => Some((item.ident.to_string(), &item.vis)),
        syn::Item::Union(item) => Some((item.ident.to_string(), &item.vis)),
        _ => None,
    }
}

pub(super) fn is_exported(visibility: &syn::Visibility) -> bool {
    !matches!(visibility, syn::Visibility::Inherited)
}

fn resolve_raw_path(module: &[String], raw: &[String]) -> Vec<String> {
    match raw.first().map(String::as_str) {
        Some("crate" | "std" | "core" | "alloc") => raw.to_vec(),
        Some("self") => module
            .iter()
            .cloned()
            .chain(raw.iter().skip(1).cloned())
            .collect(),
        Some("super") => resolve_super(module, raw),
        _ => module.iter().cloned().chain(raw.iter().cloned()).collect(),
    }
}

pub(super) fn resolve_super(module: &[String], raw: &[String]) -> Vec<String> {
    let mut resolved = module.to_vec();
    let mut offset = 0;
    while raw.get(offset).is_some_and(|segment| segment == "super") {
        resolved.pop();
        offset += 1;
    }
    resolved.extend(raw.iter().skip(offset).cloned());
    resolved
}

use std::collections::{HashMap, HashSet};

pub(super) fn collect_use_tree(
    tree: &syn::UseTree,
    prefix: Vec<String>,
    imports: &mut HashMap<String, Vec<String>>,
    module: &[String],
) {
    match tree {
        syn::UseTree::Path(path) => {
            let mut prefix = prefix;
            prefix.push(path.ident.to_string());
            collect_use_tree(&path.tree, prefix, imports, module);
        }
        syn::UseTree::Name(name) => {
            let mut path = prefix;
            path.push(name.ident.to_string());
            imports.insert(name.ident.to_string(), resolve_raw_path(module, &path));
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix;
            path.push(rename.ident.to_string());
            imports.insert(rename.rename.to_string(), resolve_raw_path(module, &path));
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree(item, prefix.clone(), imports, module);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

pub(super) fn expand_reexports(
    reexports: &HashMap<String, HashMap<String, Vec<String>>>,
    mut resolved: Vec<String>,
) -> Vec<String> {
    let mut expanded = HashSet::new();
    loop {
        let replacement = (1..resolved.len()).rev().find_map(|item_index| {
            let module = resolved[..item_index].join("::");
            let item = &resolved[item_index];
            if expanded.contains(&(module.clone(), item.clone())) {
                return None;
            }
            reexports
                .get(&module)
                .and_then(|imports| imports.get(item))
                .map(|target| {
                    (
                        module,
                        item.clone(),
                        target
                            .iter()
                            .cloned()
                            .chain(resolved.iter().skip(item_index + 1).cloned())
                            .collect(),
                    )
                })
        });
        let Some((module, item, replacement)) = replacement else {
            break;
        };
        expanded.insert((module, item));
        if replacement == resolved {
            break;
        }
        resolved = replacement;
    }
    resolved
}

fn resolve_raw_path(module: &[String], raw: &[String]) -> Vec<String> {
    match raw.first().map(String::as_str) {
        Some("crate" | "std" | "core" | "alloc") => raw.to_vec(),
        Some("self") => module
            .iter()
            .cloned()
            .chain(raw.iter().skip(1).cloned())
            .collect(),
        Some("super") => {
            let mut resolved = module.to_vec();
            let mut offset = 0;
            while raw.get(offset).is_some_and(|segment| segment == "super") {
                resolved.pop();
                offset += 1;
            }
            resolved.extend(raw.iter().skip(offset).cloned());
            resolved
        }
        _ => module.iter().cloned().chain(raw.iter().cloned()).collect(),
    }
}

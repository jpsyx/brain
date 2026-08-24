use std::collections::HashMap;

use super::super::TypeFact;
use crate::source::is_exact_cfg_test;

#[derive(Clone)]
pub(super) struct Scope {
    pub(super) module: Vec<String>,
    imports: HashMap<String, Vec<String>>,
    type_aliases: HashMap<String, syn::Type>,
}

impl Scope {
    pub(super) fn for_items(module: Vec<String>, items: &[syn::Item]) -> Self {
        let mut scope = Self {
            module,
            imports: HashMap::new(),
            type_aliases: HashMap::new(),
        };
        for item in items {
            if item_is_test(item) {
                continue;
            }
            match item {
                syn::Item::Use(item_use) => {
                    collect_use_tree(
                        &item_use.tree,
                        Vec::new(),
                        &mut scope.imports,
                        &scope.module,
                    );
                }
                syn::Item::Type(item_type) => {
                    scope
                        .type_aliases
                        .insert(item_type.ident.to_string(), (*item_type.ty).clone());
                }
                _ => {}
            }
        }
        scope
    }

    pub(super) fn resolve_path(&self, path: &syn::Path) -> String {
        self.resolve_segments(
            &path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
        )
        .join("::")
    }

    fn resolve_segments(&self, raw: &[String]) -> Vec<String> {
        let Some(first) = raw.first() else {
            return Vec::new();
        };
        if matches!(first.as_str(), "crate" | "std" | "core" | "alloc") {
            return raw.to_vec();
        }
        if first == "self" {
            return self
                .module
                .iter()
                .cloned()
                .chain(raw.iter().skip(1).cloned())
                .collect();
        }
        if first == "super" {
            let mut module = self.module.clone();
            let mut offset = 0;
            while raw.get(offset).is_some_and(|segment| segment == "super") {
                module.pop();
                offset += 1;
            }
            module.extend(raw.iter().skip(offset).cloned());
            return module;
        }
        if let Some(imported) = self.imports.get(first) {
            return imported
                .iter()
                .cloned()
                .chain(raw.iter().skip(1).cloned())
                .collect();
        }
        self.module
            .iter()
            .cloned()
            .chain(raw.iter().cloned())
            .collect()
    }

    pub(super) fn type_fact(&self, ty: &syn::Type) -> TypeFact {
        self.type_fact_inner(ty, &mut Vec::new())
    }

    fn type_fact_inner(&self, ty: &syn::Type, resolving: &mut Vec<String>) -> TypeFact {
        match ty {
            syn::Type::Reference(reference) => self.type_fact_inner(&reference.elem, resolving),
            syn::Type::Paren(parenthesized) => self.type_fact_inner(&parenthesized.elem, resolving),
            syn::Type::Group(group) => self.type_fact_inner(&group.elem, resolving),
            syn::Type::Path(path) => {
                if path.qself.is_none() && path.path.segments.len() == 1 {
                    let name = path.path.segments[0].ident.to_string();
                    if let Some(alias) = self.type_aliases.get(&name)
                        && !resolving.contains(&name)
                    {
                        resolving.push(name);
                        let fact = self.type_fact_inner(alias, resolving);
                        resolving.pop();
                        return fact;
                    }
                }
                let canonical = self.resolve_path(&path.path);
                let inbound_job = path.path.segments.iter().any(|segment| {
                    segment.ident == "InboundJob"
                        || generic_types(segment)
                            .into_iter()
                            .any(|ty| self.type_fact_inner(ty, resolving).inbound_job)
                });
                fact_for_canonical(canonical, inbound_job)
            }
            _ => TypeFact::default(),
        }
    }

    pub(super) fn call_owner_fact(&self, path: &syn::Path) -> TypeFact {
        if path.segments.len() < 2 {
            return TypeFact::default();
        }
        let mut owner = path.clone();
        owner.segments.pop();
        let inbound = owner.segments.iter().any(|segment| {
            segment.ident == "InboundJob"
                || generic_types(segment)
                    .into_iter()
                    .any(|ty| self.type_fact(ty).inbound_job)
        });
        fact_for_canonical(self.resolve_path(&owner), inbound)
    }

    pub(super) fn type_display(&self, ty: &syn::Type) -> String {
        self.type_fact(ty)
            .canonical
            .unwrap_or_else(|| format!("{}::<anonymous>", self.module.join("::")))
    }
}

pub(super) fn is_inbound_channel_creation(canonical: &str, path: &syn::Path) -> bool {
    matches!(
        canonical.rsplit("::").next(),
        Some("channel" | "sync_channel")
    ) && path.segments.iter().any(|segment| {
        generic_types(segment).into_iter().any(|ty| {
            matches!(ty, syn::Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "InboundJob"))
        })
    })
}

pub(super) fn item_is_test(item: &syn::Item) -> bool {
    match item {
        syn::Item::Const(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Enum(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::ExternCrate(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Fn(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::ForeignMod(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Impl(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Macro(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Mod(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Static(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Struct(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Trait(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::TraitAlias(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Type(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Union(item) => is_exact_cfg_test(&item.attrs),
        syn::Item::Use(item) => is_exact_cfg_test(&item.attrs),
        _ => false,
    }
}

fn fact_for_canonical(canonical: String, inbound_job: bool) -> TypeFact {
    let name = canonical.rsplit("::").next().unwrap_or_default().to_owned();
    let unix_listener = canonical == "std::os::unix::net::UnixListener";
    let unix_stream = canonical == "std::os::unix::net::UnixStream";
    let channel_receiver = canonical == "std::sync::mpsc::Receiver";
    let memory_queue = canonical == "std::collections::VecDeque";
    TypeFact {
        canonical: Some(canonical),
        inbound_job: inbound_job || name == "InboundJob",
        agent_controller: name == "AgentController",
        app: name == "App",
        brain_panel: name == "BrainPanelState",
        unix_listener,
        unix_stream,
        channel_receiver,
        memory_queue,
    }
}

fn generic_types(segment: &syn::PathSegment) -> Vec<&syn::Type> {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Vec::new();
    };
    arguments
        .args
        .iter()
        .filter_map(|argument| {
            let syn::GenericArgument::Type(ty) = argument else {
                return None;
            };
            Some(ty)
        })
        .collect()
}

fn collect_use_tree(
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

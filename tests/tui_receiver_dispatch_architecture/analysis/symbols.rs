use std::collections::HashMap;

use super::super::TypeFact;
use crate::source::is_exact_cfg_test;

#[path = "symbols/methods.rs"]
mod methods;

use methods::MethodIndex;

#[derive(Clone)]
struct TypeDefinition {
    module: Vec<String>,
    ty: syn::Type,
}

#[derive(Default)]
pub(super) struct Symbols {
    imports: HashMap<String, HashMap<String, Vec<String>>>,
    aliases: HashMap<String, TypeDefinition>,
    fields: HashMap<String, TypeDefinition>,
    returns: HashMap<String, TypeDefinition>,
    methods: MethodIndex,
}

impl Symbols {
    pub(super) fn collect_declarations(&mut self, items: &[syn::Item], module: &[String]) {
        self.collect_imports(items, module);
        for item in items {
            if item_is_test(item) {
                continue;
            }
            match item {
                syn::Item::Mod(item_mod) => {
                    if let Some((_, nested)) = &item_mod.content {
                        let mut child = module.to_vec();
                        child.push(item_mod.ident.to_string());
                        self.collect_declarations(nested, &child);
                    }
                }
                syn::Item::Type(item_type) => {
                    self.aliases.insert(
                        format!("{}::{}", module.join("::"), item_type.ident),
                        TypeDefinition {
                            module: module.to_vec(),
                            ty: (*item_type.ty).clone(),
                        },
                    );
                }
                _ => {}
            }
        }
    }

    pub(super) fn collect_definitions(&mut self, items: &[syn::Item], module: &[String]) {
        for item in items {
            if item_is_test(item) {
                continue;
            }
            match item {
                syn::Item::Fn(function) => {
                    self.collect_return(
                        format!("{}::{}", module.join("::"), function.sig.ident),
                        module,
                        &function.sig.output,
                    );
                }
                syn::Item::Impl(item_impl) => self.collect_impl(item_impl, module),
                syn::Item::Mod(item_mod) => {
                    if let Some((_, nested)) = &item_mod.content {
                        let mut child = module.to_vec();
                        child.push(item_mod.ident.to_string());
                        self.collect_definitions(nested, &child);
                    }
                }
                syn::Item::Struct(item_struct) => self.collect_fields(item_struct, module),
                _ => {}
            }
        }
    }

    fn collect_imports(&mut self, items: &[syn::Item], module: &[String]) {
        let imports = self.imports.entry(module.join("::")).or_default();
        for item in items {
            let syn::Item::Use(item_use) = item else {
                continue;
            };
            if !is_exact_cfg_test(&item_use.attrs) {
                collect_use_tree(&item_use.tree, Vec::new(), imports, module);
            }
        }
    }

    fn collect_impl(&mut self, item_impl: &syn::ItemImpl, module: &[String]) {
        let Some(owner) = self.type_fact(module, &item_impl.self_ty).canonical else {
            return;
        };
        let trait_name = item_impl
            .trait_
            .as_ref()
            .map(|(_, path, _)| self.resolve_path(module, path));
        for item in &item_impl.items {
            let syn::ImplItem::Fn(method) = item else {
                continue;
            };
            if !is_exact_cfg_test(&method.attrs) {
                let target = self.methods.register(
                    &owner,
                    trait_name.as_deref(),
                    &method.sig.ident.to_string(),
                );
                self.collect_return(target, module, &method.sig.output);
            }
        }
    }

    fn collect_fields(&mut self, item_struct: &syn::ItemStruct, module: &[String]) {
        let owner = format!("{}::{}", module.join("::"), item_struct.ident);
        for (index, field) in item_struct.fields.iter().enumerate() {
            let member = field
                .ident
                .as_ref()
                .map_or_else(|| index.to_string(), ToString::to_string);
            self.fields.insert(
                format!("{owner}::{member}"),
                TypeDefinition {
                    module: module.to_vec(),
                    ty: field.ty.clone(),
                },
            );
        }
    }

    fn collect_return(&mut self, id: String, module: &[String], output: &syn::ReturnType) {
        let syn::ReturnType::Type(_, ty) = output else {
            return;
        };
        self.returns.insert(
            id,
            TypeDefinition {
                module: module.to_vec(),
                ty: (**ty).clone(),
            },
        );
    }

    pub(super) fn resolve_path(&self, module: &[String], path: &syn::Path) -> String {
        self.resolve_segments(
            module,
            &path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
        )
        .join("::")
    }

    pub(super) fn qself_trait(
        &self,
        module: &[String],
        path: &syn::Path,
        position: usize,
    ) -> Option<String> {
        let segments = path
            .segments
            .iter()
            .take(position)
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        (!segments.is_empty()).then(|| self.resolve_segments(module, &segments).join("::"))
    }

    fn resolve_segments(&self, module: &[String], raw: &[String]) -> Vec<String> {
        let Some(first) = raw.first() else {
            return Vec::new();
        };
        if matches!(first.as_str(), "crate" | "std" | "core" | "alloc") {
            return raw.to_vec();
        }
        if first == "self" {
            return module
                .iter()
                .cloned()
                .chain(raw.iter().skip(1).cloned())
                .collect();
        }
        if first == "super" {
            let mut resolved = module.to_vec();
            let mut offset = 0;
            while raw.get(offset).is_some_and(|segment| segment == "super") {
                resolved.pop();
                offset += 1;
            }
            resolved.extend(raw.iter().skip(offset).cloned());
            return resolved;
        }
        if let Some(imported) = self
            .imports
            .get(&module.join("::"))
            .and_then(|imports| imports.get(first))
        {
            return imported
                .iter()
                .cloned()
                .chain(raw.iter().skip(1).cloned())
                .collect();
        }
        module.iter().cloned().chain(raw.iter().cloned()).collect()
    }

    pub(super) fn type_fact(&self, module: &[String], ty: &syn::Type) -> TypeFact {
        self.type_fact_inner(module, ty, &mut Vec::new())
    }

    fn type_fact_inner(
        &self,
        module: &[String],
        ty: &syn::Type,
        resolving: &mut Vec<String>,
    ) -> TypeFact {
        match ty {
            syn::Type::Reference(reference) => {
                self.type_fact_inner(module, &reference.elem, resolving)
            }
            syn::Type::Paren(parenthesized) => {
                self.type_fact_inner(module, &parenthesized.elem, resolving)
            }
            syn::Type::Group(group) => self.type_fact_inner(module, &group.elem, resolving),
            syn::Type::Path(path) => {
                let canonical = self.resolve_path(module, &path.path);
                if let Some(alias) = self.aliases.get(&canonical)
                    && !resolving.contains(&canonical)
                {
                    resolving.push(canonical);
                    let fact = self.type_fact_inner(&alias.module, &alias.ty, resolving);
                    resolving.pop();
                    return fact;
                }
                let inbound_job = path.path.segments.iter().any(|segment| {
                    segment.ident == "InboundJob"
                        || generic_types(segment)
                            .into_iter()
                            .any(|ty| self.type_fact_inner(module, ty, resolving).inbound_job)
                });
                fact_for_canonical(canonical, inbound_job)
            }
            _ => TypeFact::default(),
        }
    }

    pub(super) fn field_fact(&self, owner: &TypeFact, member: &syn::Member) -> TypeFact {
        let Some(owner) = &owner.canonical else {
            return TypeFact::default();
        };
        let member = match member {
            syn::Member::Named(name) => name.to_string(),
            syn::Member::Unnamed(index) => index.index.to_string(),
        };
        self.definition_fact(self.fields.get(&format!("{owner}::{member}")))
    }

    pub(super) fn return_fact(&self, target: &str) -> TypeFact {
        self.definition_fact(self.returns.get(target))
    }

    pub(super) fn method_call_target(
        &self,
        module: &[String],
        owner: &str,
        method: &str,
    ) -> Option<String> {
        let module = module.join("::");
        self.methods
            .resolve(owner, method, &module, self.imports.get(&module))
    }

    fn definition_fact(&self, definition: Option<&TypeDefinition>) -> TypeFact {
        definition.map_or_else(TypeFact::default, |definition| {
            self.type_fact(&definition.module, &definition.ty)
        })
    }
}

pub(super) fn method_target(owner: &str, trait_name: Option<&str>, method: &str) -> String {
    methods::method_target(owner, trait_name, method)
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

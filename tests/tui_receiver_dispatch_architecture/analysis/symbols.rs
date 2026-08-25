use std::collections::{HashMap, HashSet};

use crate::source::is_exact_cfg_test;

#[path = "symbols/imports.rs"]
mod imports;
#[path = "symbols/methods.rs"]
mod methods;
#[path = "symbols/types.rs"]
mod types;

use imports::ImportIndex;
pub(super) use imports::LexicalScope;
use methods::MethodIndex;

#[derive(Clone)]
struct TypeDefinition {
    module: Vec<String>,
    ty: syn::Type,
    lexical: LexicalScope,
}

#[derive(Clone)]
struct StructDefinition {
    module: Vec<String>,
    parameters: Vec<syn::GenericParam>,
    lexical: LexicalScope,
    field_count: usize,
}

#[derive(Default)]
pub(super) struct Symbols {
    imports: ImportIndex,
    aliases: HashMap<String, TypeDefinition>,
    fields: HashMap<String, TypeDefinition>,
    structs: HashMap<String, StructDefinition>,
    returns: HashMap<String, Vec<TypeDefinition>>,
    methods: MethodIndex,
    control_capabilities: HashSet<String>,
}

impl Symbols {
    pub(super) fn lexical_scope(generics: &[&syn::Generics]) -> LexicalScope {
        LexicalScope::from_generics(generics)
    }

    pub(super) fn push_block_scope(
        lexical: &mut LexicalScope,
        block: &syn::Block,
        module: &[String],
    ) {
        lexical.push_block(block, module);
    }

    pub(super) fn pop_block_scope(lexical: &mut LexicalScope) {
        lexical.pop_block();
    }

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
                            lexical: LexicalScope::from_generics(&[&item_type.generics]),
                        },
                    );
                }
                syn::Item::Struct(item_struct) => {
                    self.structs.insert(
                        format!("{}::{}", module.join("::"), item_struct.ident),
                        StructDefinition {
                            module: module.to_vec(),
                            parameters: item_struct.generics.params.iter().cloned().collect(),
                            lexical: LexicalScope::from_generics(&[&item_struct.generics]),
                            field_count: item_struct.fields.len(),
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
                        LexicalScope::from_generics(&[&function.sig.generics]),
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
        self.imports.collect(items, module);
    }

    fn collect_impl(&mut self, item_impl: &syn::ItemImpl, module: &[String]) {
        let impl_lexical = LexicalScope::from_generics(&[&item_impl.generics]);
        let Some(owner) = self
            .type_fact_scoped(module, &item_impl.self_ty, &impl_lexical)
            .sole_canonical()
            .map(str::to_owned)
        else {
            return;
        };
        let trait_name = item_impl
            .trait_
            .as_ref()
            .map(|(_, path, _)| self.resolve_path_scoped(module, path, &impl_lexical));
        for item in &item_impl.items {
            let syn::ImplItem::Fn(method) = item else {
                continue;
            };
            if !is_exact_cfg_test(&method.attrs) {
                let method_lexical =
                    LexicalScope::from_generics(&[&item_impl.generics, &method.sig.generics]);
                let target = self.methods.register(
                    &owner,
                    trait_name.as_deref(),
                    &method.sig.ident.to_string(),
                );
                if self.is_refresh_capability(
                    &owner,
                    trait_name.as_deref(),
                    method,
                    module,
                    &method_lexical,
                ) {
                    self.control_capabilities.insert(target.clone());
                }
                self.collect_return(target, module, &method.sig.output, method_lexical);
            }
        }
    }

    fn collect_fields(&mut self, item_struct: &syn::ItemStruct, module: &[String]) {
        let owner = format!("{}::{}", module.join("::"), item_struct.ident);
        let lexical = LexicalScope::from_generics(&[&item_struct.generics]);
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
                    lexical: lexical.clone(),
                },
            );
        }
    }

    fn collect_return(
        &mut self,
        id: String,
        module: &[String],
        output: &syn::ReturnType,
        lexical: LexicalScope,
    ) {
        let syn::ReturnType::Type(_, ty) = output else {
            return;
        };
        self.returns.entry(id).or_default().push(TypeDefinition {
            module: module.to_vec(),
            ty: (**ty).clone(),
            lexical,
        });
    }

    pub(super) fn resolve_path_scoped(
        &self,
        module: &[String],
        path: &syn::Path,
        lexical: &LexicalScope,
    ) -> String {
        self.resolve_segments_scoped(
            module,
            &path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
            lexical,
        )
        .join("::")
    }

    pub(super) fn qself_trait_scoped(
        &self,
        module: &[String],
        path: &syn::Path,
        position: usize,
        lexical: &LexicalScope,
    ) -> Option<String> {
        let segments = path
            .segments
            .iter()
            .take(position)
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        (!segments.is_empty()).then(|| {
            self.resolve_segments_scoped(module, &segments, lexical)
                .join("::")
        })
    }

    fn resolve_segments_scoped(
        &self,
        module: &[String],
        raw: &[String],
        lexical: &LexicalScope,
    ) -> Vec<String> {
        self.imports.resolve_scoped(module, raw, lexical)
    }

    pub(super) fn method_call_target_scoped(
        &self,
        module: &[String],
        owner: &str,
        method: &str,
        lexical: &LexicalScope,
    ) -> Option<String> {
        self.methods.resolve(owner, method, |trait_name| {
            self.imports.trait_is_visible(module, lexical, trait_name)
        })
    }

    pub(super) fn is_control_capability(&self, target: &str) -> bool {
        self.control_capabilities.contains(target)
    }

    fn is_refresh_capability(
        &self,
        owner: &str,
        trait_name: Option<&str>,
        method: &syn::ImplItemFn,
        module: &[String],
        lexical: &LexicalScope,
    ) -> bool {
        const CLIENT: &str = "crate::server::control::client::ServerClient";
        const GENERATION: &str = "crate::server::lifecycle::state::ServerGeneration";
        const WORKSPACE: &str = "crate::workspace::id::WorkspaceId";
        if owner != CLIENT
            || trait_name.is_some()
            || method.sig.ident != "refresh_enabled_generation"
        {
            return false;
        }
        let mut inputs = method.sig.inputs.iter();
        let receiver_is_shared = matches!(inputs.next(), Some(syn::FnArg::Receiver(receiver))
            if receiver.reference.is_some() && receiver.mutability.is_none());
        let typed = inputs
            .filter_map(|input| {
                let syn::FnArg::Typed(argument) = input else {
                    return None;
                };
                self.type_fact_scoped(module, &argument.ty, lexical)
                    .sole_canonical()
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        receiver_is_shared
            && typed == [GENERATION.to_owned(), WORKSPACE.to_owned()]
            && method.sig.inputs.len() == 3
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

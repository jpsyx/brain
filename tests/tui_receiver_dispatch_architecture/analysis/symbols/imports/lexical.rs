use std::collections::{HashMap, HashSet};

use super::super::item_is_test;
use super::collect::{CollectedUse, collect_use_tree};

#[derive(Clone, Default)]
struct LexicalLayer {
    declarations: HashSet<String>,
    named: HashMap<String, Vec<String>>,
    globs: Vec<Vec<String>>,
}

#[derive(Clone, Default)]
pub(crate) struct LexicalScope {
    layers: Vec<LexicalLayer>,
}

impl LexicalScope {
    pub(in super::super) fn from_generics(generics: &[&syn::Generics]) -> Self {
        let declarations = generics
            .iter()
            .flat_map(|generics| &generics.params)
            .filter_map(|parameter| {
                let syn::GenericParam::Type(parameter) = parameter else {
                    return None;
                };
                Some(parameter.ident.to_string())
            })
            .collect();
        Self {
            layers: vec![LexicalLayer {
                declarations,
                ..LexicalLayer::default()
            }],
        }
    }

    pub(in super::super) fn push_block(&mut self, block: &syn::Block, module: &[String]) {
        let mut layer = LexicalLayer::default();
        for statement in &block.stmts {
            let syn::Stmt::Item(item) = statement else {
                continue;
            };
            if item_is_test(item) {
                continue;
            }
            if let Some(declaration) = type_declaration(item) {
                layer.declarations.insert(declaration);
            }
            let syn::Item::Use(item_use) = item else {
                continue;
            };
            let mut collected = CollectedUse::default();
            collect_use_tree(&item_use.tree, Vec::new(), &mut collected, module);
            layer.named.extend(collected.named);
            layer.globs.extend(collected.globs);
        }
        self.layers.push(layer);
    }

    pub(in super::super) fn pop_block(&mut self) {
        self.layers.pop().expect("a lexical block scope is active");
    }

    pub(super) fn declaration_depth(&self, name: &str) -> Option<usize> {
        self.layers
            .iter()
            .enumerate()
            .rev()
            .find_map(|(depth, layer)| layer.declarations.contains(name).then_some(depth))
    }

    pub(super) fn named(&self, name: &str) -> Option<&Vec<String>> {
        self.layers
            .iter()
            .rev()
            .find_map(|layer| layer.named.get(name))
    }

    pub(super) fn glob_layers(&self) -> impl Iterator<Item = &[Vec<String>]> {
        self.layers
            .iter()
            .rev()
            .filter(|layer| !layer.globs.is_empty())
            .map(|layer| layer.globs.as_slice())
    }

    pub(super) fn has_globs(&self) -> bool {
        self.layers.iter().any(|layer| !layer.globs.is_empty())
    }

    pub(super) fn extend_named(&self, visible: &mut HashMap<String, Vec<String>>) {
        for layer in &self.layers {
            visible.extend(layer.named.clone());
        }
    }
}

fn type_declaration(item: &syn::Item) -> Option<String> {
    match item {
        syn::Item::Enum(item) => Some(item.ident.to_string()),
        syn::Item::Mod(item) => Some(item.ident.to_string()),
        syn::Item::Struct(item) => Some(item.ident.to_string()),
        syn::Item::Trait(item) => Some(item.ident.to_string()),
        syn::Item::TraitAlias(item) => Some(item.ident.to_string()),
        syn::Item::Type(item) => Some(item.ident.to_string()),
        syn::Item::Union(item) => Some(item.ident.to_string()),
        _ => None,
    }
}

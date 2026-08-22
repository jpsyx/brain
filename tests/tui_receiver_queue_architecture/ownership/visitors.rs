use std::collections::HashSet;

use syn::visit::{self, Visit};
use syn::{Expr, Field, Generics, ImplItemType, ItemType, TraitItemType, Type, Variant};

use super::cfg::is_cfg_test;
use super::identifiers::canonical_ident;
use super::macros::tokens_mention_alias;

pub(super) struct MentionVisitor<'aliases> {
    aliases: &'aliases HashSet<String>,
    found: bool,
}

impl<'aliases> MentionVisitor<'aliases> {
    pub(super) fn new(aliases: &'aliases HashSet<String>) -> Self {
        Self {
            aliases,
            found: false,
        }
    }

    pub(super) fn found(&self) -> bool {
        self.found
    }

    fn inspect_path(&mut self, path: &syn::Path) {
        self.found |= path
            .segments
            .iter()
            .any(|segment| self.aliases.contains(&canonical_ident(&segment.ident)));
    }
}

impl<'ast> Visit<'ast> for MentionVisitor<'_> {
    fn visit_expr_path(&mut self, item: &'ast syn::ExprPath) {
        self.inspect_path(&item.path);
        if !self.found {
            visit::visit_expr_path(self, item);
        }
    }

    fn visit_field(&mut self, field: &'ast Field) {
        if !is_cfg_test(&field.attrs) {
            visit::visit_field(self, field);
        }
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.found |= tokens_mention_alias(&item.tokens, self.aliases);
    }

    fn visit_type_path(&mut self, item: &'ast syn::TypePath) {
        self.inspect_path(&item.path);
        if !self.found {
            visit::visit_type_path(self, item);
        }
    }

    fn visit_variant(&mut self, variant: &'ast Variant) {
        if !is_cfg_test(&variant.attrs) {
            visit::visit_variant(self, variant);
        }
    }
}

pub(super) fn item_type_mentions_alias(item: &ItemType, aliases: &HashSet<String>) -> bool {
    let mut visitor = MentionVisitor::new(aliases);
    visitor.visit_item_type(item);
    visitor.found()
}

pub(super) fn impl_item_type_mentions_alias(
    item: &ImplItemType,
    aliases: &HashSet<String>,
) -> bool {
    let mut visitor = MentionVisitor::new(aliases);
    visitor.visit_impl_item_type(item);
    visitor.found()
}

pub(super) fn trait_item_type_mentions_alias(
    item: &TraitItemType,
    aliases: &HashSet<String>,
) -> bool {
    let mut visitor = MentionVisitor::new(aliases);
    visitor.visit_trait_item_type(item);
    visitor.found()
}

pub(super) fn mentions_generics(generics: &Generics, aliases: &HashSet<String>) -> bool {
    let mut visitor = MentionVisitor::new(aliases);
    visitor.visit_generics(generics);
    visitor.found()
}

pub(super) fn mentions_expr(expression: &Expr, aliases: &HashSet<String>) -> bool {
    let mut visitor = MentionVisitor::new(aliases);
    visitor.visit_expr(expression);
    visitor.found()
}

pub(super) fn mentions_type(ty: &Type, aliases: &HashSet<String>) -> bool {
    let mut visitor = MentionVisitor::new(aliases);
    visitor.visit_type(ty);
    visitor.found()
}

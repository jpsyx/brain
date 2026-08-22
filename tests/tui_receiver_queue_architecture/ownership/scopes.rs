use std::collections::HashSet;

use syn::visit::Visit;
use syn::{Block, ForeignItem, ImplItem, Item, Stmt, TraitItem};

use super::aliases::{
    resolve_aliases, resolve_impl_aliases, resolve_item_aliases, resolve_trait_aliases,
};
use super::cfg::{is_cfg_test, item_is_cfg_test};
use super::macros::tokens_mention_alias;
use super::persistent::PersistentItem;
use super::{OwnershipGuard, path_name};

impl OwnershipGuard {
    pub(super) fn inspect_scope(
        &mut self,
        items: &[Item],
        inherited_aliases: &HashSet<String>,
        top_level: bool,
    ) {
        let aliases = resolve_aliases(items, inherited_aliases);
        for item in items.iter().filter(|item| !item_is_cfg_test(item)) {
            self.inspect_item(item, &aliases, top_level);
        }
    }

    fn inspect_item(&mut self, item: &Item, aliases: &HashSet<String>, top_level: bool) {
        match item {
            Item::Const(item) => {
                self.inspect_persistent(PersistentItem::Const(item), aliases, top_level);
            }
            Item::Enum(item) => {
                self.inspect_persistent(PersistentItem::Enum(item), aliases, top_level);
            }
            Item::Fn(item) => self.inspect_block(&item.block, aliases),
            Item::ForeignMod(item) => self.inspect_foreign_items(item, aliases),
            Item::Impl(item) => self.inspect_impl_items(&item.items, aliases),
            Item::Macro(item) => self.reject_item_macro("item", &item.mac),
            Item::Mod(item) => {
                if let Some((_, nested)) = &item.content {
                    self.inspect_scope(nested, aliases, false);
                }
            }
            Item::Static(item) => {
                self.inspect_persistent(PersistentItem::Static(item), aliases, top_level);
            }
            Item::Struct(item) => {
                self.inspect_persistent(PersistentItem::Struct(item), aliases, top_level);
            }
            Item::Trait(item) => self.inspect_trait_items(&item.items, aliases),
            Item::Type(item) => {
                self.inspect_persistent(PersistentItem::Type(item), aliases, top_level);
            }
            Item::Use(item) => self.inspect_visible_job_reexport(item, aliases),
            Item::Union(item) => {
                self.inspect_persistent(PersistentItem::Union(item), aliases, top_level);
            }
            Item::Verbatim(_) => self.reject_verbatim("item"),
            _ => {}
        }
    }

    fn inspect_block(&mut self, block: &Block, inherited_aliases: &HashSet<String>) {
        let items = block
            .stmts
            .iter()
            .filter_map(|statement| match statement {
                Stmt::Item(item) => Some(item),
                Stmt::Local(_) | Stmt::Expr(_, _) | Stmt::Macro(_) => None,
            })
            .collect::<Vec<_>>();
        let aliases = resolve_item_aliases(&items, inherited_aliases);

        for statement in &block.stmts {
            match statement {
                Stmt::Item(item) if !item_is_cfg_test(item) => {
                    self.inspect_item(item, &aliases, false);
                }
                Stmt::Local(local) => {
                    if let Some(initializer) = &local.init {
                        self.inspect_nested_expr(&initializer.expr, &aliases, false);
                        if let Some((_, diverge)) = &initializer.diverge {
                            self.inspect_nested_expr(diverge, &aliases, false);
                        }
                    }
                }
                Stmt::Expr(expression, _) => {
                    self.inspect_nested_expr(expression, &aliases, true);
                }
                Stmt::Macro(item) => self.inspect_statement_macro(&item.mac, &aliases),
                Stmt::Item(_) => {}
            }
        }
    }

    fn inspect_impl_items(&mut self, items: &[ImplItem], inherited_aliases: &HashSet<String>) {
        let aliases = resolve_impl_aliases(items, inherited_aliases);
        for item in items {
            match item {
                ImplItem::Const(item) if !is_cfg_test(&item.attrs) => {
                    self.inspect_persistent(PersistentItem::ImplConst(item), &aliases, false);
                }
                ImplItem::Fn(item) if !is_cfg_test(&item.attrs) => {
                    self.inspect_block(&item.block, &aliases);
                }
                ImplItem::Type(item) if !is_cfg_test(&item.attrs) => {
                    self.inspect_persistent(PersistentItem::ImplType(item), &aliases, false);
                }
                ImplItem::Macro(item) if !is_cfg_test(&item.attrs) => {
                    self.reject_item_macro("impl item", &item.mac);
                }
                ImplItem::Verbatim(_) => self.reject_verbatim("impl item"),
                _ => {}
            }
        }
    }

    fn inspect_trait_items(&mut self, items: &[TraitItem], inherited_aliases: &HashSet<String>) {
        let aliases = resolve_trait_aliases(items, inherited_aliases);
        for item in items {
            match item {
                TraitItem::Const(item) if !is_cfg_test(&item.attrs) => {
                    self.inspect_persistent(PersistentItem::TraitConst(item), &aliases, false);
                }
                TraitItem::Fn(item) if !is_cfg_test(&item.attrs) => {
                    if let Some(block) = &item.default {
                        self.inspect_block(block, &aliases);
                    }
                }
                TraitItem::Type(item) if !is_cfg_test(&item.attrs) => {
                    self.inspect_persistent(PersistentItem::TraitType(item), &aliases, false);
                }
                TraitItem::Macro(item) if !is_cfg_test(&item.attrs) => {
                    self.reject_item_macro("trait item", &item.mac);
                }
                TraitItem::Verbatim(_) => self.reject_verbatim("trait item"),
                _ => {}
            }
        }
    }

    fn inspect_foreign_items(&mut self, item: &syn::ItemForeignMod, aliases: &HashSet<String>) {
        for foreign in &item.items {
            match foreign {
                ForeignItem::Static(item) if !is_cfg_test(&item.attrs) => {
                    self.inspect_persistent(PersistentItem::ForeignStatic(item), aliases, false);
                }
                ForeignItem::Macro(item) if !is_cfg_test(&item.attrs) => {
                    self.reject_item_macro("foreign item", &item.mac);
                }
                ForeignItem::Type(item) if !is_cfg_test(&item.attrs) => {
                    self.violations.push(format!(
                        "opaque foreign type {} can own storage",
                        item.ident
                    ));
                }
                ForeignItem::Verbatim(_) => self.reject_verbatim("foreign item"),
                _ => {}
            }
        }
    }

    fn inspect_nested_expr(
        &mut self,
        expression: &syn::Expr,
        aliases: &HashSet<String>,
        inspect_macros: bool,
    ) {
        let mut visitor = NestedSyntaxVisitor {
            guard: self,
            aliases,
            inspect_macros,
        };
        visitor.visit_expr(expression);
    }

    fn inspect_statement_macro(&mut self, item: &syn::Macro, aliases: &HashSet<String>) {
        if tokens_mention_alias(&item.tokens, aliases) {
            self.violations.push(format!(
                "statement macro {} mentions raw InboundJob",
                path_name(&item.path)
            ));
        }
    }

    fn reject_item_macro(&mut self, kind: &str, item: &syn::Macro) {
        self.violations.push(format!(
            "opaque {kind} macro {} can generate persistent storage",
            path_name(&item.path)
        ));
    }

    fn reject_verbatim(&mut self, kind: &str) {
        self.violations.push(format!(
            "opaque {kind} syntax can declare persistent storage"
        ));
    }

    pub(super) fn inspect_nested_persistent_syntax(
        &mut self,
        item: PersistentItem<'_>,
        aliases: &HashSet<String>,
    ) {
        let mut nested = NestedSyntaxVisitor {
            guard: self,
            aliases,
            inspect_macros: false,
        };
        item.visit(&mut nested);
    }
}

struct NestedSyntaxVisitor<'guard, 'aliases> {
    guard: &'guard mut OwnershipGuard,
    aliases: &'aliases HashSet<String>,
    inspect_macros: bool,
}

impl<'ast> Visit<'ast> for NestedSyntaxVisitor<'_, '_> {
    fn visit_block(&mut self, block: &'ast Block) {
        self.guard.inspect_block(block, self.aliases);
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        if self.inspect_macros {
            self.guard.inspect_statement_macro(item, self.aliases);
        }
    }
}

use std::collections::HashMap;
use std::path::Path;

use syn::visit::Visit;

#[path = "scope.rs"]
mod scope;
#[path = "symbols.rs"]
mod symbols;

use super::{
    FunctionNode, Program, RawCall, TypeFact, classify_operation, is_receiver_tick_call,
    receiver_owned_module,
};
use crate::source::{ProductionSource, is_exact_cfg_test, production_sources};
use scope::Scope;
use symbols::{LexicalScope, Symbols, item_is_test, method_target};

struct ParsedSource {
    production: ProductionSource,
    syntax: syn::File,
}

pub(super) fn collect_program(root: &Path) -> Program {
    let sources = production_sources(root)
        .into_iter()
        .map(|production| {
            let source = std::fs::read_to_string(&production.path)
                .unwrap_or_else(|error| panic!("read {}: {error}", production.path.display()));
            let syntax = syn::parse_file(&source)
                .unwrap_or_else(|error| panic!("parse {}: {error}", production.path.display()));
            ParsedSource { production, syntax }
        })
        .collect::<Vec<_>>();
    let mut symbols = Symbols::default();
    for source in &sources {
        symbols.collect_declarations(&source.syntax.items, &source.production.module);
    }
    for source in &sources {
        symbols.collect_definitions(&source.syntax.items, &source.production.module);
    }

    let mut program = Program::default();
    for source in sources {
        collect_items(
            &source.syntax.items,
            &source.production.module,
            source.production.audited_orphan,
            &symbols,
            &mut program,
        );
    }
    program
}

fn collect_items(
    items: &[syn::Item],
    module: &[String],
    audited_orphan: bool,
    symbols: &Symbols,
    program: &mut Program,
) {
    let scope = Scope::new(module.to_owned(), symbols);
    for item in items {
        if item_is_test(item) {
            continue;
        }
        match item {
            syn::Item::Fn(function) => collect_function(
                &function.sig,
                &[&function.sig.generics],
                None,
                &function.block,
                &scope,
                audited_orphan,
                program,
            ),
            syn::Item::Impl(item_impl) => {
                collect_impl(item_impl, &scope, audited_orphan, program);
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    let mut child = module.to_owned();
                    child.push(item_mod.ident.to_string());
                    collect_items(nested, &child, audited_orphan, symbols, program);
                }
            }
            _ => {}
        }
    }
}

fn collect_impl(
    item_impl: &syn::ItemImpl,
    scope: &Scope,
    audited_orphan: bool,
    program: &mut Program,
) {
    let impl_lexical = Scope::lexical_scope(&[&item_impl.generics]);
    let self_fact = scope.type_fact_scoped(&item_impl.self_ty, &impl_lexical);
    let self_type = (
        self_fact
            .canonical
            .clone()
            .unwrap_or_else(|| scope.type_display_scoped(&item_impl.self_ty, &impl_lexical)),
        self_fact,
        item_impl
            .trait_
            .as_ref()
            .map(|(_, path, _)| scope.resolve_path_scoped(path, &impl_lexical)),
    );
    for item in &item_impl.items {
        let syn::ImplItem::Fn(method) = item else {
            continue;
        };
        if is_exact_cfg_test(&method.attrs) {
            continue;
        }
        collect_function(
            &method.sig,
            &[&item_impl.generics, &method.sig.generics],
            Some(&self_type),
            &method.block,
            scope,
            audited_orphan,
            program,
        );
    }
}

fn collect_function(
    signature: &syn::Signature,
    generics: &[&syn::Generics],
    self_type: Option<&(String, TypeFact, Option<String>)>,
    block: &syn::Block,
    scope: &Scope,
    audited_orphan: bool,
    program: &mut Program,
) {
    let id = self_type.as_ref().map_or_else(
        || format!("{}::{}", scope.module.join("::"), signature.ident),
        |(owner, _, trait_name)| {
            method_target(owner, trait_name.as_deref(), &signature.ident.to_string())
        },
    );
    let lexical = Scope::lexical_scope(generics);
    let mut variables = vec![HashMap::new()];
    if let Some((_, fact, _)) = &self_type {
        variables[0].insert("self".to_owned(), fact.clone());
    }
    for input in &signature.inputs {
        if let syn::FnArg::Typed(argument) = input {
            bind_pattern(
                &argument.pat,
                scope.type_fact_scoped(&argument.ty, &lexical),
                &mut variables[0],
            );
        }
    }
    let mut visitor = BodyVisitor {
        scope,
        lexical,
        variables,
        calls: Vec::new(),
        violations: Vec::new(),
        receiver_tick_calls: 0,
    };
    visitor.visit_block(block);
    program.receiver_tick_calls += visitor.receiver_tick_calls;
    program.functions.insert(
        id.clone(),
        FunctionNode {
            id,
            receiver_owned: audited_orphan || receiver_owned_module(&scope.module),
            calls: visitor.calls,
            violations: visitor.violations,
        },
    );
}

struct BodyVisitor<'scope, 'symbols> {
    scope: &'scope Scope<'symbols>,
    lexical: LexicalScope,
    variables: Vec<HashMap<String, TypeFact>>,
    calls: Vec<RawCall>,
    violations: Vec<String>,
    receiver_tick_calls: usize,
}

impl<'ast> Visit<'ast> for BodyVisitor<'_, '_> {
    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.scope.push_block_scope(&mut self.lexical, block);
        self.variables.push(HashMap::new());
        syn::visit::visit_block(self, block);
        self.variables
            .pop()
            .expect("a variable block scope is active");
        Scope::pop_block_scope(&mut self.lexical);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        let fact = match &local.pat {
            syn::Pat::Type(pattern) => {
                Some(self.scope.type_fact_scoped(&pattern.ty, &self.lexical))
            }
            _ => local
                .init
                .as_ref()
                .map(|initialization| self.expression_fact(&initialization.expr)),
        };
        syn::visit::visit_local(self, local);
        if let Some(fact) = fact {
            bind_pattern(
                &local.pat,
                fact,
                self.variables
                    .last_mut()
                    .expect("a variable block scope is active"),
            );
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(target) = call.func.as_ref() {
            let exact_target = self.scope.call_target_scoped(target, &self.lexical);
            let name = target
                .path
                .segments
                .last()
                .expect("call path has a segment")
                .ident
                .to_string();
            let owner = self.scope.call_owner_fact_scoped(target, &self.lexical);
            if is_receiver_tick_call(&owner, &name) {
                self.receiver_tick_calls += 1;
            }
            if let Some(violation) = classify_operation(&owner, &name) {
                self.violations.push(violation.to_owned());
            } else if owner
                .canonical
                .as_deref()
                .is_some_and(|owner| owner.ends_with("::Read"))
                && let Some(stream) = call.args.first()
                && let Some(violation) = classify_operation(&self.expression_fact(stream), &name)
            {
                self.violations.push(violation.to_owned());
            }
            if exact_target.as_deref().is_some_and(|canonical| {
                self.scope.is_inbound_channel_creation_scoped(
                    canonical,
                    &target.path,
                    &self.lexical,
                )
            }) {
                self.violations
                    .push("in-memory receiver channel creation".to_owned());
            }
            self.calls.push(RawCall {
                exact_target: self.scope.receiver_reachable_target(&owner, exact_target),
            });
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        let owner = self.expression_fact(&call.receiver);
        if is_receiver_tick_call(&owner, &name) {
            self.receiver_tick_calls += 1;
        }
        if let Some(violation) = classify_operation(&owner, &name) {
            self.violations.push(violation.to_owned());
        }
        let exact_target = owner.canonical.as_ref().and_then(|owner| {
            self.scope
                .method_call_target_scoped(owner, &name, &self.lexical)
        });
        self.calls.push(RawCall {
            exact_target: self.scope.receiver_reachable_target(&owner, exact_target),
        });
        syn::visit::visit_expr_method_call(self, call);
    }
}

impl BodyVisitor<'_, '_> {
    fn expression_fact(&self, expression: &syn::Expr) -> TypeFact {
        match expression {
            syn::Expr::Path(path) if path.path.segments.len() == 1 => self
                .variables
                .iter()
                .rev()
                .find_map(|variables| variables.get(&path.path.segments[0].ident.to_string()))
                .cloned()
                .unwrap_or_default(),
            syn::Expr::Reference(reference) => self.expression_fact(&reference.expr),
            syn::Expr::Paren(parenthesized) => self.expression_fact(&parenthesized.expr),
            syn::Expr::Group(group) => self.expression_fact(&group.expr),
            syn::Expr::Field(field) => {
                let owner = self.expression_fact(&field.base);
                self.scope.field_fact(&owner, &field.member)
            }
            syn::Expr::Call(call) => {
                let syn::Expr::Path(target) = call.func.as_ref() else {
                    return TypeFact::default();
                };
                self.scope
                    .call_target_scoped(target, &self.lexical)
                    .map_or_else(TypeFact::default, |target| self.scope.return_fact(&target))
            }
            syn::Expr::MethodCall(call) => {
                let owner = self.expression_fact(&call.receiver);
                owner
                    .canonical
                    .as_ref()
                    .and_then(|owner| {
                        self.scope.method_call_target_scoped(
                            owner,
                            &call.method.to_string(),
                            &self.lexical,
                        )
                    })
                    .map_or_else(TypeFact::default, |target| self.scope.return_fact(&target))
            }
            _ => TypeFact::default(),
        }
    }
}

fn bind_pattern(pattern: &syn::Pat, fact: TypeFact, variables: &mut HashMap<String, TypeFact>) {
    match pattern {
        syn::Pat::Ident(identifier) => {
            variables.insert(identifier.ident.to_string(), fact);
        }
        syn::Pat::Type(typed) => bind_pattern(&typed.pat, fact, variables),
        syn::Pat::Reference(reference) => bind_pattern(&reference.pat, fact, variables),
        syn::Pat::Paren(parenthesized) => bind_pattern(&parenthesized.pat, fact, variables),
        syn::Pat::Tuple(tuple) => {
            for item in &tuple.elems {
                bind_pattern(item, fact.clone(), variables);
            }
        }
        _ => {}
    }
}

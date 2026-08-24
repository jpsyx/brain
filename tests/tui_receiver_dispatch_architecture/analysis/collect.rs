use std::collections::HashMap;
use std::path::Path;

use syn::visit::Visit;

#[path = "scope.rs"]
mod scope;
#[path = "symbols.rs"]
mod symbols;

use super::{FunctionNode, Program, RawCall, TypeFact, classify_operation, receiver_owned_module};
use crate::source::{ProductionSource, is_exact_cfg_test, production_sources};
use scope::Scope;
use symbols::{Symbols, item_is_test};

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
        symbols.collect_items(&source.syntax.items, &source.production.module);
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
                &function.sig.ident,
                &function.sig.inputs,
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
    let self_fact = scope.type_fact(&item_impl.self_ty);
    let self_type = (
        self_fact
            .canonical
            .clone()
            .unwrap_or_else(|| scope.type_display(&item_impl.self_ty)),
        self_fact,
    );
    for item in &item_impl.items {
        let syn::ImplItem::Fn(method) = item else {
            continue;
        };
        if is_exact_cfg_test(&method.attrs) {
            continue;
        }
        collect_function(
            &method.sig.ident,
            &method.sig.inputs,
            Some(&self_type),
            &method.block,
            scope,
            audited_orphan,
            program,
        );
    }
}

fn collect_function(
    name: &syn::Ident,
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    self_type: Option<&(String, TypeFact)>,
    block: &syn::Block,
    scope: &Scope,
    audited_orphan: bool,
    program: &mut Program,
) {
    let id = self_type.as_ref().map_or_else(
        || format!("{}::{name}", scope.module.join("::")),
        |(owner, _)| format!("{owner}::{name}"),
    );
    let mut variables = HashMap::new();
    if let Some((_, fact)) = &self_type {
        variables.insert("self".to_owned(), fact.clone());
    }
    for input in inputs {
        if let syn::FnArg::Typed(argument) = input {
            bind_pattern(&argument.pat, scope.type_fact(&argument.ty), &mut variables);
        }
    }
    let mut visitor = BodyVisitor {
        scope,
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
    variables: HashMap<String, TypeFact>,
    calls: Vec<RawCall>,
    violations: Vec<String>,
    receiver_tick_calls: usize,
}

impl<'ast> Visit<'ast> for BodyVisitor<'_, '_> {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let syn::Pat::Type(pattern) = &local.pat {
            bind_pattern(
                &pattern.pat,
                self.scope.type_fact(&pattern.ty),
                &mut self.variables,
            );
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(target) = call.func.as_ref() {
            let exact_target = self.scope.call_target(target);
            let name = target
                .path
                .segments
                .last()
                .expect("call path has a segment")
                .ident
                .to_string();
            let owner = self.scope.call_owner_fact(target);
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
                self.scope
                    .is_inbound_channel_creation(canonical, &target.path)
            }) {
                self.violations
                    .push("in-memory receiver channel creation".to_owned());
            }
            self.calls.push(RawCall { exact_target });
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        if name == "tick_receiver" {
            self.receiver_tick_calls += 1;
        }
        let owner = self.expression_fact(&call.receiver);
        if let Some(violation) = classify_operation(&owner, &name) {
            self.violations.push(violation.to_owned());
        }
        let exact_target = owner
            .canonical
            .as_ref()
            .map(|owner| format!("{owner}::{name}"));
        self.calls.push(RawCall { exact_target });
        syn::visit::visit_expr_method_call(self, call);
    }
}

impl BodyVisitor<'_, '_> {
    fn expression_fact(&self, expression: &syn::Expr) -> TypeFact {
        match expression {
            syn::Expr::Path(path) if path.path.segments.len() == 1 => self
                .variables
                .get(&path.path.segments[0].ident.to_string())
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
                    .call_target(target)
                    .map_or_else(TypeFact::default, |target| self.scope.return_fact(&target))
            }
            syn::Expr::MethodCall(call) => {
                let owner = self.expression_fact(&call.receiver);
                owner
                    .canonical
                    .as_ref()
                    .map_or_else(TypeFact::default, |owner| {
                        self.scope.return_fact(&format!("{owner}::{}", call.method))
                    })
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

use std::collections::HashMap;

use super::super::TypeFact;
use super::scope::Scope;
use super::symbols::LexicalScope;

pub(super) fn closure_parameter_fact(
    scope: &Scope<'_>,
    lexical: &LexicalScope,
    pattern: &syn::Pat,
) -> TypeFact {
    match pattern {
        syn::Pat::Type(typed) => scope.type_fact_scoped(&typed.ty, lexical),
        syn::Pat::Paren(parenthesized) => {
            closure_parameter_fact(scope, lexical, &parenthesized.pat)
        }
        syn::Pat::Reference(reference) => closure_parameter_fact(scope, lexical, &reference.pat),
        _ => TypeFact::default(),
    }
}

pub(super) fn bind_pattern(
    pattern: &syn::Pat,
    fact: TypeFact,
    variables: &mut HashMap<String, TypeFact>,
) {
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

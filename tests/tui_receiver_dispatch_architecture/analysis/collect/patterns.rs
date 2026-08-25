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
    scope: &Scope<'_>,
    pattern: &syn::Pat,
    fact: TypeFact,
    variables: &mut HashMap<String, TypeFact>,
) {
    match pattern {
        syn::Pat::Ident(identifier) => {
            merge_binding(variables, identifier.ident.to_string(), fact.clone());
            if let Some((_, subpattern)) = &identifier.subpat {
                bind_pattern(scope, subpattern, fact, variables);
            }
        }
        syn::Pat::Type(typed) => bind_pattern(scope, &typed.pat, fact, variables),
        syn::Pat::Reference(reference) => bind_pattern(scope, &reference.pat, fact, variables),
        syn::Pat::Paren(parenthesized) => {
            bind_pattern(scope, &parenthesized.pat, fact, variables);
        }
        syn::Pat::Tuple(tuple) => {
            let rest = tuple
                .elems
                .iter()
                .position(|item| matches!(item, syn::Pat::Rest(_)));
            for (index, item) in tuple.elems.iter().enumerate() {
                if matches!(item, syn::Pat::Rest(_)) {
                    continue;
                }
                let component = rest.filter(|rest| index > *rest).map_or_else(
                    || fact.tuple_component(index),
                    |_| fact.tuple_component_from_end(tuple.elems.len() - index - 1),
                );
                bind_pattern(scope, item, component, variables);
            }
        }
        syn::Pat::TupleStruct(tuple) => {
            for (index, item) in tuple.elems.iter().enumerate() {
                bind_pattern(
                    scope,
                    item,
                    scope.field_fact(&fact, &syn::Member::Unnamed(index.into())),
                    variables,
                );
            }
        }
        syn::Pat::Struct(structure) => {
            for field in &structure.fields {
                bind_pattern(
                    scope,
                    &field.pat,
                    scope.field_fact(&fact, &field.member),
                    variables,
                );
            }
        }
        syn::Pat::Slice(slice) => {
            for item in &slice.elems {
                if !matches!(item, syn::Pat::Rest(_)) {
                    bind_pattern(scope, item, fact.sequence_component(), variables);
                }
            }
        }
        syn::Pat::Or(alternatives) => {
            for alternative in &alternatives.cases {
                bind_pattern(scope, alternative, fact.clone(), variables);
            }
        }
        _ => {}
    }
}

fn merge_binding(variables: &mut HashMap<String, TypeFact>, name: String, fact: TypeFact) {
    if let Some(existing) = variables.get_mut(&name) {
        *existing = TypeFact::alternatives([existing.clone(), fact]);
    } else {
        variables.insert(name, fact);
    }
}

use std::collections::HashMap;

use super::super::super::super::TypeFact;
use super::super::{LexicalScope, StructDefinition, Symbols};

pub(super) struct AliasExpansion<'a> {
    pub(super) use_module: &'a [String],
    pub(super) definition_module: &'a [String],
    pub(super) use_scope: &'a LexicalScope,
    pub(super) definition_scope: &'a LexicalScope,
    pub(super) outer_bindings: &'a HashMap<String, TypeFact>,
}

#[derive(Eq, PartialEq)]
pub(super) enum ResolutionFrame {
    Definition(String),
    LexicalAlias {
        declaration: String,
        supplied_types: Vec<(String, TypeFact)>,
    },
}

impl Symbols {
    pub(super) fn lexical_alias_bindings(
        &self,
        parameters: &[syn::GenericParam],
        arguments: &[&syn::GenericArgument],
        resolving: &mut Vec<ResolutionFrame>,
        expansion: &AliasExpansion<'_>,
    ) -> HashMap<String, TypeFact> {
        let mut argument_index = 0;
        let mut bindings = HashMap::new();
        for parameter in parameters {
            match parameter {
                syn::GenericParam::Lifetime(_) => {
                    if matches!(
                        arguments.get(argument_index).copied(),
                        Some(syn::GenericArgument::Lifetime(_))
                    ) {
                        argument_index += 1;
                    }
                }
                syn::GenericParam::Const(_) => {
                    if arguments
                        .get(argument_index)
                        .copied()
                        .is_some_and(is_const_position_argument)
                    {
                        argument_index += 1;
                    }
                }
                syn::GenericParam::Type(parameter) => {
                    let explicit = arguments.get(argument_index).copied().and_then(|argument| {
                        let syn::GenericArgument::Type(ty) = argument else {
                            return None;
                        };
                        Some(ty)
                    });
                    if let Some(ty) = explicit {
                        argument_index += 1;
                        let fact = self.type_fact_inner(
                            expansion.use_module,
                            ty,
                            expansion.use_scope,
                            resolving,
                            expansion.outer_bindings,
                        );
                        bindings.insert(parameter.ident.to_string(), fact);
                    }
                }
            }
        }
        bindings
    }

    pub(super) fn apply_lexical_alias_defaults(
        &self,
        parameters: &[syn::GenericParam],
        bindings: &mut HashMap<String, TypeFact>,
        resolving: &mut Vec<ResolutionFrame>,
        expansion: &AliasExpansion<'_>,
    ) {
        for parameter in parameters {
            let syn::GenericParam::Type(parameter) = parameter else {
                continue;
            };
            let name = parameter.ident.to_string();
            if bindings.contains_key(&name) {
                continue;
            }
            let Some(default) = &parameter.default else {
                continue;
            };
            let fact = self.type_fact_inner(
                expansion.definition_module,
                default,
                expansion.definition_scope,
                resolving,
                bindings,
            );
            bindings.insert(name, fact);
        }
    }

    pub(super) fn struct_type_arguments(
        &self,
        definition: &StructDefinition,
        path: &syn::TypePath,
        use_module: &[String],
        use_scope: &LexicalScope,
        resolving: &mut Vec<ResolutionFrame>,
        outer_bindings: &HashMap<String, TypeFact>,
    ) -> Vec<(String, TypeFact)> {
        let arguments = path
            .path
            .segments
            .last()
            .map(generic_arguments)
            .unwrap_or_default();
        let expansion = AliasExpansion {
            use_module,
            definition_module: &definition.module,
            use_scope,
            definition_scope: &definition.lexical,
            outer_bindings,
        };
        let mut bindings =
            self.lexical_alias_bindings(&definition.parameters, &arguments, resolving, &expansion);
        self.apply_lexical_alias_defaults(
            &definition.parameters,
            &mut bindings,
            resolving,
            &expansion,
        );
        definition
            .parameters
            .iter()
            .filter_map(|parameter| {
                let syn::GenericParam::Type(parameter) = parameter else {
                    return None;
                };
                let name = parameter.ident.to_string();
                bindings.get(&name).cloned().map(|fact| (name, fact))
            })
            .collect()
    }
}

pub(super) fn lexical_alias_frame(
    declaration: &str,
    parameters: &[syn::GenericParam],
    bindings: &HashMap<String, TypeFact>,
) -> ResolutionFrame {
    let supplied_types = parameters
        .iter()
        .filter_map(|parameter| {
            let syn::GenericParam::Type(parameter) = parameter else {
                return None;
            };
            let name = parameter.ident.to_string();
            bindings.get(&name).cloned().map(|fact| (name, fact))
        })
        .collect();
    ResolutionFrame::LexicalAlias {
        declaration: declaration.to_owned(),
        supplied_types,
    }
}

pub(super) fn generic_types(segment: &syn::PathSegment) -> Vec<&syn::Type> {
    generic_arguments(segment)
        .into_iter()
        .filter_map(|argument| {
            let syn::GenericArgument::Type(ty) = argument else {
                return None;
            };
            Some(ty)
        })
        .collect()
}

pub(super) fn generic_arguments(segment: &syn::PathSegment) -> Vec<&syn::GenericArgument> {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Vec::new();
    };
    arguments.args.iter().collect()
}

fn is_const_position_argument(argument: &syn::GenericArgument) -> bool {
    match argument {
        syn::GenericArgument::Const(_) | syn::GenericArgument::Type(syn::Type::Infer(_)) => true,
        syn::GenericArgument::Type(syn::Type::Path(path)) => {
            path.qself.is_none()
                && path.path.leading_colon.is_none()
                && path.path.segments.len() == 1
                && matches!(path.path.segments[0].arguments, syn::PathArguments::None)
        }
        _ => false,
    }
}

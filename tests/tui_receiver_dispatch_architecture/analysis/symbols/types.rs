use std::collections::HashMap;

use super::super::super::TypeFact;
use super::{LexicalScope, Symbols, TypeDefinition};

struct AliasExpansion<'a> {
    module: &'a [String],
    use_scope: &'a LexicalScope,
    definition_scope: &'a LexicalScope,
    outer_bindings: &'a HashMap<String, TypeFact>,
}

#[derive(Eq, PartialEq)]
enum ResolutionFrame {
    Definition(String),
    LexicalAlias {
        declaration: String,
        supplied_types: Vec<(String, TypeFact)>,
    },
}

impl Symbols {
    pub(in super::super) fn type_fact_scoped(
        &self,
        module: &[String],
        ty: &syn::Type,
        lexical: &LexicalScope,
    ) -> TypeFact {
        self.type_fact_inner(module, ty, lexical, &mut Vec::new(), &HashMap::new())
    }

    fn type_fact_inner(
        &self,
        module: &[String],
        ty: &syn::Type,
        lexical: &LexicalScope,
        resolving: &mut Vec<ResolutionFrame>,
        type_parameters: &HashMap<String, TypeFact>,
    ) -> TypeFact {
        match ty {
            syn::Type::Reference(reference) => {
                let mut fact = self.type_fact_inner(
                    module,
                    &reference.elem,
                    lexical,
                    resolving,
                    type_parameters,
                );
                fact.borrowed = true;
                fact
            }
            syn::Type::Paren(parenthesized) => self.type_fact_inner(
                module,
                &parenthesized.elem,
                lexical,
                resolving,
                type_parameters,
            ),
            syn::Type::Group(group) => {
                self.type_fact_inner(module, &group.elem, lexical, resolving, type_parameters)
            }
            syn::Type::Path(path) => {
                if let Some(fact) = type_parameter_fact(path, type_parameters) {
                    return fact;
                }
                if let Some((key, target, parameters, definition_scope)) =
                    lexical_alias(path, lexical)
                {
                    let arguments = path
                        .path
                        .segments
                        .last()
                        .map(generic_arguments)
                        .unwrap_or_default();
                    let mut bindings = self.lexical_alias_bindings(
                        &parameters,
                        &arguments,
                        resolving,
                        &AliasExpansion {
                            module,
                            use_scope: lexical,
                            definition_scope: &definition_scope,
                            outer_bindings: type_parameters,
                        },
                    );
                    let frame = lexical_alias_frame(&key, &parameters, &bindings);
                    if resolving.contains(&frame) {
                        return fact_for_canonical(key, false);
                    }
                    resolving.push(frame);
                    self.apply_lexical_alias_defaults(
                        &parameters,
                        &mut bindings,
                        resolving,
                        &AliasExpansion {
                            module,
                            use_scope: lexical,
                            definition_scope: &definition_scope,
                            outer_bindings: type_parameters,
                        },
                    );
                    let fact = self.type_fact_inner(
                        module,
                        &target,
                        &definition_scope,
                        resolving,
                        &bindings,
                    );
                    resolving.pop();
                    return fact;
                }
                let canonical = self.resolve_path_scoped(module, &path.path, lexical);
                let frame = ResolutionFrame::Definition(canonical.clone());
                if let Some(alias) = self.aliases.get(&canonical)
                    && !resolving.contains(&frame)
                {
                    resolving.push(frame);
                    let fact = self.type_fact_inner(
                        &alias.module,
                        &alias.ty,
                        &alias.lexical,
                        resolving,
                        &HashMap::new(),
                    );
                    resolving.pop();
                    return fact;
                }
                let inbound_job = path.path.segments.iter().any(|segment| {
                    generic_types(segment).into_iter().any(|ty| {
                        self.type_fact_inner(module, ty, lexical, resolving, type_parameters)
                            .inbound_job
                    })
                });
                fact_for_canonical(canonical, inbound_job)
            }
            _ => TypeFact::default(),
        }
    }

    fn lexical_alias_bindings(
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
                            expansion.module,
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

    fn apply_lexical_alias_defaults(
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
                expansion.module,
                default,
                expansion.definition_scope,
                resolving,
                bindings,
            );
            bindings.insert(name, fact);
        }
    }

    pub(in super::super) fn field_fact(&self, owner: &TypeFact, member: &syn::Member) -> TypeFact {
        let Some(owner) = &owner.canonical else {
            return TypeFact::default();
        };
        let member = match member {
            syn::Member::Named(name) => name.to_string(),
            syn::Member::Unnamed(index) => index.index.to_string(),
        };
        self.definition_fact(self.fields.get(&format!("{owner}::{member}")))
    }

    pub(in super::super) fn return_fact(&self, target: &str) -> TypeFact {
        self.definition_fact(self.returns.get(target))
    }

    fn definition_fact(&self, definition: Option<&TypeDefinition>) -> TypeFact {
        definition.map_or_else(TypeFact::default, |definition| {
            self.type_fact_scoped(&definition.module, &definition.ty, &definition.lexical)
        })
    }
}

fn lexical_alias_frame(
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

fn type_parameter_fact(
    path: &syn::TypePath,
    type_parameters: &HashMap<String, TypeFact>,
) -> Option<TypeFact> {
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    type_parameters
        .get(&path.path.segments[0].ident.to_string())
        .cloned()
}

fn lexical_alias(
    path: &syn::TypePath,
    lexical: &LexicalScope,
) -> Option<(String, syn::Type, Vec<syn::GenericParam>, LexicalScope)> {
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    lexical.alias_definition(&path.path.segments[0].ident.to_string())
}

fn fact_for_canonical(canonical: String, inbound_job: bool) -> TypeFact {
    let unresolved_glob = canonical.starts_with("<ambiguous-glob>::");
    let inbound_job = inbound_job || canonical == "crate::server::receiver::job::InboundJob";
    let agent_controller = canonical == "crate::agent::controller::AgentController";
    let app = canonical == "crate::tui::App";
    let brain_panel = canonical == "crate::tui::state::brain::BrainPanelState";
    let server_control_client = canonical == "crate::server::control::client::ServerClient";
    let unix_listener = canonical == "std::os::unix::net::UnixListener";
    let unix_stream = canonical == "std::os::unix::net::UnixStream";
    let channel_receiver = canonical == "std::sync::mpsc::Receiver";
    let memory_queue = canonical == "std::collections::VecDeque";
    TypeFact {
        canonical: Some(canonical),
        borrowed: false,
        unresolved_glob,
        inbound_job,
        agent_controller,
        app,
        brain_panel,
        server_control_client,
        unix_listener,
        unix_stream,
        channel_receiver,
        memory_queue,
    }
}

fn generic_types(segment: &syn::PathSegment) -> Vec<&syn::Type> {
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

fn generic_arguments(segment: &syn::PathSegment) -> Vec<&syn::GenericArgument> {
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

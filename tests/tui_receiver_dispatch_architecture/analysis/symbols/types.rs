use std::collections::HashMap;

use super::super::super::TypeFact;
use super::{LexicalScope, LexicalTypeParameter, Symbols, TypeDefinition};

struct AliasExpansion<'a> {
    module: &'a [String],
    use_scope: &'a LexicalScope,
    definition_scope: &'a LexicalScope,
    outer_bindings: &'a HashMap<String, TypeFact>,
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
        resolving: &mut Vec<String>,
        type_parameters: &HashMap<String, TypeFact>,
    ) -> TypeFact {
        match ty {
            syn::Type::Reference(reference) => {
                self.type_fact_inner(module, &reference.elem, lexical, resolving, type_parameters)
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
                    if resolving.contains(&key) {
                        return fact_for_canonical(key, false);
                    }
                    resolving.push(key);
                    let arguments = path
                        .path
                        .segments
                        .last()
                        .map(generic_types)
                        .unwrap_or_default();
                    let bindings = self.lexical_alias_bindings(
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
                if let Some(alias) = self.aliases.get(&canonical)
                    && !resolving.contains(&canonical)
                {
                    resolving.push(canonical);
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
        parameters: &[LexicalTypeParameter],
        arguments: &[&syn::Type],
        resolving: &mut Vec<String>,
        expansion: &AliasExpansion<'_>,
    ) -> HashMap<String, TypeFact> {
        let mut arguments = arguments.iter();
        let mut bindings = HashMap::new();
        for parameter in parameters {
            let fact = if let Some(argument) = arguments.next() {
                Some(self.type_fact_inner(
                    expansion.module,
                    argument,
                    expansion.use_scope,
                    resolving,
                    expansion.outer_bindings,
                ))
            } else {
                parameter.default.as_ref().map(|default| {
                    self.type_fact_inner(
                        expansion.module,
                        default,
                        expansion.definition_scope,
                        resolving,
                        &bindings,
                    )
                })
            };
            if let Some(fact) = fact {
                bindings.insert(parameter.name.clone(), fact);
            }
        }
        bindings
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
) -> Option<(String, syn::Type, Vec<LexicalTypeParameter>, LexicalScope)> {
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
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Vec::new();
    };
    arguments
        .args
        .iter()
        .filter_map(|argument| {
            let syn::GenericArgument::Type(ty) = argument else {
                return None;
            };
            Some(ty)
        })
        .collect()
}

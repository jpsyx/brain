use std::collections::HashMap;

use super::super::super::TypeFact;
use super::{LexicalScope, Symbols, TypeDefinition};

#[path = "types/generics.rs"]
mod generics;

use generics::{
    AliasExpansion, ResolutionFrame, generic_arguments, generic_types, lexical_alias_frame,
};

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
            syn::Type::Reference(reference) => self
                .type_fact_inner(module, &reference.elem, lexical, resolving, type_parameters)
                .mark_borrowed(),
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
            syn::Type::Tuple(tuple) => TypeFact::tuple(
                tuple
                    .elems
                    .iter()
                    .map(|element| {
                        self.type_fact_inner(module, element, lexical, resolving, type_parameters)
                    })
                    .collect(),
            ),
            syn::Type::Array(array) => TypeFact::sequence(self.type_fact_inner(
                module,
                &array.elem,
                lexical,
                resolving,
                type_parameters,
            )),
            syn::Type::Slice(slice) => TypeFact::sequence(self.type_fact_inner(
                module,
                &slice.elem,
                lexical,
                resolving,
                type_parameters,
            )),
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
                            use_module: module,
                            definition_module: module,
                            use_scope: lexical,
                            definition_scope: &definition_scope,
                            outer_bindings: type_parameters,
                        },
                    );
                    let frame = lexical_alias_frame(&key, &parameters, &bindings);
                    if resolving.contains(&frame) {
                        return fact_for_canonical(key, false, false, false, false);
                    }
                    resolving.push(frame);
                    self.apply_lexical_alias_defaults(
                        &parameters,
                        &mut bindings,
                        resolving,
                        &AliasExpansion {
                            use_module: module,
                            definition_module: module,
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
                let generic_facts = path
                    .path
                    .segments
                    .iter()
                    .flat_map(generic_types)
                    .map(|ty| self.type_fact_inner(module, ty, lexical, resolving, type_parameters))
                    .collect::<Vec<_>>();
                let inbound_job = generic_facts
                    .iter()
                    .any(|fact| fact.any_variant(|fact| fact.inbound_job));
                let job_socket = generic_facts
                    .iter()
                    .any(|fact| fact.any_variant(|fact| fact.job_socket));
                let warm_panel_authority = generic_facts
                    .iter()
                    .any(|fact| fact.any_variant(|fact| fact.warm_panel_authority));
                let generation = path.qself.is_none()
                    && path.path.leading_colon.is_none()
                    && path.path.is_ident("u64");
                let mut fact = fact_for_canonical(
                    canonical.clone(),
                    inbound_job,
                    job_socket,
                    warm_panel_authority,
                    generation,
                );
                if let Some(definition) = self.structs.get(&canonical) {
                    fact.type_arguments = self.struct_type_arguments(
                        definition,
                        path,
                        module,
                        lexical,
                        resolving,
                        type_parameters,
                    );
                    let frame = ResolutionFrame::Definition(canonical);
                    if !resolving.contains(&frame) {
                        resolving.push(frame);
                        fact.warm_panel_authority |= self.struct_has_warm_panel_authority(
                            definition,
                            &fact.type_arguments,
                            resolving,
                        );
                        resolving.pop();
                    }
                }
                fact
            }
            _ => TypeFact::default(),
        }
    }

    pub(in super::super) fn field_fact(&self, owner: &TypeFact, member: &syn::Member) -> TypeFact {
        let member = match member {
            syn::Member::Named(name) => name.to_string(),
            syn::Member::Unnamed(index) => index.index.to_string(),
        };
        TypeFact::alternatives(owner.variants().filter_map(|owner| {
            let canonical = owner.canonical.as_ref()?;
            let definition = self.fields.get(&format!("{canonical}::{member}"));
            let bindings = owner.type_arguments.iter().cloned().collect();
            Some(definition.map_or_else(TypeFact::default, |definition| {
                let fact = self.type_fact_inner(
                    &definition.module,
                    &definition.ty,
                    &definition.lexical,
                    &mut Vec::new(),
                    &bindings,
                );
                if owner.borrowed {
                    fact.mark_borrowed()
                } else {
                    fact
                }
            }))
        }))
    }

    pub(in super::super) fn field_fact_from_end(
        &self,
        owner: &TypeFact,
        reverse_index: usize,
    ) -> TypeFact {
        TypeFact::alternatives(owner.variants().filter_map(|owner| {
            let canonical = owner.canonical.as_ref()?;
            let field_count = self.structs.get(canonical)?.field_count;
            let index = field_count.checked_sub(reverse_index + 1)?;
            Some(self.field_fact(owner, &syn::Member::Unnamed(index.into())))
        }))
    }

    pub(in super::super) fn return_fact(&self, target: &str) -> TypeFact {
        TypeFact::alternatives(
            self.returns
                .get(target)
                .into_iter()
                .flatten()
                .map(|definition| self.definition_fact(Some(definition))),
        )
    }

    fn definition_fact(&self, definition: Option<&TypeDefinition>) -> TypeFact {
        definition.map_or_else(TypeFact::default, |definition| {
            self.type_fact_scoped(&definition.module, &definition.ty, &definition.lexical)
        })
    }

    fn struct_has_warm_panel_authority(
        &self,
        definition: &super::StructDefinition,
        type_arguments: &[(String, TypeFact)],
        resolving: &mut Vec<ResolutionFrame>,
    ) -> bool {
        let bindings = type_arguments.iter().cloned().collect::<HashMap<_, _>>();
        let fields = definition
            .fields
            .iter()
            .map(|field| {
                self.type_fact_inner(
                    &field.module,
                    &field.ty,
                    &field.lexical,
                    resolving,
                    &bindings,
                )
            })
            .collect::<Vec<_>>();
        fields
            .iter()
            .any(|fact| fact.any_variant(|fact| fact.warm_panel_authority))
            || (fields
                .iter()
                .any(|fact| fact.any_variant(|fact| fact.receiver_channel))
                && fields
                    .iter()
                    .any(|fact| fact.any_variant(|fact| fact.generation))
                && fields
                    .iter()
                    .any(|fact| fact.any_variant(|fact| fact.instant)))
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

fn fact_for_canonical(
    canonical: String,
    inbound_job: bool,
    job_socket: bool,
    warm_panel_authority: bool,
    generation: bool,
) -> TypeFact {
    let unresolved_glob = canonical.starts_with("<ambiguous-glob>::");
    let inbound_job = inbound_job || canonical == "crate::server::receiver::job::InboundJob";
    let job_socket = job_socket || canonical == "crate::tui::singleton::JobSocket";
    let receiver_channel = canonical == "crate::server::receiver::job::Channel";
    let instant = canonical == "std::time::Instant";
    let agent_controller = canonical == "crate::agent::controller::AgentController";
    let app = canonical == "crate::tui::App";
    let brain_panel = canonical == "crate::tui::state::brain::BrainPanelState";
    let server_control_client = canonical == "crate::server::control::client::ServerClient";
    let unix_listener = canonical == "std::os::unix::net::UnixListener";
    let unix_stream = canonical == "std::os::unix::net::UnixStream";
    let channel_receiver = canonical == "std::sync::mpsc::Receiver";
    let memory_queue = canonical == "std::collections::VecDeque";
    let condition_variable = canonical == "std::sync::Condvar";
    TypeFact {
        canonical: Some(canonical),
        borrowed: false,
        unresolved_glob,
        inbound_job,
        job_socket,
        receiver_channel,
        instant,
        generation,
        warm_panel_authority,
        agent_controller,
        app,
        brain_panel,
        server_control_client,
        unix_listener,
        unix_stream,
        channel_receiver,
        memory_queue,
        condition_variable,
        ..TypeFact::default()
    }
}

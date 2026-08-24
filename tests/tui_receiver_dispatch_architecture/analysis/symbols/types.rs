use super::super::super::TypeFact;
use super::{LexicalScope, Symbols, TypeDefinition};

impl Symbols {
    pub(in super::super) fn type_fact_scoped(
        &self,
        module: &[String],
        ty: &syn::Type,
        lexical: &LexicalScope,
    ) -> TypeFact {
        self.type_fact_inner(module, ty, lexical, &mut Vec::new())
    }

    fn type_fact_inner(
        &self,
        module: &[String],
        ty: &syn::Type,
        lexical: &LexicalScope,
        resolving: &mut Vec<String>,
    ) -> TypeFact {
        match ty {
            syn::Type::Reference(reference) => {
                self.type_fact_inner(module, &reference.elem, lexical, resolving)
            }
            syn::Type::Paren(parenthesized) => {
                self.type_fact_inner(module, &parenthesized.elem, lexical, resolving)
            }
            syn::Type::Group(group) => {
                self.type_fact_inner(module, &group.elem, lexical, resolving)
            }
            syn::Type::Path(path) => {
                let canonical = self.resolve_path_scoped(module, &path.path, lexical);
                if let Some(alias) = self.aliases.get(&canonical)
                    && !resolving.contains(&canonical)
                {
                    resolving.push(canonical);
                    let fact =
                        self.type_fact_inner(&alias.module, &alias.ty, &alias.lexical, resolving);
                    resolving.pop();
                    return fact;
                }
                let inbound_job = path.path.segments.iter().any(|segment| {
                    generic_types(segment).into_iter().any(|ty| {
                        self.type_fact_inner(module, ty, lexical, resolving)
                            .inbound_job
                    })
                });
                fact_for_canonical(canonical, inbound_job)
            }
            _ => TypeFact::default(),
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

use super::super::TypeFact;
use super::symbols::{Symbols, method_target};

pub(super) struct Scope<'symbols> {
    pub(super) module: Vec<String>,
    symbols: &'symbols Symbols,
}

impl<'symbols> Scope<'symbols> {
    pub(super) fn new(module: Vec<String>, symbols: &'symbols Symbols) -> Self {
        Self { module, symbols }
    }

    pub(super) fn resolve_path(&self, path: &syn::Path) -> String {
        self.symbols.resolve_path(&self.module, path)
    }

    pub(super) fn call_target(&self, expression: &syn::ExprPath) -> Option<String> {
        if let Some(qself) = &expression.qself {
            let owner = self.type_fact(&qself.ty).canonical?;
            let operation = expression.path.segments.last()?.ident.to_string();
            let trait_name =
                self.symbols
                    .qself_trait(&self.module, &expression.path, qself.position);
            return Some(method_target(&owner, trait_name.as_deref(), &operation));
        }
        Some(self.resolve_path(&expression.path))
    }

    pub(super) fn type_fact(&self, ty: &syn::Type) -> TypeFact {
        self.symbols.type_fact(&self.module, ty)
    }

    pub(super) fn call_owner_fact(&self, expression: &syn::ExprPath) -> TypeFact {
        if let Some(qself) = &expression.qself {
            return self.type_fact(&qself.ty);
        }
        let path = &expression.path;
        if path.segments.len() < 2 {
            return TypeFact::default();
        }
        let mut owner = path.clone();
        owner.segments.pop();
        self.symbols.type_fact(
            &self.module,
            &syn::Type::Path(syn::TypePath {
                qself: None,
                path: owner,
            }),
        )
    }

    pub(super) fn type_display(&self, ty: &syn::Type) -> String {
        self.type_fact(ty)
            .canonical
            .unwrap_or_else(|| format!("{}::<anonymous>", self.module.join("::")))
    }

    pub(super) fn field_fact(&self, owner: &TypeFact, member: &syn::Member) -> TypeFact {
        self.symbols.field_fact(owner, member)
    }

    pub(super) fn return_fact(&self, target: &str) -> TypeFact {
        self.symbols.return_fact(target)
    }

    pub(super) fn method_call_target(&self, owner: &str, method: &str) -> Option<String> {
        self.symbols.method_call_target(&self.module, owner, method)
    }

    pub(super) fn receiver_reachable_target(
        &self,
        owner: &TypeFact,
        target: Option<String>,
    ) -> Option<String> {
        if owner.server_control_client
            && target
                .as_deref()
                .is_some_and(|target| self.symbols.is_control_capability(target))
        {
            None
        } else {
            target
        }
    }

    pub(super) fn is_inbound_channel_creation(&self, canonical: &str, path: &syn::Path) -> bool {
        matches!(
            canonical.rsplit("::").next(),
            Some("channel" | "sync_channel")
        ) && path.segments.iter().any(|segment| {
            generic_types(segment)
                .into_iter()
                .any(|ty| self.type_fact(ty).inbound_job)
        })
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

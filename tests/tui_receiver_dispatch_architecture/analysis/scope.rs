use super::super::TypeFact;
use super::symbols::{LexicalScope, Symbols, method_target};

pub(super) struct Scope<'symbols> {
    pub(super) module: Vec<String>,
    symbols: &'symbols Symbols,
}

impl<'symbols> Scope<'symbols> {
    pub(super) fn new(module: Vec<String>, symbols: &'symbols Symbols) -> Self {
        Self { module, symbols }
    }

    pub(super) fn resolve_path_scoped(&self, path: &syn::Path, lexical: &LexicalScope) -> String {
        self.symbols
            .resolve_path_scoped(&self.module, path, lexical)
    }

    pub(super) fn call_target_scoped(
        &self,
        expression: &syn::ExprPath,
        lexical: &LexicalScope,
    ) -> Option<String> {
        if let Some(qself) = &expression.qself {
            let owner = self.type_fact_scoped(&qself.ty, lexical);
            let owner = owner.sole_canonical()?;
            let operation = expression.path.segments.last()?.ident.to_string();
            let trait_name = self.symbols.qself_trait_scoped(
                &self.module,
                &expression.path,
                qself.position,
                lexical,
            );
            return Some(method_target(owner, trait_name.as_deref(), &operation));
        }
        Some(self.resolve_path_scoped(&expression.path, lexical))
    }

    pub(super) fn type_fact_scoped(&self, ty: &syn::Type, lexical: &LexicalScope) -> TypeFact {
        self.symbols.type_fact_scoped(&self.module, ty, lexical)
    }

    pub(super) fn call_owner_fact_scoped(
        &self,
        expression: &syn::ExprPath,
        lexical: &LexicalScope,
    ) -> TypeFact {
        if let Some(qself) = &expression.qself {
            return self.type_fact_scoped(&qself.ty, lexical);
        }
        let path = &expression.path;
        if path.segments.len() < 2 {
            return TypeFact::default();
        }
        let mut owner = path.clone();
        owner.segments.pop();
        self.symbols.type_fact_scoped(
            &self.module,
            &syn::Type::Path(syn::TypePath {
                qself: None,
                path: owner,
            }),
            lexical,
        )
    }

    pub(super) fn type_display_scoped(&self, ty: &syn::Type, lexical: &LexicalScope) -> String {
        self.type_fact_scoped(ty, lexical)
            .sole_canonical()
            .map_or_else(
                || format!("{}::<anonymous>", self.module.join("::")),
                str::to_owned,
            )
    }

    pub(super) fn lexical_scope(generics: &[&syn::Generics]) -> LexicalScope {
        Symbols::lexical_scope(generics)
    }

    pub(super) fn push_block_scope(&self, lexical: &mut LexicalScope, block: &syn::Block) {
        Symbols::push_block_scope(lexical, block, &self.module);
    }

    pub(super) fn pop_block_scope(lexical: &mut LexicalScope) {
        Symbols::pop_block_scope(lexical);
    }

    pub(super) fn field_fact(&self, owner: &TypeFact, member: &syn::Member) -> TypeFact {
        self.symbols.field_fact(owner, member)
    }

    pub(super) fn return_fact(&self, target: &str) -> TypeFact {
        self.symbols.return_fact(target)
    }

    pub(super) fn method_call_target_scoped(
        &self,
        owner: &str,
        method: &str,
        lexical: &LexicalScope,
    ) -> Option<String> {
        self.symbols
            .method_call_target_scoped(&self.module, owner, method, lexical)
    }

    pub(super) fn receiver_reachable_target(
        &self,
        owner: &TypeFact,
        target: Option<String>,
    ) -> Option<String> {
        if owner.all_variants(|owner| owner.server_control_client)
            && target
                .as_deref()
                .is_some_and(|target| self.symbols.is_control_capability(target))
        {
            None
        } else {
            target
        }
    }

    pub(super) fn is_inbound_channel_creation_scoped(
        &self,
        canonical: &str,
        path: &syn::Path,
        lexical: &LexicalScope,
    ) -> bool {
        matches!(
            canonical.rsplit("::").next(),
            Some("channel" | "sync_channel")
        ) && path.segments.iter().any(|segment| {
            generic_types(segment).into_iter().any(|ty| {
                self.type_fact_scoped(ty, lexical)
                    .any_variant(|fact| fact.inbound_job)
            })
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

use super::super::TypeFact;
use super::BodyVisitor;

impl BodyVisitor<'_, '_> {
    pub(super) fn expression_fact(&self, expression: &syn::Expr) -> TypeFact {
        match expression {
            syn::Expr::Path(path) if path.path.segments.len() == 1 => self
                .variables
                .iter()
                .rev()
                .find_map(|variables| variables.get(&path.path.segments[0].ident.to_string()))
                .cloned()
                .unwrap_or_default(),
            syn::Expr::Reference(reference) => {
                self.expression_fact(&reference.expr).mark_borrowed()
            }
            syn::Expr::Paren(parenthesized) => self.expression_fact(&parenthesized.expr),
            syn::Expr::Group(group) => self.expression_fact(&group.expr),
            syn::Expr::Field(field) => {
                let owner = self.expression_fact(&field.base);
                self.scope.field_fact(&owner, &field.member)
            }
            syn::Expr::Tuple(tuple) => TypeFact::tuple(
                tuple
                    .elems
                    .iter()
                    .map(|element| self.expression_fact(element))
                    .collect(),
            ),
            syn::Expr::Array(array) => TypeFact::sequence(TypeFact::alternatives(
                array
                    .elems
                    .iter()
                    .map(|element| self.expression_fact(element)),
            )),
            syn::Expr::Call(call) => {
                let syn::Expr::Path(target) = call.func.as_ref() else {
                    return TypeFact::default();
                };
                self.scope
                    .call_target_scoped(target, &self.lexical)
                    .map_or_else(TypeFact::default, |target| self.scope.return_fact(&target))
            }
            syn::Expr::MethodCall(call) => {
                let owner = self.expression_fact(&call.receiver);
                TypeFact::alternatives(owner.variants().filter_map(|owner| {
                    owner
                        .canonical
                        .as_ref()
                        .and_then(|owner| {
                            self.scope.method_call_target_scoped(
                                owner,
                                &call.method.to_string(),
                                &self.lexical,
                            )
                        })
                        .map(|target| self.scope.return_fact(&target))
                }))
            }
            _ => TypeFact::default(),
        }
    }
}

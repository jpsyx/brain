use std::collections::BTreeSet;

use syn::visit::{self, Visit};

pub(super) fn inferred_private_identifiers(
    source: &str,
    masked: &str,
    initial: &BTreeSet<String>,
) -> BTreeSet<String> {
    let Some(completed) = complete_prefix(source, masked) else {
        return initial.clone();
    };
    let parsed = syn::parse_file(&completed)
        .or_else(|_| syn::parse_file(&format!("fn privacy_fixture() {{ {completed} }}")));
    let Ok(file) = parsed else {
        return initial.clone();
    };
    let mut private = initial.clone();
    loop {
        let mut aliases = BTreeSet::new();
        AliasVisitor {
            private: &private,
            aliases: &mut aliases,
        }
        .visit_file(&file);
        let before = private.len();
        private.extend(aliases);
        if private.len() == before {
            return private;
        }
    }
}

fn complete_prefix(source: &str, masked: &str) -> Option<String> {
    if source.len() != masked.len() {
        return None;
    }
    let mut closing = Vec::new();
    for character in masked.chars() {
        let expected_close = match character {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            _ => None,
        };
        if let Some(expected_close) = expected_close {
            closing.push(expected_close);
        } else if matches!(character, ')' | ']' | '}') && closing.pop() != Some(character) {
            return None;
        }
    }
    let mut completed = String::with_capacity(source.len() + closing.len() + 2);
    completed.push_str(source);
    completed.push_str("()");
    completed.extend(closing.into_iter().rev());
    Some(completed)
}

pub(super) fn propagate_control_flow_aliases(scope: &str, private: &mut BTreeSet<String>) {
    propagate_match_aliases(scope, private);
    propagate_for_aliases(scope, private);
}

fn propagate_match_aliases(scope: &str, private: &mut BTreeSet<String>) {
    let mut cursor = 0;
    while let Some(relative) = scope[cursor..].find("match ") {
        let start = cursor + relative + "match ".len();
        let Some(open_relative) = scope[start..].find('{') else {
            break;
        };
        let open = start + open_relative;
        let Some(arrow_relative) = scope[open + 1..].find("=>") else {
            cursor = open + 1;
            continue;
        };
        let arrow = open + 1 + arrow_relative;
        if contains_private_identifier(&scope[start..open], private) {
            insert_pattern_aliases(&scope[open + 1..arrow], private);
        }
        cursor = arrow + 2;
    }
}

fn propagate_for_aliases(scope: &str, private: &mut BTreeSet<String>) {
    let mut cursor = 0;
    while let Some(relative) = scope[cursor..].find("for ") {
        let start = cursor + relative + "for ".len();
        let Some(in_relative) = scope[start..].find(" in ") else {
            break;
        };
        let in_start = start + in_relative;
        let expression_start = in_start + " in ".len();
        let Some(open_relative) = scope[expression_start..].find('{') else {
            break;
        };
        let open = expression_start + open_relative;
        if contains_private_identifier(&scope[expression_start..open], private) {
            insert_pattern_aliases(&scope[start..in_start], private);
        }
        cursor = open + 1;
    }
}

fn contains_private_identifier(source: &str, private: &BTreeSet<String>) -> bool {
    identifiers(source).any(|identifier| private.contains(identifier))
}

fn insert_pattern_aliases(pattern: &str, private: &mut BTreeSet<String>) {
    private.extend(
        identifiers(pattern)
            .filter(|identifier| {
                !matches!(*identifier, "mut" | "ref")
                    && identifier
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_lowercase)
            })
            .map(str::to_owned),
    );
}

fn identifiers(source: &str) -> impl Iterator<Item = &str> {
    source
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|identifier| !identifier.is_empty())
}

struct AliasVisitor<'a> {
    private: &'a BTreeSet<String>,
    aliases: &'a mut BTreeSet<String>,
}

impl<'ast> Visit<'ast> for AliasVisitor<'_> {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if local
            .init
            .as_ref()
            .is_some_and(|init| expression_contains_private(&init.expr, self.private))
        {
            collect_pattern_bindings(&local.pat, self.aliases);
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_assign(&mut self, assignment: &'ast syn::ExprAssign) {
        if expression_contains_private(&assignment.right, self.private) {
            collect_assigned_roots(&assignment.left, self.aliases);
        }
        visit::visit_expr_assign(self, assignment);
    }

    fn visit_expr_for_loop(&mut self, for_loop: &'ast syn::ExprForLoop) {
        if expression_contains_private(&for_loop.expr, self.private) {
            collect_pattern_bindings(&for_loop.pat, self.aliases);
        }
        visit::visit_expr_for_loop(self, for_loop);
    }

    fn visit_expr_match(&mut self, expression: &'ast syn::ExprMatch) {
        if expression_contains_private(&expression.expr, self.private) {
            for arm in &expression.arms {
                collect_pattern_bindings(&arm.pat, self.aliases);
            }
        }
        visit::visit_expr_match(self, expression);
    }
}

fn expression_contains_private(expression: &syn::Expr, private: &BTreeSet<String>) -> bool {
    if is_exact_content_proof_call(expression) {
        return false;
    }
    let mut visitor = PrivatePathVisitor {
        private,
        found: false,
    };
    visitor.visit_expr(expression);
    visitor.found
}

fn is_exact_content_proof_call(expression: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expression else {
        return false;
    };
    let syn::Expr::Path(function) = call.func.as_ref() else {
        return false;
    };
    function
        .path
        .get_ident()
        .is_some_and(|identifier| super::is_exact_content_proof_function(&identifier.to_string()))
}

struct PrivatePathVisitor<'a> {
    private: &'a BTreeSet<String>,
    found: bool,
}

impl<'ast> Visit<'ast> for PrivatePathVisitor<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path
            .segments
            .iter()
            .any(|segment| self.private.contains(&segment.ident.to_string()))
        {
            self.found = true;
        }
        visit::visit_path(self, path);
    }
}

fn collect_pattern_bindings(pattern: &syn::Pat, bindings: &mut BTreeSet<String>) {
    match pattern {
        syn::Pat::Ident(binding) => {
            bindings.insert(binding.ident.to_string());
            if let Some((_, subpattern)) = &binding.subpat {
                collect_pattern_bindings(subpattern, bindings);
            }
        }
        syn::Pat::Or(pattern) => {
            for case in &pattern.cases {
                collect_pattern_bindings(case, bindings);
            }
        }
        syn::Pat::Paren(pattern) => collect_pattern_bindings(&pattern.pat, bindings),
        syn::Pat::Reference(pattern) => collect_pattern_bindings(&pattern.pat, bindings),
        syn::Pat::Slice(pattern) => {
            for element in &pattern.elems {
                collect_pattern_bindings(element, bindings);
            }
        }
        syn::Pat::Struct(pattern) => {
            for field in &pattern.fields {
                collect_pattern_bindings(&field.pat, bindings);
            }
        }
        syn::Pat::Tuple(pattern) => {
            for element in &pattern.elems {
                collect_pattern_bindings(element, bindings);
            }
        }
        syn::Pat::TupleStruct(pattern) => {
            for element in &pattern.elems {
                collect_pattern_bindings(element, bindings);
            }
        }
        syn::Pat::Type(pattern) => collect_pattern_bindings(&pattern.pat, bindings),
        _ => {}
    }
}

fn collect_assigned_roots(expression: &syn::Expr, bindings: &mut BTreeSet<String>) {
    match expression {
        syn::Expr::Array(expression) => {
            for element in &expression.elems {
                collect_assigned_roots(element, bindings);
            }
        }
        syn::Expr::Field(expression) => collect_assigned_roots(&expression.base, bindings),
        syn::Expr::Index(expression) => collect_assigned_roots(&expression.expr, bindings),
        syn::Expr::Paren(expression) => collect_assigned_roots(&expression.expr, bindings),
        syn::Expr::Path(expression) => {
            if let Some(identifier) = expression.path.get_ident() {
                bindings.insert(identifier.to_string());
            }
        }
        syn::Expr::Tuple(expression) => {
            for element in &expression.elems {
                collect_assigned_roots(element, bindings);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_assignment_propagates_private_binding() {
        let initial = BTreeSet::from(["sender".to_owned()]);
        let source = "let alias; if condition { alias = sender; }";
        let inferred = inferred_private_identifiers(source, source, &initial);

        assert!(
            inferred.contains("alias"),
            "nested assignment was not tracked"
        );
    }
}

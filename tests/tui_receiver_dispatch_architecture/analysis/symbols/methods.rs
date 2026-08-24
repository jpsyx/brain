use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct MethodIndex {
    inherent: HashSet<String>,
    traits: HashMap<String, HashSet<String>>,
}

impl MethodIndex {
    pub(super) fn register(
        &mut self,
        owner: &str,
        trait_name: Option<&str>,
        method: &str,
    ) -> String {
        let target = method_target(owner, trait_name, method);
        let unqualified = method_target(owner, None, method);
        if let Some(trait_name) = trait_name {
            self.traits
                .entry(unqualified)
                .or_default()
                .insert(trait_name.to_owned());
        } else {
            self.inherent.insert(unqualified);
        }
        target
    }

    pub(super) fn resolve(
        &self,
        owner: &str,
        method: &str,
        module: &str,
        imports: Option<&HashMap<String, Vec<String>>>,
    ) -> Option<String> {
        let unqualified = method_target(owner, None, method);
        if self.inherent.contains(&unqualified) {
            return Some(unqualified);
        }
        let mut visible = self
            .traits
            .get(&unqualified)?
            .iter()
            .filter(|trait_name| trait_is_visible(trait_name, module, imports));
        let trait_name = visible.next()?;
        if visible.next().is_some() {
            return None;
        }
        Some(method_target(owner, Some(trait_name), method))
    }
}

pub(super) fn method_target(owner: &str, trait_name: Option<&str>, method: &str) -> String {
    trait_name.map_or_else(
        || format!("{owner}::{method}"),
        |trait_name| format!("<{owner} as {trait_name}>::{method}"),
    )
}

fn trait_is_visible(
    trait_name: &str,
    module: &str,
    imports: Option<&HashMap<String, Vec<String>>>,
) -> bool {
    trait_name
        .rsplit_once("::")
        .is_some_and(|(trait_module, _)| trait_module == module)
        || imports.is_some_and(|imports| imports.values().any(|path| path.join("::") == trait_name))
}

use std::collections::HashMap;

use super::{ImportIndex, LexicalScope};

impl ImportIndex {
    pub(in super::super) fn trait_is_visible(
        &self,
        module: &[String],
        lexical: &LexicalScope,
        canonical: &str,
    ) -> bool {
        let Some(name) = canonical.rsplit("::").next() else {
            return false;
        };
        if self
            .resolve_scoped(module, &[name.to_owned()], lexical)
            .join("::")
            == canonical
        {
            return true;
        }
        self.visible_named(&module.join("::"), lexical)
            .values()
            .any(|path| self.canonicalize(path.clone()).join("::") == canonical)
    }

    fn visible_named(&self, module: &str, lexical: &LexicalScope) -> HashMap<String, Vec<String>> {
        let mut visible = self.named.get(module).cloned().unwrap_or_default();
        if let Some(declared) = self.declared.get(module) {
            visible.retain(|name, _| !declared.contains(name));
        }
        lexical.extend_named(&mut visible);
        visible
    }
}

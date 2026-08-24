use std::collections::{BTreeSet, HashMap, HashSet};

use super::item_is_test;
use collect::{CollectedUse, collect_use_tree, is_exported, item_declaration, resolve_super};

#[path = "imports/collect.rs"]
mod collect;
#[path = "imports/lexical.rs"]
mod lexical;

pub(crate) use lexical::LexicalScope;

type ModuleSymbols = HashMap<String, HashSet<String>>;
type NamedImports = HashMap<String, HashMap<String, Vec<String>>>;
type GlobImports = HashMap<String, Vec<Vec<String>>>;

#[derive(Default)]
pub(super) struct ImportIndex {
    declared: ModuleSymbols,
    exported: ModuleSymbols,
    named: NamedImports,
    reexports: NamedImports,
    globs: GlobImports,
    reexport_globs: GlobImports,
}

impl ImportIndex {
    pub(super) fn collect(&mut self, items: &[syn::Item], module: &[String]) {
        let module_key = module.join("::");
        for item in items {
            if item_is_test(item) {
                continue;
            }
            if let Some((name, visibility)) = item_declaration(item) {
                self.declared
                    .entry(module_key.clone())
                    .or_default()
                    .insert(name.clone());
                if is_exported(visibility) {
                    self.exported
                        .entry(module_key.clone())
                        .or_default()
                        .insert(name);
                }
            }
            let syn::Item::Use(item_use) = item else {
                continue;
            };
            let mut collected = CollectedUse::default();
            collect_use_tree(&item_use.tree, Vec::new(), &mut collected, module);
            self.named
                .entry(module_key.clone())
                .or_default()
                .extend(collected.named.clone());
            self.globs
                .entry(module_key.clone())
                .or_default()
                .extend(collected.globs.clone());
            if is_exported(&item_use.vis) {
                self.reexports
                    .entry(module_key.clone())
                    .or_default()
                    .extend(collected.named);
                self.reexport_globs
                    .entry(module_key.clone())
                    .or_default()
                    .extend(collected.globs);
            }
        }
    }

    pub(super) fn resolve_scoped(
        &self,
        module: &[String],
        raw: &[String],
        lexical: &LexicalScope,
    ) -> Vec<String> {
        let Some(first) = raw.first() else {
            return Vec::new();
        };
        let module_key = module.join("::");
        let resolved = if matches!(first.as_str(), "crate" | "std" | "core" | "alloc") {
            raw.to_vec()
        } else if first == "self" {
            module
                .iter()
                .cloned()
                .chain(raw.iter().skip(1).cloned())
                .collect()
        } else if first == "super" {
            resolve_super(module, raw)
        } else if let Some(depth) = lexical.declaration_depth(first) {
            local_path(&module_key, depth, raw)
        } else if self
            .declared
            .get(&module_key)
            .is_some_and(|symbols| symbols.contains(first))
        {
            module.iter().cloned().chain(raw.iter().cloned()).collect()
        } else if let Some(imported) = lexical.named(first) {
            imported
                .iter()
                .cloned()
                .chain(raw.iter().skip(1).cloned())
                .collect()
        } else if let Some(imported) = self
            .named
            .get(&module_key)
            .and_then(|imports| imports.get(first))
        {
            imported
                .iter()
                .cloned()
                .chain(raw.iter().skip(1).cloned())
                .collect()
        } else {
            let lexical_resolution = lexical
                .glob_layers()
                .map(|sources| self.glob_resolution_from(sources, first))
                .find(|resolution| !matches!(resolution, Resolution::Missing))
                .unwrap_or(Resolution::Missing);
            match lexical_resolution {
                Resolution::Unique(path) => path
                    .into_iter()
                    .chain(raw.iter().skip(1).cloned())
                    .collect(),
                Resolution::Ambiguous => ambiguous_path(&module_key, first, raw),
                Resolution::Missing => match self.glob_resolution(&module_key, first) {
                    Resolution::Unique(path) => path
                        .into_iter()
                        .chain(raw.iter().skip(1).cloned())
                        .collect(),
                    Resolution::Ambiguous => ambiguous_path(&module_key, first, raw),
                    Resolution::Missing
                        if lexical.has_globs()
                            || self
                                .globs
                                .get(&module_key)
                                .is_some_and(|globs| !globs.is_empty()) =>
                    {
                        ambiguous_path(&module_key, first, raw)
                    }
                    Resolution::Missing => {
                        module.iter().cloned().chain(raw.iter().cloned()).collect()
                    }
                },
            }
        };
        self.canonicalize(resolved)
    }

    pub(super) fn visible_named(
        &self,
        module: &str,
        lexical: &LexicalScope,
    ) -> HashMap<String, Vec<String>> {
        let mut visible = self.named.get(module).cloned().unwrap_or_default();
        lexical.extend_named(&mut visible);
        visible
    }

    fn canonicalize(&self, mut resolved: Vec<String>) -> Vec<String> {
        let mut expanded = HashSet::new();
        loop {
            let replacement = (1..resolved.len()).rev().find_map(|item_index| {
                let module = resolved[..item_index].join("::");
                let item = &resolved[item_index];
                if expanded.contains(&(module.clone(), item.clone())) {
                    return None;
                }
                match self.resolve_export(&module, item, &mut HashSet::new()) {
                    Resolution::Unique(target) => Some((
                        module,
                        item.clone(),
                        target
                            .into_iter()
                            .chain(resolved.iter().skip(item_index + 1).cloned())
                            .collect(),
                    )),
                    Resolution::Ambiguous => Some((
                        module.clone(),
                        item.clone(),
                        ambiguous_path(&module, item, &resolved[item_index..]),
                    )),
                    Resolution::Missing => None,
                }
            });
            let Some((module, item, replacement)) = replacement else {
                break;
            };
            expanded.insert((module, item));
            if replacement == resolved || is_ambiguous(&replacement) {
                resolved = replacement;
                break;
            }
            resolved = replacement;
        }
        resolved
    }

    fn glob_resolution(&self, module: &str, name: &str) -> Resolution {
        self.glob_resolution_from(self.globs.get(module).into_iter().flatten(), name)
    }

    fn glob_resolution_from<'a>(
        &self,
        sources: impl IntoIterator<Item = &'a Vec<String>>,
        name: &str,
    ) -> Resolution {
        let mut candidates = BTreeSet::new();
        for source in sources {
            self.collect_from_glob(source, name, &mut HashSet::new(), &mut candidates);
        }
        Resolution::from_candidates(candidates)
    }

    fn resolve_export(
        &self,
        module: &str,
        name: &str,
        visiting: &mut HashSet<(String, String)>,
    ) -> Resolution {
        let key = (module.to_owned(), name.to_owned());
        if !visiting.insert(key.clone()) {
            return Resolution::Missing;
        }
        let mut candidates = BTreeSet::new();
        if self
            .exported
            .get(module)
            .is_some_and(|symbols| symbols.contains(name))
        {
            candidates.insert(
                module
                    .split("::")
                    .map(str::to_owned)
                    .chain(std::iter::once(name.to_owned()))
                    .collect(),
            );
        }
        if let Some(target) = self
            .reexports
            .get(module)
            .and_then(|exports| exports.get(name))
        {
            self.collect_target(target, visiting, &mut candidates);
        }
        for source in self.reexport_globs.get(module).into_iter().flatten() {
            self.collect_from_glob(source, name, visiting, &mut candidates);
        }
        visiting.remove(&key);
        Resolution::from_candidates(candidates)
    }

    fn collect_from_glob(
        &self,
        source: &[String],
        name: &str,
        visiting: &mut HashSet<(String, String)>,
        candidates: &mut BTreeSet<Vec<String>>,
    ) {
        for module in self.module_targets(source, visiting) {
            match self.resolve_export(&module.join("::"), name, visiting) {
                Resolution::Unique(target) => {
                    candidates.insert(target);
                }
                Resolution::Ambiguous => {
                    candidates.insert(ambiguous_path(&module.join("::"), name, &[]));
                }
                Resolution::Missing => {}
            }
        }
    }

    fn collect_target(
        &self,
        target: &[String],
        visiting: &mut HashSet<(String, String)>,
        candidates: &mut BTreeSet<Vec<String>>,
    ) {
        if self.is_module(target) {
            candidates.insert(target.to_vec());
            return;
        }
        let Some((name, module)) = target.split_last() else {
            return;
        };
        let module_key = module.join("::");
        if self
            .declared
            .get(&module_key)
            .is_some_and(|symbols| symbols.contains(name))
            || matches!(
                target.first().map(String::as_str),
                Some("std" | "core" | "alloc")
            )
        {
            candidates.insert(target.to_vec());
            return;
        }
        match self.resolve_export(&module_key, name, visiting) {
            Resolution::Unique(target) => {
                candidates.insert(target);
            }
            Resolution::Ambiguous => {
                candidates.insert(ambiguous_path(&module_key, name, &[]));
            }
            Resolution::Missing => {
                candidates.insert(target.to_vec());
            }
        }
    }

    fn module_targets(
        &self,
        source: &[String],
        visiting: &mut HashSet<(String, String)>,
    ) -> BTreeSet<Vec<String>> {
        if self.is_module(source) {
            return BTreeSet::from([source.to_vec()]);
        }
        let Some((name, module)) = source.split_last() else {
            return BTreeSet::new();
        };
        match self.resolve_export(&module.join("::"), name, visiting) {
            Resolution::Unique(target) if self.is_module(&target) => BTreeSet::from([target]),
            _ => BTreeSet::new(),
        }
    }

    fn is_module(&self, path: &[String]) -> bool {
        self.declared.contains_key(&path.join("::"))
            || self.named.contains_key(&path.join("::"))
            || self.globs.contains_key(&path.join("::"))
    }
}

enum Resolution {
    Missing,
    Unique(Vec<String>),
    Ambiguous,
}

impl Resolution {
    fn from_candidates(candidates: BTreeSet<Vec<String>>) -> Self {
        if candidates.iter().any(|path| is_ambiguous(path)) {
            return Self::Ambiguous;
        }
        let mut candidates = candidates.into_iter();
        let Some(first) = candidates.next() else {
            return Self::Missing;
        };
        if candidates.next().is_some() {
            Self::Ambiguous
        } else {
            Self::Unique(first)
        }
    }
}

fn ambiguous_path(module: &str, name: &str, suffix: &[String]) -> Vec<String> {
    vec![
        "<ambiguous-glob>".to_owned(),
        module.to_owned(),
        name.to_owned(),
    ]
    .into_iter()
    .chain(suffix.iter().skip(1).cloned())
    .collect()
}

fn is_ambiguous(path: &[String]) -> bool {
    path.first()
        .is_some_and(|segment| segment == "<ambiguous-glob>")
}

fn local_path(module: &str, depth: usize, raw: &[String]) -> Vec<String> {
    vec![
        "<lexical-type>".to_owned(),
        module.to_owned(),
        depth.to_string(),
    ]
    .into_iter()
    .chain(raw.iter().cloned())
    .collect()
}

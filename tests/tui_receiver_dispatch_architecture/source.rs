use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Ownership {
    Production,
    Test,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct ProductionSource {
    pub(super) path: PathBuf,
    pub(super) module: Vec<String>,
}

pub(super) fn production_source_paths(root: &Path) -> Vec<PathBuf> {
    production_sources(root)
        .into_iter()
        .map(|source| source.path)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn production_sources(root: &Path) -> Vec<ProductionSource> {
    let src = root.join("src");
    let inventory = rust_inventory(&src);
    let mut graph = SourceGraph::default();

    for source in production_roots(&src, &inventory) {
        graph.visit_module_file(&source.path, &source.module, Ownership::Production);
    }

    for path in inventory {
        let production_reached = graph.production.iter().any(|source| source.path == path);
        if !production_reached && !graph.test.contains(&path) {
            graph.production.insert(ProductionSource {
                module: inferred_module(&src, &path),
                path,
            });
        }
    }

    let mut sources = graph.production.into_iter().collect::<Vec<_>>();
    sources.sort();
    sources
}

fn rust_inventory(src: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(src)
        .into_iter()
        .map(|entry| entry.expect("walk Rust source"))
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("rs"))
        .map(walkdir::DirEntry::into_path)
        .collect()
}

fn production_roots(src: &Path, inventory: &[PathBuf]) -> Vec<ProductionSource> {
    inventory
        .iter()
        .filter(|path| {
            path.parent() == Some(src)
                && matches!(
                    path.file_name().and_then(|value| value.to_str()),
                    Some("lib.rs" | "main.rs")
                )
                || path
                    .parent()
                    .is_some_and(|parent| parent == src.join("bin"))
        })
        .map(|path| ProductionSource {
            module: root_module(path),
            path: path.clone(),
        })
        .collect()
}

#[derive(Default)]
struct SourceGraph {
    production: HashSet<ProductionSource>,
    test: HashSet<PathBuf>,
    visited: HashSet<(PathBuf, Vec<String>, Ownership)>,
}

impl SourceGraph {
    fn visit_module_file(&mut self, path: &Path, module: &[String], ownership: Ownership) {
        let path = path.to_path_buf();
        if !self
            .visited
            .insert((path.clone(), module.to_owned(), ownership))
        {
            return;
        }
        self.record(&path, module, ownership);

        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let syntax = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        let module_dir = child_module_dir(&path);
        self.visit_items(&syntax.items, &path, &module_dir, module, ownership);
    }

    fn visit_items(
        &mut self,
        items: &[syn::Item],
        containing_file: &Path,
        module_dir: &Path,
        logical_module: &[String],
        inherited: Ownership,
    ) {
        for item in items {
            if let syn::Item::Macro(item_macro) = item
                && item_macro.mac.path.is_ident("include")
                && let Ok(relative) = syn::parse2::<syn::LitStr>(item_macro.mac.tokens.clone())
            {
                let ownership = child_ownership(inherited, &item_macro.attrs);
                let included = containing_file
                    .parent()
                    .expect("included source has parent")
                    .join(relative.value());
                self.visit_included_file(&included, module_dir, logical_module, ownership);
                continue;
            }
            let syn::Item::Mod(module) = item else {
                continue;
            };
            let ownership = child_ownership(inherited, &module.attrs);
            let mut child_module = logical_module.to_vec();
            child_module.push(module.ident.to_string());
            if let Some((_, items)) = &module.content {
                self.visit_items(
                    items,
                    containing_file,
                    &module_dir.join(module.ident.to_string()),
                    &child_module,
                    ownership,
                );
                continue;
            }

            let Some(path) = declared_module_path(containing_file, module_dir, module) else {
                continue;
            };
            self.visit_module_file(&path, &child_module, ownership);
        }
    }

    fn visit_included_file(
        &mut self,
        path: &Path,
        module_dir: &Path,
        module: &[String],
        ownership: Ownership,
    ) {
        let path = path.to_path_buf();
        if !self
            .visited
            .insert((path.clone(), module.to_owned(), ownership))
        {
            return;
        }
        self.record(&path, module, ownership);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let syntax = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        self.visit_items(&syntax.items, &path, module_dir, module, ownership);
    }

    fn record(&mut self, path: &Path, module: &[String], ownership: Ownership) {
        match ownership {
            Ownership::Production => {
                self.production.insert(ProductionSource {
                    path: path.to_path_buf(),
                    module: module.to_vec(),
                });
            }
            Ownership::Test => {
                self.test.insert(path.to_path_buf());
            }
        }
    }
}

fn child_ownership(inherited: Ownership, attributes: &[syn::Attribute]) -> Ownership {
    if inherited == Ownership::Test || is_exact_cfg_test(attributes) {
        Ownership::Test
    } else {
        Ownership::Production
    }
}

pub(super) fn is_exact_cfg_test(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        let syn::Meta::List(meta) = &attribute.meta else {
            return false;
        };
        meta.path.is_ident("cfg")
            && meta
                .parse_args::<syn::Path>()
                .is_ok_and(|path| path.is_ident("test"))
    })
}

fn declared_module_path(
    containing_file: &Path,
    module_dir: &Path,
    module: &syn::ItemMod,
) -> Option<PathBuf> {
    if let Some(path) = explicit_path(&module.attrs) {
        let base = containing_file.parent().expect("module file has parent");
        let candidate = base.join(path);
        return candidate.is_file().then_some(candidate);
    }

    let stem = module.ident.to_string();
    let flat = module_dir.join(format!("{stem}.rs"));
    if flat.is_file() {
        return Some(flat);
    }
    let nested = module_dir.join(stem).join("mod.rs");
    nested.is_file().then_some(nested)
}

fn explicit_path(attributes: &[syn::Attribute]) -> Option<PathBuf> {
    attributes.iter().find_map(|attribute| {
        if !attribute.path().is_ident("path") {
            return None;
        }
        let syn::Meta::NameValue(meta) = &attribute.meta else {
            return None;
        };
        let syn::Expr::Lit(expression) = &meta.value else {
            return None;
        };
        let syn::Lit::Str(path) = &expression.lit else {
            return None;
        };
        Some(PathBuf::from(path.value()))
    })
}

fn child_module_dir(path: &Path) -> PathBuf {
    let parent = path.parent().expect("module file has parent");
    match path.file_name().and_then(|value| value.to_str()) {
        Some("lib.rs" | "main.rs" | "mod.rs") => parent.to_path_buf(),
        _ => parent.join(path.file_stem().expect("Rust source has stem")),
    }
}

fn root_module(path: &Path) -> Vec<String> {
    match path.file_name().and_then(|value| value.to_str()) {
        Some("lib.rs") => vec!["crate".to_owned()],
        Some("main.rs") => vec!["binary".to_owned()],
        Some(file) => vec![
            "binary".to_owned(),
            file.strip_suffix(".rs").unwrap_or(file).to_owned(),
        ],
        None => vec!["binary".to_owned(), path.display().to_string()],
    }
}

fn inferred_module(src: &Path, path: &Path) -> Vec<String> {
    let relative = path
        .strip_prefix(src)
        .unwrap_or_else(|_| panic!("source outside src: {}", path.display()));
    if matches!(
        relative.file_name().and_then(|value| value.to_str()),
        Some("lib.rs")
    ) {
        return vec!["crate".to_owned()];
    }
    if matches!(
        relative.file_name().and_then(|value| value.to_str()),
        Some("main.rs")
    ) {
        return vec!["binary".to_owned()];
    }
    let mut module = vec!["crate".to_owned()];
    for component in relative.components() {
        let value = component.as_os_str().to_string_lossy();
        if value == "mod.rs" {
            continue;
        }
        module.push(value.strip_suffix(".rs").unwrap_or(&value).to_owned());
    }
    module
}

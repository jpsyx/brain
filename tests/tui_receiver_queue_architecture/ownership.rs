#[path = "ownership/aliases.rs"]
mod aliases;
#[path = "ownership/attributes.rs"]
mod attributes;
#[path = "ownership/receiver_effect.rs"]
mod receiver_effect;
#[path = "ownership/visitors.rs"]
mod visitors;

use std::collections::HashSet;
use std::path::{Component, Path};

use aliases::resolve_aliases;
use attributes::AttributeAudit;
use receiver_effect::receiver_effect_payloads_are_one_shot;
use syn::visit::Visit;
use syn::{
    Attribute, ForeignItem, ForeignItemStatic, ImplItem, ImplItemConst, Item, ItemConst, ItemEnum,
    ItemStatic, ItemStruct, ItemType, ItemUnion, TraitItem, TraitItemConst,
};
use visitors::MentionVisitor;

const EFFECT_PATH: &str = "src/tui/receiver/effect.rs";
const QUEUE_PATH: &str = "src/tui/receiver/queue.rs";

pub(super) fn queue_boundary_violations(source: &str) -> Vec<String> {
    queue_boundary_violations_at(Path::new("src/tui/unrelated.rs"), source)
}

pub(super) fn queue_boundary_violations_at(path: &Path, source: &str) -> Vec<String> {
    if is_test_source(path) || is_exact_manifest_path(path, QUEUE_PATH) {
        return Vec::new();
    }
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(error) => return vec![format!("could not parse production Rust: {error}")],
    };

    let mut guard = OwnershipGuard {
        effect_owner: is_exact_manifest_path(path, EFFECT_PATH),
        violations: Vec::new(),
    };
    let aliases = HashSet::from(["InboundJob".to_owned()]);
    guard.inspect_scope(&file.items, &aliases, true);
    guard.violations
}

struct OwnershipGuard {
    effect_owner: bool,
    violations: Vec<String>,
}

impl OwnershipGuard {
    fn inspect_scope(
        &mut self,
        items: &[Item],
        inherited_aliases: &HashSet<String>,
        top_level: bool,
    ) {
        let aliases = resolve_aliases(items, inherited_aliases);
        for item in items.iter().filter(|item| !item_is_cfg_test(item)) {
            match item {
                Item::Const(item) => {
                    self.inspect_persistent(PersistentItem::Const(item), &aliases, top_level);
                }
                Item::Enum(item) => {
                    self.inspect_persistent(PersistentItem::Enum(item), &aliases, top_level);
                }
                Item::ForeignMod(item) => self.inspect_foreign_items(item, &aliases),
                Item::Impl(item) => {
                    for impl_item in &item.items {
                        if let ImplItem::Const(item) = impl_item
                            && !is_cfg_test(&item.attrs)
                        {
                            self.inspect_persistent(
                                PersistentItem::ImplConst(item),
                                &aliases,
                                false,
                            );
                        }
                    }
                }
                Item::Macro(item) => {
                    self.violations.push(format!(
                        "opaque module-level item macro {} can generate persistent storage",
                        path_name(&item.mac.path)
                    ));
                }
                Item::Mod(item) => {
                    if let Some((_, nested)) = &item.content {
                        self.inspect_scope(nested, &aliases, false);
                    }
                }
                Item::Static(item) => {
                    self.inspect_persistent(PersistentItem::Static(item), &aliases, top_level);
                }
                Item::Struct(item) => {
                    self.inspect_persistent(PersistentItem::Struct(item), &aliases, top_level);
                }
                Item::Trait(item) => {
                    for trait_item in &item.items {
                        if let TraitItem::Const(item) = trait_item
                            && !is_cfg_test(&item.attrs)
                        {
                            self.inspect_persistent(
                                PersistentItem::TraitConst(item),
                                &aliases,
                                false,
                            );
                        }
                    }
                }
                Item::Type(item) => {
                    self.inspect_persistent(PersistentItem::Type(item), &aliases, top_level);
                }
                Item::Union(item) => {
                    self.inspect_persistent(PersistentItem::Union(item), &aliases, top_level);
                }
                _ => {}
            }
        }
    }

    fn inspect_foreign_items(&mut self, item: &syn::ItemForeignMod, aliases: &HashSet<String>) {
        for foreign in &item.items {
            match foreign {
                ForeignItem::Static(item) if !is_cfg_test(&item.attrs) => {
                    self.inspect_persistent(PersistentItem::ForeignStatic(item), aliases, false);
                }
                ForeignItem::Macro(item) if !is_cfg_test(&item.attrs) => {
                    self.violations.push(format!(
                        "opaque foreign item macro {} can generate persistent storage",
                        path_name(&item.mac.path)
                    ));
                }
                ForeignItem::Verbatim(_) => self
                    .violations
                    .push("opaque foreign item syntax can declare persistent storage".to_owned()),
                _ => {}
            }
        }
    }

    fn inspect_persistent(
        &mut self,
        item: PersistentItem<'_>,
        aliases: &HashSet<String>,
        top_level: bool,
    ) {
        let mut attributes = AttributeAudit::default();
        item.visit(&mut attributes);
        if let Some(attribute) = attributes.unsupported() {
            self.violations.push(format!(
                "{} {} has unsupported attribute {attribute}",
                item.kind(),
                item.name()
            ));
            return;
        }

        if let PersistentItem::Enum(effect) = item
            && self.effect_owner
            && top_level
            && effect.ident == "ReceiverEffect"
            && receiver_effect_payloads_are_one_shot(effect, aliases)
        {
            return;
        }

        let mut mentions = MentionVisitor::new(aliases);
        item.visit(&mut mentions);
        if mentions.found() {
            self.violations.push(format!(
                "{} {} declares persistent raw InboundJob storage",
                item.kind(),
                item.name()
            ));
        }
    }
}

#[derive(Clone, Copy)]
enum PersistentItem<'ast> {
    Const(&'ast ItemConst),
    Enum(&'ast ItemEnum),
    ForeignStatic(&'ast ForeignItemStatic),
    ImplConst(&'ast ImplItemConst),
    Static(&'ast ItemStatic),
    Struct(&'ast ItemStruct),
    TraitConst(&'ast TraitItemConst),
    Type(&'ast ItemType),
    Union(&'ast ItemUnion),
}

impl<'ast> PersistentItem<'ast> {
    fn visit<V: Visit<'ast>>(self, visitor: &mut V) {
        match self {
            Self::Const(item) => visitor.visit_item_const(item),
            Self::Enum(item) => visitor.visit_item_enum(item),
            Self::ForeignStatic(item) => visitor.visit_foreign_item_static(item),
            Self::ImplConst(item) => visitor.visit_impl_item_const(item),
            Self::Static(item) => visitor.visit_item_static(item),
            Self::Struct(item) => visitor.visit_item_struct(item),
            Self::TraitConst(item) => visitor.visit_trait_item_const(item),
            Self::Type(item) => visitor.visit_item_type(item),
            Self::Union(item) => visitor.visit_item_union(item),
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::Const(_) | Self::ImplConst(_) | Self::TraitConst(_) => "const",
            Self::Enum(_) => "enum",
            Self::ForeignStatic(_) | Self::Static(_) => "static",
            Self::Struct(_) => "struct",
            Self::Type(_) => "type alias",
            Self::Union(_) => "union",
        }
    }

    fn name(self) -> String {
        match self {
            Self::Const(item) => item.ident.to_string(),
            Self::Enum(item) => item.ident.to_string(),
            Self::ForeignStatic(item) => item.ident.to_string(),
            Self::ImplConst(item) => item.ident.to_string(),
            Self::Static(item) => item.ident.to_string(),
            Self::Struct(item) => item.ident.to_string(),
            Self::TraitConst(item) => item.ident.to_string(),
            Self::Type(item) => item.ident.to_string(),
            Self::Union(item) => item.ident.to_string(),
        }
    }
}

pub(super) fn item_is_cfg_test(item: &Item) -> bool {
    match item {
        Item::Const(item) => is_cfg_test(&item.attrs),
        Item::Enum(item) => is_cfg_test(&item.attrs),
        Item::ExternCrate(item) => is_cfg_test(&item.attrs),
        Item::Fn(item) => is_cfg_test(&item.attrs),
        Item::ForeignMod(item) => is_cfg_test(&item.attrs),
        Item::Impl(item) => is_cfg_test(&item.attrs),
        Item::Macro(item) => is_cfg_test(&item.attrs),
        Item::Mod(item) => is_cfg_test(&item.attrs),
        Item::Static(item) => is_cfg_test(&item.attrs),
        Item::Struct(item) => is_cfg_test(&item.attrs),
        Item::Trait(item) => is_cfg_test(&item.attrs),
        Item::TraitAlias(item) => is_cfg_test(&item.attrs),
        Item::Type(item) => is_cfg_test(&item.attrs),
        Item::Union(item) => is_cfg_test(&item.attrs),
        Item::Use(item) => is_cfg_test(&item.attrs),
        _ => false,
    }
}

pub(super) fn is_cfg_test(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        let syn::Meta::List(meta) = &attribute.meta else {
            return false;
        };
        meta.path.is_ident("cfg")
            && syn::parse2::<syn::Meta>(meta.tokens.clone()).is_ok_and(
                |nested| matches!(nested, syn::Meta::Path(path) if path.is_ident("test")),
            )
    })
}

fn is_test_source(path: &Path) -> bool {
    let path = manifest_relative(path).unwrap_or(path);
    path.components()
        .any(|component| matches!(component, Component::Normal(name) if name == "tests"))
        || path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem == "tests" || stem.ends_with("_tests"))
}

fn is_exact_manifest_path(path: &Path, expected: &str) -> bool {
    manifest_relative(path).is_some_and(|path| path == Path::new(expected))
}

fn manifest_relative(path: &Path) -> Option<&Path> {
    if path.is_absolute() {
        path.strip_prefix(env!("CARGO_MANIFEST_DIR")).ok()
    } else {
        Some(path)
    }
}

pub(super) fn path_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

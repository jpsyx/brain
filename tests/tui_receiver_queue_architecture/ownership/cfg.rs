use syn::{Attribute, Item, Meta};

use super::identifiers::ident_is;

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
        let Meta::List(cfg) = &attribute.meta else {
            return false;
        };
        path_is(&cfg.path, "cfg")
            && syn::parse2::<Meta>(cfg.tokens.clone())
                .is_ok_and(|condition| cfg_condition_implies_test(&condition))
    })
}

pub(super) fn cfg_condition_implies_test(condition: &Meta) -> bool {
    match condition {
        Meta::Path(path) => path_is(path, "test"),
        Meta::List(list) if path_is(&list.path, "all") => parse_conditions(list)
            .is_some_and(|conditions| conditions.iter().any(cfg_condition_implies_test)),
        Meta::List(list) if path_is(&list.path, "any") => parse_conditions(list)
            .is_some_and(|conditions| conditions.iter().all(cfg_condition_implies_test)),
        Meta::List(_) | Meta::NameValue(_) => false,
    }
}

fn path_is(path: &syn::Path, expected: &str) -> bool {
    path.segments.len() == 1
        && path
            .segments
            .first()
            .is_some_and(|segment| ident_is(&segment.ident, expected))
}

pub(super) fn parse_conditions(list: &syn::MetaList) -> Option<Vec<Meta>> {
    list.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        .ok()
        .map(|conditions| conditions.into_iter().collect())
}

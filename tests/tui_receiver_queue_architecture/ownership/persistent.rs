use syn::visit::Visit;
use syn::{
    ForeignItemStatic, ImplItemConst, ImplItemType, ItemConst, ItemEnum, ItemStatic, ItemStruct,
    ItemType, ItemUnion, TraitItemConst, TraitItemType,
};

#[derive(Clone, Copy)]
pub(super) enum PersistentItem<'ast> {
    Const(&'ast ItemConst),
    Enum(&'ast ItemEnum),
    ForeignStatic(&'ast ForeignItemStatic),
    ImplConst(&'ast ImplItemConst),
    ImplType(&'ast ImplItemType),
    Static(&'ast ItemStatic),
    Struct(&'ast ItemStruct),
    TraitConst(&'ast TraitItemConst),
    TraitType(&'ast TraitItemType),
    Type(&'ast ItemType),
    Union(&'ast ItemUnion),
}

impl<'ast> PersistentItem<'ast> {
    pub(super) fn visit<V: Visit<'ast>>(self, visitor: &mut V) {
        match self {
            Self::Const(item) => visitor.visit_item_const(item),
            Self::Enum(item) => visitor.visit_item_enum(item),
            Self::ForeignStatic(item) => visitor.visit_foreign_item_static(item),
            Self::ImplConst(item) => visitor.visit_impl_item_const(item),
            Self::ImplType(item) => visitor.visit_impl_item_type(item),
            Self::Static(item) => visitor.visit_item_static(item),
            Self::Struct(item) => visitor.visit_item_struct(item),
            Self::TraitConst(item) => visitor.visit_trait_item_const(item),
            Self::TraitType(item) => visitor.visit_trait_item_type(item),
            Self::Type(item) => visitor.visit_item_type(item),
            Self::Union(item) => visitor.visit_item_union(item),
        }
    }

    pub(super) fn kind(self) -> &'static str {
        match self {
            Self::Const(_) | Self::ImplConst(_) | Self::TraitConst(_) => "const",
            Self::Enum(_) => "enum",
            Self::ForeignStatic(_) | Self::Static(_) => "static",
            Self::Struct(_) => "struct",
            Self::ImplType(_) | Self::TraitType(_) | Self::Type(_) => "type alias",
            Self::Union(_) => "union",
        }
    }

    pub(super) fn name(self) -> String {
        match self {
            Self::Const(item) => item.ident.to_string(),
            Self::Enum(item) => item.ident.to_string(),
            Self::ForeignStatic(item) => item.ident.to_string(),
            Self::ImplConst(item) => item.ident.to_string(),
            Self::ImplType(item) => item.ident.to_string(),
            Self::Static(item) => item.ident.to_string(),
            Self::Struct(item) => item.ident.to_string(),
            Self::TraitConst(item) => item.ident.to_string(),
            Self::TraitType(item) => item.ident.to_string(),
            Self::Type(item) => item.ident.to_string(),
            Self::Union(item) => item.ident.to_string(),
        }
    }
}

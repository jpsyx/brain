use syn::visit::{self, Visit};
use syn::{Attribute, Field, Variant};

use super::{is_cfg_test, path_name};

#[derive(Default)]
pub(super) struct AttributeAudit {
    unsupported: Option<String>,
}

impl AttributeAudit {
    pub(super) fn unsupported(&self) -> Option<&str> {
        self.unsupported.as_deref()
    }
}

impl<'ast> Visit<'ast> for AttributeAudit {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        if self.unsupported.is_none() {
            self.unsupported = unsupported_attribute(attribute);
        }
    }

    fn visit_field(&mut self, field: &'ast Field) {
        if !is_cfg_test(&field.attrs) {
            visit::visit_field(self, field);
        }
    }

    fn visit_variant(&mut self, variant: &'ast Variant) {
        if !is_cfg_test(&variant.attrs) {
            visit::visit_variant(self, variant);
        }
    }
}

fn unsupported_attribute(attribute: &Attribute) -> Option<String> {
    unsupported_attribute_meta(&attribute.meta)
}

fn unsupported_attribute_meta(meta: &syn::Meta) -> Option<String> {
    let path = meta.path();
    if path.segments.len() != 1 {
        return Some(path_name(path));
    }
    let name = path.segments.first()?.ident.to_string();
    if name == "cfg_attr" {
        let syn::Meta::List(list) = meta else {
            return Some(name);
        };
        let Ok(nested) = list.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) else {
            return Some(name);
        };
        return nested.iter().skip(1).find_map(unsupported_attribute_meta);
    }
    if matches!(
        name.as_str(),
        "allow"
            | "cfg"
            | "deny"
            | "deprecated"
            | "derive"
            | "doc"
            | "expect"
            | "forbid"
            | "must_use"
            | "non_exhaustive"
            | "path"
            | "repr"
            | "warn"
    ) {
        None
    } else {
        Some(name)
    }
}

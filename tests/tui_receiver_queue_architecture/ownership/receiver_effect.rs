use std::collections::HashSet;

use syn::{Fields, GenericArgument, ItemEnum, PathArguments, Type, Variant};

use super::cfg::is_cfg_test;
use super::visitors::{mentions_expr, mentions_generics, mentions_type};

pub(super) fn receiver_effect_payloads_are_one_shot(
    item: &ItemEnum,
    aliases: &HashSet<String>,
) -> bool {
    if !item.generics.params.is_empty()
        || item.generics.lt_token.is_some()
        || item.generics.where_clause.is_some()
        || mentions_generics(&item.generics, aliases)
    {
        return false;
    }

    item.variants
        .iter()
        .filter(|variant| !is_cfg_test(&variant.attrs))
        .all(|variant| receiver_variant_is_valid(variant, aliases))
}

fn receiver_variant_is_valid(variant: &Variant, aliases: &HashSet<String>) -> bool {
    if variant
        .discriminant
        .as_ref()
        .is_some_and(|(_, expression)| mentions_expr(expression, aliases))
    {
        return false;
    }
    let mentions_job = variant
        .fields
        .iter()
        .filter(|field| !is_cfg_test(&field.attrs))
        .any(|field| mentions_type(&field.ty, aliases));
    if !mentions_job {
        return true;
    }

    let Fields::Unnamed(fields) = &variant.fields else {
        return false;
    };
    if fields.unnamed.len() != 1 {
        return false;
    }
    let payload = &fields.unnamed[0].ty;
    match variant.ident.to_string().as_str() {
        "ApplyRestart" => is_boxed_restart_plan(payload),
        "ApplyNewSession" | "Dispatch" => is_boxed_job(payload),
        _ => false,
    }
}

fn is_boxed_job(ty: &Type) -> bool {
    generic_type_argument(ty, &["std", "boxed", "Box"]).is_some_and(is_job_type)
}

fn is_boxed_restart_plan(ty: &Type) -> bool {
    generic_type_argument(ty, &["std", "boxed", "Box"])
        .and_then(|payload| {
            generic_type_argument(payload, &["crate", "server", "receiver", "RestartPlan"])
        })
        .is_some_and(is_job_type)
}

fn generic_type_argument<'ast>(ty: &'ast Type, expected: &[&str]) -> Option<&'ast Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() {
        return None;
    }
    if path.path.segments.len() != expected.len() {
        return None;
    }
    for (segment, expected) in path.path.segments.iter().zip(expected) {
        if segment.ident != *expected {
            return None;
        }
    }
    let segment = path.path.segments.last()?;
    if path
        .path
        .segments
        .iter()
        .take(path.path.segments.len().saturating_sub(1))
        .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    if arguments.args.len() != 1 {
        return None;
    }
    match arguments.args.first()? {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    }
}

fn is_job_type(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path.path.leading_colon.is_none()
        && path.path.segments.len() == 4
        && path
            .path
            .segments
            .iter()
            .zip(["crate", "server", "receiver", "InboundJob"])
            .all(|(segment, expected)| {
                segment.ident == expected && matches!(segment.arguments, PathArguments::None)
            })
}

use std::collections::HashSet;

use syn::{Fields, GenericArgument, ItemEnum, PathArguments, Type, Variant};

use super::is_cfg_test;
use super::visitors::{mentions_expr, mentions_generics, mentions_type};

pub(super) fn receiver_effect_payloads_are_one_shot(
    item: &ItemEnum,
    aliases: &HashSet<String>,
) -> bool {
    if mentions_generics(&item.generics, aliases) {
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
        "ApplyRestart" => is_boxed_restart_plan(payload, aliases),
        "ApplyNewSession" | "Dispatch" => is_boxed_job(payload, aliases),
        _ => false,
    }
}

fn is_boxed_job(ty: &Type, aliases: &HashSet<String>) -> bool {
    generic_type_argument(ty, "Box").is_some_and(|payload| is_job_type(payload, aliases))
}

fn is_boxed_restart_plan(ty: &Type, aliases: &HashSet<String>) -> bool {
    generic_type_argument(ty, "Box")
        .and_then(|payload| generic_type_argument(payload, "RestartPlan"))
        .is_some_and(|job| is_job_type(job, aliases))
}

fn generic_type_argument<'ast>(ty: &'ast Type, expected: &str) -> Option<&'ast Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != expected {
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

fn is_job_type(ty: &Type, aliases: &HashSet<String>) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path.path.segments.last().is_some_and(|segment| {
            aliases.contains(&segment.ident.to_string())
                && matches!(segment.arguments, PathArguments::None)
        })
}

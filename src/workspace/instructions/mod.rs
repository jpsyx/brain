//! Pure ownership and rendering for the workspace instruction document.

mod document;
mod legacy;
mod render;

pub(crate) use document::{
    CURRENT_REVISION, DocumentPlan, InstructionReceipt, has_managed_region, is_whole_managed_frame,
    plan_document, plan_for_revision, verified_body,
};

pub(crate) use legacy::{RemovedLegacySpan, is_legacy_document};
pub(crate) use render::render_contract;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod replacement_tests;

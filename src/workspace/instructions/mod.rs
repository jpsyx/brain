//! Pure ownership and rendering for the workspace instruction document.

mod document;
mod legacy;
mod render;

pub(crate) use document::{
    DocumentPlan, InstructionReceipt, has_managed_region, is_whole_managed_frame, plan_document,
    verified_body,
};

pub(crate) use legacy::{RemovedLegacySpan, is_legacy_document};
pub(crate) use render::render_contract;

#[cfg(test)]
mod tests;

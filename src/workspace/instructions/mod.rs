//! Pure ownership and rendering for the workspace instruction document.

mod document;
mod legacy;
mod render;

pub(crate) use document::{
    DocumentPlan, InstructionReceipt, has_managed_region, plan_document, verified_body,
};

pub(crate) use legacy::RemovedLegacySpan;
pub(crate) use render::render_contract;

#[cfg(test)]
mod tests;

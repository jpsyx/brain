//! Pure ownership and rendering for the workspace instruction document.

#![allow(dead_code)] // The package coordinator integrates this model next.

mod document;
mod legacy;
mod render;

#[allow(unused_imports)] // Consumed by the package coordinator in the next stage.
pub(crate) use document::{DocumentPlan, InstructionReceipt, plan_document, verified_body};

#[allow(unused_imports)] // Retained-span provenance is consumed by package down.
pub(crate) use legacy::RemovedLegacySpan;
pub(crate) use render::render_contract;

#[cfg(test)]
mod tests;

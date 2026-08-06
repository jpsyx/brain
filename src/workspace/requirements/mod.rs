//! Central selected-workspace feature health and remediation decisions.

mod capabilities;
mod features;
mod inspect;
mod model;
mod receiver;
mod render;
mod sync;

pub use inspect::requirements;
pub use model::{
    FeatureStatus, PromptMetadata, RequiredStatus, Requirement, RequirementScope, RequirementStatus,
};
pub use render::format_requirements;

pub(crate) use model::required_fields;

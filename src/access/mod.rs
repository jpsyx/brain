//! Portable workspace access policy and advisory enforcement helpers.

mod capabilities;
mod enforcement;
mod mcp;
mod mode;
mod prompt;
mod skills;
mod store;

pub use capabilities::{AccessPolicy, render_access_status};
pub use enforcement::{CapabilityEnforcement, CapabilityEnforcementReport, EnforcementEvidence};
pub use mcp::MachineCapabilityEnvironment;
pub use mode::AccessMode;
pub use prompt::{boundary_prompt, classify_obvious_outside_path};
pub use skills::{CapabilityError, CapabilityPlan, capability_plan, capability_plan_for};

pub(crate) use mcp::{codex_mcp_launch, write_claude_runtime_config};
pub(crate) use skills::ResolvedSkillSource;
pub(crate) use store::{
    ensure_portable_access_mode, ensure_registry_access_modes, load_portable_access_mode,
    set_portable_access_mode,
};

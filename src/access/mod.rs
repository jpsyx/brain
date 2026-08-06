//! Portable workspace access policy and advisory enforcement helpers.

mod artifact;
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

pub(crate) use artifact::{
    ensure_directory as ensure_capability_directory,
    existing_directory as existing_capability_directory, remove_path as remove_capability_path,
};
pub(crate) use mcp::{
    cleanup_claude_runtime_artifacts, cleanup_codex_runtime_artifacts,
    cleanup_workspace_capabilities, codex_mcp_launch, prepare_workspace_capabilities,
    write_claude_runtime_config,
};
pub(crate) use skills::ResolvedSkillSource;
pub(crate) use store::{
    ensure_portable_access_mode, ensure_registry_access_modes, set_portable_access_mode,
};

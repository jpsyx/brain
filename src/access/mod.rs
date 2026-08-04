//! Portable workspace access policy and advisory enforcement helpers.

mod capabilities;
mod mode;
mod prompt;
mod store;

pub use capabilities::{AccessPolicy, render_access_status};
pub use mode::AccessMode;
pub use prompt::{boundary_prompt, classify_obvious_outside_path};

pub(crate) use store::{
    ensure_portable_access_mode, ensure_registry_access_modes, load_portable_access_mode,
    set_portable_access_mode,
};

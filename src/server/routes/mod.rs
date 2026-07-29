//! The brain server's route registry.
//!
//! Each recognized endpoint gets a module here; the accept loop in [`super`]
//! dispatches to it via the pure [`super::router`]. Adding an endpoint is one
//! `pub mod` line plus its module.

pub mod habits;

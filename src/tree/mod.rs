//! The brain-directory tree sub-view's pure model.
// The model is built bottom-up over several commits, so its functions are
// written and tested before the sub-view that calls them exists. The tests do
// use them, so the allowance is scoped to non-test builds only, mirroring
// `tasks::shortcuts`.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "wired up when the sub-view lands")
)]

pub(crate) mod build;
pub(crate) mod root;

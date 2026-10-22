//! Compatibility names for the shared completion and removal service.
#[cfg(test)]
pub(crate) use crate::tui::task_mutation::worker::NativeService;
pub(crate) use crate::tui::task_mutation::worker::{RemovalService, Request, Response};

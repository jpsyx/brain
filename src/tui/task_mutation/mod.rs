//! Shared native task mutation queue and optimistic effects.
mod native;
pub(crate) mod projection;
pub(crate) mod worker;
pub(crate) type Rows = (Vec<crate::tasks::task::Task>, Vec<crate::tasks::task::Task>);
mod coordinator;
pub(crate) use coordinator::Coordinator;

mod app;

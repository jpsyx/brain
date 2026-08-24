//! The tasks **main view**: task management, agenda, and triage over
//! `~/brain/tasks/{tasks,habits}.csv`.
//!
//! Everything under `tasks::` is specific to the tasks view (the CSV model,
//! the sub-view pipeline, task-card rendering, the `complete`/`doctor`
//! utilities, and the tasks clap args). It reuses the crate-level
//! `session` / `state` / `pty_pane` / `plan` infrastructure shared with the
//! brain-search view. See [`docs/glossary.md`](../../docs/glossary.md) for the
//! main-view / sub-view / brain-panel vocabulary.

pub mod add;
pub(crate) mod agenda;
pub mod cli;
pub mod complete;
pub mod doctor;
pub mod identity;
pub mod plain;
pub mod render;
pub mod revive;
pub mod schema;
pub mod selector;
pub mod set;
pub mod shortcuts;
pub mod skip;
pub(crate) mod store_lock;
pub mod task;
pub mod triage_habits;
pub mod view;

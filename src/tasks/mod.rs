//! The tasks **main view**: task management, agenda, and triage over
//! `~/brain/tasks/{tasks,habits}.csv`.
//!
//! Everything under `tasks::` is specific to the tasks view (the CSV model,
//! the sub-view pipeline, task-card rendering, the `complete`/`doctor`
//! utilities, and the tasks clap args). It reuses the crate-level
//! `session` / `state` / `pty_pane` / `plan` infrastructure shared with the
//! brain-search view. See [`docs/glossary.md`](../../docs/glossary.md) for the
//! main-view / sub-view / brain-panel vocabulary.

pub mod cli;
pub mod complete;
pub mod doctor;
pub mod plain;
pub mod render;
pub mod revive;
pub mod selector;
pub mod shortcuts;
pub mod task;
pub mod view;

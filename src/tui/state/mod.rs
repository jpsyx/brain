mod shell;
mod tasks;

#[cfg(test)]
pub(crate) use shell::resolve_active_tab;
pub(crate) use shell::{ShellState, tab_for_slot, tab_order};
pub(crate) use tasks::{TasksState, TasksStateInit};

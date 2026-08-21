use std::path::PathBuf;

use chrono::NaiveDate;

use crate::session::AgentKind;
use crate::tasks::task::Task;
use crate::tasks::view::{TaskViewOptions, View, ViewSpec};

pub(crate) struct TuiLaunch {
    pub(crate) command_context: crate::workspace::CommandContext,
    pub(crate) view: ViewSpec,
    pub(crate) task_options: TaskViewOptions,
    pub(crate) agent_kind: AgentKind,
    pub(crate) today: NaiveDate,
    pub(crate) csv_path: PathBuf,
    pub(crate) all_tasks: Vec<Task>,
    pub(crate) all_habits: Vec<Task>,
    pub(crate) active_view: Option<View>,
    pub(crate) initial_search: Option<String>,
    pub(crate) skip_daily_triage_check: bool,
}

use crate::tasks::cli::Cli;

pub(crate) struct TaskViewOptions {
    pub(crate) hard_deadline: Option<bool>,
    pub(crate) status: Option<String>,
    pub(crate) priority: Option<String>,
    pub(crate) task_type: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) energy: Option<String>,
    pub(crate) context: Option<String>,
    pub(crate) assigned_to: Option<String>,
    pub(crate) past_due: bool,
    pub(crate) mit: bool,
    pub(crate) stale: bool,
    pub(crate) no_due: bool,
    pub(crate) blocked: bool,
    pub(crate) include_done: bool,
    pub(crate) include_deferred: bool,
    pub(crate) linear_issue: Option<String>,
    pub(crate) search: Option<String>,
    pub(crate) sort: String,
    pub(crate) reverse: bool,
    pub(crate) full_notes: bool,
}

impl From<&Cli> for TaskViewOptions {
    fn from(cli: &Cli) -> Self {
        let filters = &cli.filters;
        let display = &cli.display;
        Self {
            hard_deadline: filters.hard_deadline,
            status: filters.status.clone(),
            priority: filters.priority.clone(),
            task_type: filters.task_type.clone(),
            project: filters.project.clone(),
            energy: filters.energy.clone(),
            context: filters.context.clone(),
            assigned_to: filters.assigned_to.clone(),
            past_due: filters.past_due,
            mit: filters.mit,
            stale: filters.stale,
            no_due: filters.no_due,
            blocked: filters.blocked,
            include_done: filters.include_done,
            include_deferred: filters.include_deferred,
            linear_issue: filters.linear_issue.clone(),
            search: filters.search.clone(),
            sort: display.sort.clone(),
            reverse: display.reverse,
            full_notes: display.full_notes,
        }
    }
}

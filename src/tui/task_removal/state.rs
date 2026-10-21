use crate::tasks::removal::{
    model::{Options, Problem, RemovalPlan},
    operations::Receipt,
};

#[derive(Clone)]
pub(crate) enum RemovalScreen {
    Preview(RemovalPlan),
    Running { operation_id: Option<String> },
    Failed(Problem),
    Finished(Receipt),
}

#[derive(Clone)]
pub(crate) struct RemovalState {
    pub generation: u64,
    pub targets: Vec<String>,
    pub options: Options,
    pub screen: RemovalScreen,
    pub operation_id: Option<String>,
    pub notice: String,
    pub abandon_confirm: bool,
    pub scroll: u16,
    pub selected: usize,
}

impl RemovalState {
    pub(super) const fn running(&self) -> bool {
        matches!(self.screen, RemovalScreen::Running { .. })
    }
    pub(super) const fn recovery_available(&self) -> bool {
        !self.running() && !self.abandon_confirm && self.operation_id.is_some()
    }
}

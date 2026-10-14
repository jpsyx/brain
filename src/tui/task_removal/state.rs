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
}

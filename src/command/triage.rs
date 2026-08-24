//! `brain triage state` — the one bit of triage bookkeeping brain owns.

use anyhow::Result;
use chrono::Local;

use crate::cli::{TriageAction, TriageArgs};
use crate::tasks::triage_state;
use crate::workspace::CommandContext;

pub fn run(args: &TriageArgs, context: &CommandContext) -> Result<()> {
    let TriageAction::State(state_args) = &args.action;
    let root = context.workspace.root();
    let today = Local::now().date_naive();
    if state_args.mark {
        let previous = triage_state::mark(root, today)?;
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "marked": triage_state::month_of(today),
                "previous": previous,
            }))?
        );
        return Ok(());
    }
    println!(
        "{}",
        serde_json::to_string(&triage_state::read(root, today))?
    );
    Ok(())
}

use super::{MaintenanceIntent, options};
use crate::tasks::{
    backlog::{dedupe, minus_six_months, purge},
    complete::parse_date,
    identity::CsvKind,
    removal::{
        execute::problem,
        model::{Identity, Options, Problem},
        snapshot::Snapshot,
    },
};
use std::collections::BTreeSet;

pub(in crate::tasks::removal) fn validate(
    snapshot: &Snapshot,
    targets: &[Identity],
    policy: &Options,
    intent: &MaintenanceIntent,
) -> Result<(), Problem> {
    let invalid = || {
        problem(
            "invalid_maintenance",
            "Maintenance intent does not match the frozen task identities and policy",
        )
    };
    if *policy != options()
        || targets.is_empty()
        || targets.iter().any(|id| id.csv != CsvKind::Tasks)
    {
        return Err(invalid());
    }
    let actual = |identity: &Identity| {
        snapshot.tasks.identities.iter().any(|current| {
            current.csv == identity.csv
                && current.task_id == identity.task_id
                && current.task_uuid == identity.task_uuid
        })
    };
    if targets.iter().any(|id| !actual(id)) {
        return Err(invalid());
    }
    match intent {
        MaintenanceIntent::BacklogPurge { cutoff } => {
            let date = parse_date(&snapshot.execution_date)
                .and_then(minus_six_months)
                .ok_or_else(invalid)?;
            if date.to_string() != *cutoff {
                return Err(invalid());
            }
            let eligible = purge::expired(&snapshot.tasks.parsed.rows, date);
            if targets
                .iter()
                .any(|id| !eligible.iter().any(|task| task.task_id == id.task_id))
            {
                return Err(invalid());
            }
        }
        MaintenanceIntent::BacklogDedupe { replacements } => {
            let removed: BTreeSet<_> = targets.iter().map(|id| id.task_id.as_str()).collect();
            let sources: BTreeSet<_> = replacements
                .iter()
                .map(|pair| pair.duplicate.task_id.as_str())
                .collect();
            if sources != removed || sources.len() != replacements.len() {
                return Err(invalid());
            }
            let (eligible, _) = dedupe::candidates(&snapshot.tasks.parsed.rows);
            for pair in replacements {
                if pair.survivor.csv != CsvKind::Tasks
                    || pair.duplicate.csv != CsvKind::Tasks
                    || !actual(&pair.survivor)
                    || !actual(&pair.duplicate)
                    || removed.contains(pair.survivor.task_id.as_str())
                    || !eligible.iter().any(|task| {
                        task.task_id == pair.duplicate.task_id
                            && task.survivor_id == pair.survivor.task_id
                    })
                {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}

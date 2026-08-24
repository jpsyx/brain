//! The task ↔ project bidirectional link.
//!
//! A task names its project in a column; the project lists its tasks in
//! `.METADATA.json`. Either side can drift, and the two drifts are not
//! symmetric:
//!
//! - A project that does not know about a task pointing at it is a **fixable**
//!   omission: the task's claim is the newer, more specific fact, so the
//!   metadata is brought up to date.
//! - A project listing a task id that exists nowhere, or a task naming a
//!   project directory that does not exist, is **only reported**. Something was
//!   deleted or renamed, and guessing which would destroy information.

use std::collections::{BTreeMap, BTreeSet};

use super::row::Issue;

/// One project's declared task list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectLinks {
    pub(crate) slug: String,
    pub(crate) listed: BTreeSet<String>,
}

/// What reconciling the two sides found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LinkFindings {
    /// `slug -> the full task list the project should carry`.
    pub(crate) repairs: BTreeMap<String, BTreeSet<String>>,
    pub(crate) issues: Vec<Issue>,
}

/// Pure: reconcile `forward` (task → project) against every project's list.
pub(crate) fn reconcile(
    forward: &BTreeMap<String, BTreeSet<String>>,
    known_slugs: &BTreeSet<String>,
    projects: &[ProjectLinks],
) -> LinkFindings {
    let mut findings = LinkFindings::default();
    for (slug, task_ids) in forward {
        if !known_slugs.contains(slug) {
            findings.issues.push(Issue(format!(
                "orphan task→project: project '{slug}' (referenced by {} task(s)) does not exist",
                task_ids.len()
            )));
        }
    }
    for project in projects {
        let expected = forward.get(&project.slug).cloned().unwrap_or_default();
        let missing_in_metadata: BTreeSet<String> =
            expected.difference(&project.listed).cloned().collect();
        let missing_in_csv: BTreeSet<String> =
            project.listed.difference(&expected).cloned().collect();
        if !missing_in_metadata.is_empty() {
            findings.repairs.insert(
                project.slug.clone(),
                project
                    .listed
                    .union(&missing_in_metadata)
                    .cloned()
                    .collect(),
            );
        }
        if !missing_in_csv.is_empty() {
            findings.issues.push(Issue(format!(
                "orphan project→task: project '{}' lists task_id(s) {:?} that don't exist in any CSV",
                project.slug,
                missing_in_csv.iter().collect::<Vec<_>>()
            )));
        }
    }
    findings
}

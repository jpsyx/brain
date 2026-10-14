//! Explicit deletion policies and serializable domain failures.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Dependents {
    #[default]
    Reject,
    Unlink,
    Rewire,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinkedIssue {
    #[default]
    Configured,
    Keep,
    Cancel,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Options {
    pub habit: bool,
    pub dependents: Dependents,
    pub linked_issue: LinkedIssue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Problem {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

impl Problem {
    pub(crate) fn new(code: &str, message: &str, details: serde_json::Value) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details,
        }
    }
}

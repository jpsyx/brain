//! Audited effective hook destinations, including unchanged semantic phases.

pub(super) const HOOKS: &[(&str, &str)] = &[
    ("second-brain:company-context", "workspace:context"),
    (
        "second-brain:reference-manager",
        "organize:reference-manager",
    ),
    ("todo:anchors", "agenda:anchors"),
    ("todo:calendar", "agenda:calendar"),
    ("todo:cutoff", "agenda:cutoff"),
    ("todo:agenda-after-build", "agenda:after-build"),
    ("todo:linear-backlog", "todo:linear-backlog"),
    ("todo:linear", "todo:linear"),
    ("contacts:fallback", "contacts:fallback"),
    ("triage:daily-open", "triage:daily-open"),
    ("triage:daily-subagents", "triage:daily-subagents"),
    ("triage:daily-linear", "triage:daily-linear"),
    ("triage:daily-merge", "triage:daily-merge"),
    (
        "triage:daily-required-outputs",
        "triage:daily-required-outputs",
    ),
    ("triage:weekly-inboxes", "triage:weekly-inboxes"),
    ("triage:weekly-linear", "triage:weekly-linear"),
];

pub(super) fn mapping(owner: &str, hook: &str) -> Option<(&'static str, &'static str)> {
    HOOKS.iter().copied().find(|(legacy, canonical)| {
        (hook == *legacy || hook == *canonical)
            && (owner == skill(legacy) || owner == skill(canonical))
    })
}

pub(super) fn skill(hook: &str) -> &str {
    hook.split(':').next().unwrap_or(hook)
}

# Triage heuristics compatibility reference

<a id="triage-heuristics"></a>
<a id="when-to-invoke"></a>
<a id="step-1--pick-the-most-useful-grouping"></a>
<a id="step-2--present-the-groups"></a>
<a id="step-3--bulk-operations-per-group"></a>
<a id="step-4--1-by-1-walk"></a>
<a id="step-5--summary"></a>
<a id="never-auto-mutate"></a>
<a id="hard-deadline-tasks"></a>

Follow the canonical [/triage daily workflow](../SKILL.md#daily-triage),
including deadline screening before group mutation, native removal previews,
no-penalty waiting/blocked defers, and the current chronic/at-risk detectors.
This path preserves old callers; it does not define a second triage procedure.

For a drop, submit all consented IDs in one command under
[the native removal contract](../../todo/SKILL.md#remove-tasks). Consent covers the
chosen dependency and linked-issue policies; pass the preview fingerprint as
`--expect-plan`. The CLI owns link cleanup. A stale preview requires fresh consent.

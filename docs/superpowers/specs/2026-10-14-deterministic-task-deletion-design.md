# Deterministic task deletion

**Date:** 2026-10-14
**Status:** Core implementation complete; final acceptance and private deployment pending.
**Source baseline:** `fe477ec` (`0.101.0`).
**Implementation plan:** [Deterministic task deletion](../plans/2026-10-14-deterministic-task-deletion.md).

Deleting an explicitly selected task must be a native Brain operation. The
same workspace state, target identities, and options must produce the same
changes from the CLI, TUI, and skill workflows. Confirmation remains a user
decision; deciding which references are worth preserving is no longer an
agent decision.

## Original baseline behavior and problem

At the source baseline, `src/tui/app_actions/commands.rs::run_remove` validated
the selected row and sent a prompt to the agent frontend after TUI confirmation.
The prompt asked the agent to inspect chunked siblings, blockers, and project references,
then decide whether to delete or offer alternatives. One suggested alternative,
`status=dropped`, is not a supported task status.

The baseline `src/tasks/mutate/remove.rs::remove_in_root` removed one row
natively, protected managed triage habits, required `--habit` for habit rows,
and refreshed the agenda. It did not repair other tasks' references or project
metadata, and used the agenda's defer action. Separate backlog removal paths
performed their own writes. Completion and agenda chunk handoff searched for
exactly index + 1, so deleting a middle chunk left a gap they could not cross.

Baseline skill installation had version stamps, startup sync, and a
machine-wide legacy migration. They overlapped, had different auto-sync gates,
and lacked a reliable installer-time, per-workspace compatibility migration.

## Scope and alternatives

| Approach | Result | Decision |
|---|---|---|
| Keep the agent and improve its prompt | Still varies by frontend and model; relationships remain prose policy | Reject |
| Wire the TUI directly to today's removal primitive | Removes the agent but leaves dangling relationships and upgrade gaps | Reject |
| Share a native planner and executor across every surface | Explicit policies, one implementation, testable effects and upgrades | Adopt |

The deliverable includes native local deletion, a narrow optional integration
adapter protocol, CLI/TUI wiring, bundled skill updates, and automatic managed
skill upgrades. It does not introduce a dropped status, an undo product,
automatic project archiving, recursive deletion of dependent tasks, general
workflow hooks, or a new task identity/schema migration.

Provider-specific integration code and user-owned skill customizations remain
outside the public core. Shipping the generic core does not by itself migrate
an existing agent-only tracker integration. A deployment using such an
integration is complete only when its adapter and instructions are updated.

## Requirements

| ID | Requirement |
|---|---|
| R1 | Resolve explicit targets once and bind execution to immutable workspace/task identity. |
| R2 | Provide deterministic dependency, cross-reference, and project cleanup. |
| R3 | Preserve chunk ordering and MIT/agenda behavior after gaps appear. |
| R4 | Retain habit opt-in and managed-triage protections. |
| R5 | Preview and apply one validated local batch with crash recovery. |
| R6 | Distinguish committed deletion, integration progress, and derived-artifact failures. |
| R7 | Support human and noninteractive CLI callers without an agent. |
| R8 | Execute exactly the same service from the TUI without a frontend handoff. |
| R9 | Support optional deterministic external cancellation and explicit local-only removal. |
| R10 | Route maintenance callers through shared cleanup without changing their scope or consent. |
| R11 | Make bundled skills name supported commands and explicit policies. |
| R12 | Refresh managed skills in every available registered workspace at installation. |
| R13 | Retry unavailable/failed workspace refreshes, preserve customization, and support downgrade. |
| R14 | Preserve workspace isolation, unknown data, and existing sync conflict semantics. |

## CLI contract

```text
brain tasks remove <TARGET>... [--habit]
  [--dependents reject|unlink|rewire]
  [--linked-issue configured|keep|cancel]
  [--dry-run] [--json] [--yes] [--expect-plan HASH]

brain tasks remove --resume OPERATION_ID [--json]
brain tasks remove --operation-status OPERATION_ID [--json]
brain tasks remove --abandon OPERATION_ID --yes [--json]
brain skills sync --all-workspaces [--json]
```

`rm` and `drop` remain aliases. Targets accept the existing case-insensitive
display IDs, unambiguous bare integers, and unique name fragments. Resolve all
targets against one snapshot, deduplicate by immutable identity, and sort the
result by CSV kind and numeric display ID. A missing or ambiguous target
rejects the entire batch. No listing filter expands the target set. Reject
mutation-relevant inherited filters with an explanatory error rather than
silently ignoring their apparent effect.

The normal default is `--dependents reject --linked-issue configured`.
`--yes` represents existing consent, not permission to bypass a guard. A human
without `--yes` receives a concrete preview and confirmation. A noninteractive
caller without `--yes` receives `confirmation_required`; it never waits for
stdin. `--json` is noninteractive and writes exactly one JSON document to
stdout. Progress goes to stderr. Dry-run needs no confirmation, does not run
an external adapter, and does not mutate task or project data. Normal Brain
bootstrap maintenance is outside the dry-run's task-data scope.

Preview returns a plan even when policy prevents execution: `ready=false`,
typed blockers, affected objects, and available options. Exit status is 0 for
a ready preview or committed apply, 2 for validation/consent/stale-plan
errors, and 1 for operational failures. A committed deletion with an agenda
warning exits 0 and reports `committed_with_warnings`. Broken CLI syntax keeps
clap's exit status 2; syntax failures need not use the JSON envelope.

`--expect-plan` is accepted for apply, not dry-run. A plan fingerprint is a
SHA-256 digest of a canonical versioned document including workspace UUID,
actor ID, date, options, sorted and deduplicated selected identity keys
(CSV kind, task UUID, display ID), task/habit bytes, inspected project metadata bytes,
and relevant integration configuration. Sort arrays and map keys explicitly.
Exclude presentation text, file mtimes, credentials, and random operation IDs.
A changed fingerprint never authorizes a recomputed deletion silently.

`--resume`, `--operation-status`, and `--abandon` are mutually exclusive with
targets and policy flags. Resume/status reject confirmation flags; abandon
requires `--yes`. Resume continues only the originally consented
operation, revalidates its saved identity/state, and cannot broaden its scope.
Status is read-only. It reports `prepared`, `external_pending`,
`external_complete`, `local_committed`, `complete`, `needs_reconfirmation`,
or `abandoned`. Abandon acknowledges already-recorded external effects,
leaves local tasks intact, and closes an uncommitted operation. It cannot run
while that operation has an active executor or an unknown in-flight external
outcome. Run resume to settle that outcome first. After abandonment, an
ordinary new preview/apply supplies fresh consent; the adapter recognizes an
already-canceled issue. A committed operation cannot be abandoned.

JSON uses `schema_version: 1` and a `kind` of `preview`, `result`, `error`, or
`operation`. A preview contains `fingerprint`, `ready`, `targets`, `effects`,
and `blockers`. A result contains `operation_id`, `status` (`committed` or
`committed_with_warnings`), `committed`, `removed`,
`updated`, `external`, and `warnings`. Errors contain a stable `code`, a
human-readable `message`, and structured `details`. Never put credentials in
plans, fingerprints, logs, or results.

## Target and relationship semantics

### Identity and habits

Use persisted `task_uuid` when available. On a supported legacy store, derive
the existing deterministic legacy UUID from workspace UUID, CSV kind, and
display ID, and bind the complete before-image too. Do not write new UUIDs or
activate the coordinated task-schema migration as a deletion side effect.

A batch contains tasks or habits, never both. `--habit` is required for habit
rows and rejected for task rows. Habit removal deletes only explicitly named
occurrences. Retiring the ordinary pending occurrence stops its future
recurrence; completed historical occurrences are preserved unless separately
named. Do not infer a whole habit family from a matching title. Managed
triage rows remain protected while the feature is enabled. No flag overrides
that protection.

### Dependencies

Let D be the explicitly deleted set. Only surviving rows that reference D are
changed. Dependencies wholly inside D do not block deletion.

| Policy | Surviving task B has `blocked_by=A`, with A in D |
|---|---|
| `reject` | Return `dependents_exist`, with B's ID/name and the affected edge. |
| `unlink` | Remove A; retain every unrelated blocker. |
| `rewire` | Replace A by A's prerequisites, recursively expanding prerequisites in D until reaching surviving tasks. |

Rewire preserves unrelated blockers, deduplicates references, and sorts them
by numeric task ID. It does not mark any task complete or delete dependents.
Reject missing prerequisite targets, ambiguous IDs, self-edges, and cycles
in the traversed/changed graph before writing. Unrelated pre-existing cycles
are outside this operation's repair scope. Stamp changed surviving rows'
`last_touched` once using the execution date; leave schedules, priority,
assignment, completion dates, and defer counts unchanged.

### Structured links and project metadata

For `see_also`, remove only an entire pipe-delimited item whose trimmed value
is exactly a deleted task ID. Preserve URLs, paths, punctuation-bearing
prose, and every other item byte-for-byte. Free-text occurrences in `notes`
and Markdown documents remain history and are not rewritten.

Inspect `.METADATA.json` under active `projects/` and mirrored
`archive/projects/`. Remove deleted IDs from every `tasks[]` array, including
a stale reverse link not named by the deleted row's `project` field. Preserve
all other fields and surviving array order. Fail before mutation on malformed
metadata that prevents a complete scan. Missing project metadata is an
explicit preview warning, not permission to create a project. Do not run
global `lint --fix` or reindex as an accidental broad repair.

Do not delete/archive an empty project or its notes. Never follow symlinks
outside the selected workspace to mutate linked content.

### Chunks

Keep original chunk labels and IDs; deleting `(2/4)` does not rename history.
Share one next-surviving-chunk resolver among deletion, completion, and agenda
sync. Use the existing parsed base/index/total plus project identity and
dependency evidence to distinguish families. Duplicate candidate positions
are `ambiguous_chunk_family`, not an invitation to choose the first row.

The next chunk is the lowest greater surviving index in the same family with
status other than `done`. If a deleted chunk has MIT, move that tag to this
next chunk when its status is `not_started` or `in_progress`, it is due/visible
today, and it is unblocked by unfinished
tasks after the relationship edits. Otherwise remove the deleted MIT without
inventing a new actionable task. Preserve existing MIT tags and never create
a duplicate agenda entry. No date, duration, priority, or family-wide
renumbering is part of deletion.

Agenda sync receives the removed before-images and explicit successor IDs;
it cannot look up deleted rows in the post-deletion CSV. A batch performs one
document refresh. Preserve sections the native agenda synchronizer does not
own. Missing agenda files are a successful no-op. A PDF/rendering failure is
a derived-artifact warning after canonical commit, not a failed deletion.

## Architecture and transaction boundary

Introduce `src/tasks/removal/` with focused modules for model, snapshot,
resolution, dependencies, references, chunks, projects, planning, fingerprint,
execution, operations, integrations, and reporting. `src/tasks/mutate/remove.rs`
becomes a compatibility facade with no alternate deletion implementation.

The planner accepts an immutable snapshot and typed options and returns a
`RemovalPlan`. The executor accepts the selected `CommandContext`, saved plan,
and consent; it does not parse natural language. Both CLI and TUI call it.

Local execution acquires task-store ownership, revalidates the snapshot and
fingerprint, and writes the task CSV, affected project metadata, and
maintenance breadcrumbs as one recoverable group. Extract the existing
managed-triage grouped transaction engine into a shared task-store module;
retain caller-specific path allowlists and old-journal recovery. Do not
broaden the triage allowlist simply because removal can write archive paths.

All native writers of the affected task/project files, including project
set/archive and sync metadata publication, must obey the same lock order.
Acquire sync ownership before task ownership where both are required; removal
never acquires sync ownership while holding task ownership. Background sync
must not publish a partially installed group. Compare-before-write catches
unsupported external editors; it is not a replacement for coordinating
Brain's own writers. Automatic local journal recovery precedes the next native
task/project mutation or sync publication.

After a successful local commit, release canonical ownership and update
derived agenda artifacts. Keep the committed transaction journal until the
operation's immutable local-commit receipt is durable; recovery records that
separate receipt before journal cleanup, without acquiring an operation lock
while holding task ownership. Operation status treats the commit receipt as
authoritative even if its mutable progress record still says external-complete.
Executors acquire operation ownership before task ownership; recovery never
reverses that order. Maintain the existing three-way sync policy:
deletion beats an unchanged remote row; a concurrent remote edit survives and
is reported as a conflict. This feature adds no portable deletion tombstones.

## Optional external cancellation

Core defines a removal-only adapter protocol, not a tracker implementation.
Portable `task_removal_integration` is absent by default; when present it is
`{"adapter":"tracker","link_field":"linear_issue","on_remove":"cancel"}`.
The supported link field in v1 is the existing `linear_issue` column. Machine
env `task_removal_adapters` maps adapter names to executable argv arrays,
explicit environment-variable names to forward, and a timeout. Credentials
stay in machine environment or the adapter's credential store.

`configured` means `keep` without a policy and the configured action with one.
`keep` never invokes an adapter. `cancel` requires a configured, available
adapter for any nonempty linked issue. If no selected task has a link it is a
no-op. A nonselected row with status other than `done` sharing an issue blocks cancellation with
`shared_external_link`; deleting one local duplicate must not cancel work
still represented by another row. Group identical issue references into one
external action.

The executable receives one bounded JSON document on stdin, using argv rather
than a shell, and returns one bounded JSON response. Request fields are
`schema_version`, `operation_id`, `action_id`, `workspace_id`, `actor_id`,
`action` (`preflight`, `cancel`, or `status`), and `issue`. Responses echo the
version/IDs and report `ready`, `canceled`, `already_canceled`, `pending`, or
`error`. Preflight/status must not mutate. The adapter owns provider state
mapping, authentication, and idempotent recognition of an already-canceled
issue. An unrelated completed state is an explicit conflict, not permission
to change it.

Preview never invokes the executable. Apply revalidates and durably records
consent, then preflights all external actions before canceling any. Execute
cancellations in stable order with bounded timeouts, recording each result.
Release task ownership during network calls. Before local commit, reacquire
ownership and revalidate all canonical before-images and integration policy.
If anything changed, retain the operation as `needs_reconfirmation`; do not
delete a different row or silently undo an already successful cancellation.

External effects cannot be rolled back atomically with local files. On
partial external failure, leave every local target intact and retain precise
per-action progress. Resume queries unknown outcomes before retrying and
never expands the consented set. Only after every required external action
succeeds may the local transaction commit. Operation records are machine-local,
workspace-UUID scoped, written atomically with restrictive permissions, and
contain no credentials. Read-only status and explicit resume are available
without an agent; startup must not perform new external cancellations.

A provider integration must update its existing skill instructions to call
the configured native operation. Use `--linked-issue keep` when a local row is
removed because ownership changed, the issue is already canceled, or the row
is a duplicate. Provider-specific deployment and acceptance tests are a
separate deliverable from the public core.

## TUI behavior

Keep `Ctrl+Backspace`, the palette Remove command, and target picker behavior.
Replace the generic confirmation with a plan-backed removal modal. Display
IDs plus names, relationship effects, project cleanup, habit retirement, and
external actions. When dependents exist, offer Keep prerequisites (rewire),
Remove dependency links (unlink), and Cancel. No selection authorizes
deleting dependent tasks.

The selected policy rebuilds the preview before final confirmation. A stale
plan returns to the preview with an explanation. Adapter execution runs as a
bounded background operation so the event loop remains responsive. The TUI
shows progress and recovery/status actions for its operation. Cancel before
execution makes no changes; once an external operation is underway, closing
the modal does not claim to roll back completed effects.

Success reloads tasks and selects the nearest surviving row. Failure retains
selection and reports the typed error. No path invokes `send_brain_prompt`,
opens a new agent tab, or depends on a configured frontend.

## Maintenance and skills

Bulk triage passes exact selected IDs once, after group consent. Agents may
translate intent to explicit options; they must not implement relationship
cleanup, edit CSV rows, or infer cancellation from a title.

Backlog purge retains its six-month cutoff, silent success, and project
breadcrumbs. Its fixed policy is rewire plus external keep. If safe rewiring
cannot be computed, leave the affected batch unchanged and log an operational
error; do not silently unlink. Backlog dedupe returns an unambiguous retained
task identity, redirects structured references from duplicate to survivor,
preserves the external issue, and removes the duplicate through the same
transaction executor. Multiple eligible twins are reported/skipped, not
arbitrarily selected. Habit-history cleanup retains its existing retention
rule and never gains access to active habit deletion.

Task-to-project conversion creates its new tasks and links using existing
commands, then invokes deletion with explicit consent and policies. Failure
to remove the source leaves it in place and reports incomplete conversion;
this change does not promise a transaction across the entire conversion.

Update bundled `todo`, its command catalog and project-link reference,
`triage`, and relevant `second-brain` task references. Explain both CLI and
TUI behavior, all guards, and recovery commands. Preserve agent judgment for
choosing whether to abandon work, not for applying an already selected
deletion. Remove instructions proposing unsupported statuses or manual
post-deletion repair. Ordinary native completion remains unchanged except for
the shared gap-aware chunk resolver.

## Managed skill upgrade and downgrade

Register a versioned `task_deletion_skills` startup migration with up/down
callbacks. Up runs the existing rendering model for every available registered
workspace, validating manifest identity and preserving legacy workspace
bootstrap rules. It refreshes all Brain-managed bundled skill trees, including
reference files, and their project-local frontend links. No global skill
registry is modified.

This is a mandatory bundled-command compatibility refresh, independent of
`skills_auto_sync`. Preserve that setting; update its documentation to state
that it governs optional routine refreshes. User plugin/extension source
directories are never rewritten. Same-name plugin overrides and unmarked
user-owned directories are explicit conflicts, not silently overwritten source. Render
extensions as usual and report a machine-detectable same-name core override
as requiring user reconciliation. Do not attempt LLM-based migration of
arbitrary custom prose.

Existing generated directories have only a version marker, not a content
manifest. Back them up before the first compatibility refresh; do not claim
to detect every historical manual edit. Introduce a managed-file digest
manifest for future comparisons, preserve files outside that manifest, and
report a modified managed file before replacing it on later refreshes.

Stage each complete managed tree and validate it before replacement. Use a
skills-install lock and a recovery manifest to complete or restore an
interrupted installation. Stamp each workspace only after its files and links
are installed. A failed or unavailable root remains pending; never create a
missing workspace root. One root's failure must not prevent attempts on the
others. Installer output distinguishes updated, pending-unavailable, and
failed. Available-root failures return nonzero; unavailable roots may remain
pending with a visible summary. Reconciliation retries when they reappear.

Reuse this coordinator from version change, TUI startup, and explicit
`brain skills sync --all-workspaces`. Explicit all-workspace sync conflicts
with `--root` and an explicit workspace selector. Existing selected-workspace
sync remains supported. Per-actor capability renders are refreshed at the
next launch; an already-running conversation cannot be made to forget text
it has already read.

Downgrade must render the target binary's embedded skills. The newer binary's
down callback invalidates its compatibility markers and refuses downgrade if
an unresolved removal operation would become unrecoverable. It must not
render newer skills labeled as older. The installer retains a temporary copy
of the migration-capable source binary on downgrade; after replacement, that
helper enumerates registered workspaces and invokes the target binary's
existing `-w <NAME> skills sync` interface for each available root. Do not use
`--root`: older binaries treat it as a sandbox and read different extension
and plugin directories.
Preflight that target interface before replacing the installed executable.
Use argv, validate each workspace identity, and exclude unavailable roots.
Supported targets must have that interface and a compatible workspace schema;
otherwise stop before replacement with the exact incompatibility.

The same post-install coordinator runs on upgrades and reinstalls. Tests must
exercise the real install protocol with stub old/new binaries; a migration
unit test alone does not prove the correct binary renders on downgrade.
Direct binary replacement receives the normal startup retry path for upgrades;
downgrades through unsupported old installers are outside this guarantee.

## Acceptance and release

| Area | Acceptance evidence |
|---|---|
| Native behavior | CLI and TUI produce identical canonical bytes for the same plan. |
| Relationships | Deletion leaves no references to removed IDs in supported structured fields or project arrays. |
| Consent | Ambiguous, stale, protected, or unconfirmed requests change no canonical data. |
| Chunks | First/middle/last and consecutive deletions preserve eligible successor handoff. |
| Recovery | Failure injection at each local journal boundary restores old or commits new state; never a silent partial group. |
| Integrations | Offline, timeout, shared issue, partial cancellation, crash, and resume tests require no agent. |
| Maintenance | Purge/dedupe retain their special semantics while using shared cleanup. |
| Skills | Full documented command examples parse, including options and global workspace flags. |
| Upgrades | Two workspaces, disabled auto-sync, customizations, missing roots, retry, same-version repair, and downgrade are covered. |
| Isolation | No operation changes another workspace or a global skill registry. |

Use red/green TDD, the repository's formatting/lint conventions, and its
version policy. Required release checks are `cargo test --release`,
`cargo clippy --release --all-targets -- -D warnings`, and the repository's
configured personal-data guard. Update architecture, features, data model,
integrations, configuration, keybinding descriptions, testing, and decisions.
No real task deletion or live tracker cancellation belongs in validation.

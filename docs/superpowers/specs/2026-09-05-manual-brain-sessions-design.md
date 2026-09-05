# Manual Brain Sessions Design

## Summary

Brain will support multiple persistent manual sessions in the brain panel. The
Main session remains a manual session and is permanently assigned to tab 0.
Users can start additional named manual sessions from the global command
palette, close those additional sessions, and have the same ordered set restored
after restarting the TUI.

This change also establishes three explicit session categories:

- **Receiver sessions:** background Email or SMS sessions owned by the receiver
  lifecycle.
- **Skill sessions:** ephemeral Daily triage or user-specified skill sessions.
- **Manual sessions:** persistent user-facing sessions, including the Main
  session and sessions started from the command palette.

## Session and tab architecture

The Main session stays structurally separate from the ordered collection of
additional tabs. This makes the tab-0 invariant direct and prevents an ordinary
remove operation from deleting or reordering Main.

The existing ephemeral-tab collection becomes an additional-session collection.
Each entry keeps a lifetime-monotonic `SessionTabId`, title, controller, and
typed metadata:

- `Manual`, with a stable durable manual-session identity;
- `Skill`, with its `SkillSessionKey` and completion token;
- `Receiver`, with its receiver job and remote instance identities.

The shared collection continues to provide insertion order, stable selection,
controller lookup, tab titles, and shutdown traversal. Type-specific operations
must inspect metadata, so manual close logic cannot remove a receiver tab and
receiver cleanup cannot consume manual or skill metadata.

The code and documentation will retire names that imply every additional tab is
ephemeral. `BrainTab::Main` remains appropriate because it identifies the
special tab position, while collection and accessor names will use session-tab
terminology.

## Manual-session naming modal

The global `Start new brain session` command opens a captive single-line text
modal. It follows the existing modal ownership and routing model.

- `Esc` cancels without changing session state.
- `Enter` trims surrounding whitespace.
- Blank names keep the modal open and show an inline validation error.
- A name equal to an open manual-session title after case-insensitive comparison
  is rejected and keeps the modal open.
- A valid name closes the modal and launches a fresh, unprompted session.
- A successful launch selects the new tab and focuses the brain panel.
- A launch or persistence failure creates no tab and reports a themed error.

The fixed Main title participates in uniqueness checks, so a user cannot create
another session named `Brain`.

## Lifecycle behavior

Main is always a manual session and is never user-closeable. `Ctrl+X` on Main is
a no-op, the global `Close brain` command is removed, and an exited Main
controller is relaunched into tab 0 through the existing resume-refusal
fallback. `Message brain` selects and focuses Main, launching its controller
there if necessary.

Additional manual sessions are persistent and interactive. Each has the same
frontend-neutral `AgentController`, input, rendering, resize, observation, and
shutdown behavior as Main. `Ctrl+X` or its matching palette command closes the
active additional manual session, releases only its own lock, deletes its
durable mapping, and chooses a valid neighboring tab without changing unrelated
tab identities.

Skill sessions keep their current single-prompt completion protocol and remain
untracked. Users can close them through `Ctrl+X` or a matching palette command,
and their completion signal can still auto-close them.

Receiver sessions keep their existing background ownership. They receive no
user-close action, and `Ctrl+X` does not close them. Receiver insertion and
cleanup continue to preserve active tab, main view, panel visibility, and
keyboard focus.

Whole-shell teardown shuts down every controller and releases each manual
session's runtime lock without deleting its durable mapping. This allows the
ordered manual tabs to return on the next launch.

## Persistence model

A versioned `manual_sessions` table records the ordered manual-session set. Each
row stores:

- a stable manual-session UUID;
- the exact current native agent session ID;
- title and position;
- whether the row is Main;
- agent frontend, workspace, actor, and channel scope.

Constraints enforce one Main row per scope, unique positions, and
case-insensitive unique titles. Main is always position 0. Additional manual
positions are compacted after a close while their stable identities remain
unchanged.

Each manual session receives its own stable Brain instance identity. The value
is passed through the existing lifecycle metadata, allowing native session
locking and release to operate per manual tab instead of per TUI. The generic
session-start bridge updates both `brain_sessions` and the matching
`manual_sessions` row atomically when `/new`, `/clear`, compaction, or another
frontend lifecycle event changes the native session ID. Claude, Codex, and
OpenCode use the same bridge contract.

The state-schema migration provides both `up` and `down` operations. Up creates
the new table and indexes without guessing a Main mapping from historical rows.
For an upgraded database with no manual rows, the first TUI launch uses the
existing most-recent eligible resume behavior for Main and records the selected
session afterward. Down drops only the manual-session mapping and leaves the
native `brain_sessions` history intact.

## Startup restoration

Startup loads manual mappings for the selected frontend, workspace, actor, and
interactive channel. It restores Main first at tab 0, then additional sessions
in stored order.

For each saved native session, Brain verifies frontend-specific resume evidence
before claiming and launching it. If evidence is stale or the frontend refuses
the resume, Brain launches a fresh native session under the same manual identity
and title, then updates the mapping. A failed additional-tab launch is reported
without shifting or selecting unrelated tabs; its durable row remains available
for a later restart. Main launch failure leaves the invariant-bearing Main tab
in place and allows the existing retry path to recover it.

Fresh manual-session creation registers the native session and durable mapping
before process launch. A synchronous launch failure rolls both records back.
Closing a manual session removes its mapping so it cannot return on restart,
releases its exact native-session lock, and shuts down only that controller.

## Command palette and switching

The global command palette contains:

- `Start new brain session`;
- `Show main brain session` whenever an additional user-selectable session is
  open;
- `Show {title} session` for every additional manual or skill session;
- `Close {title} session` for every additional manual or skill session.

Main never receives a Close row. Receiver sessions receive neither Show nor
Close rows, preserving their background-only behavior and the current receiver
switching rules. Existing `Alt+[` and `Alt+]` cycling, stable tab-slot selection,
and tab-strip rendering operate over Main followed by the shared additional-tab
order.

Palette actions carry stable tab or manual-session identities rather than list
indices or names. Closing a neighboring tab therefore cannot repoint a queued
palette action.

## Error handling

All persistence operations return typed results and route failures to the TUI
status surface. The implementation does not silently downgrade a persistent
manual session to an ephemeral one.

- Validation errors remain in the naming modal.
- Creation failures leave no partial in-memory tab.
- Failed stale-session resumes use the existing fresh-session fallback.
- Per-tab close and lock release target exact identities.
- Receiver errors stay within receiver-owned paths.
- Shutdown attempts every controller even if an earlier controller reports an
  error.

## Test strategy

Development follows one red, green, refactor cycle per behavior. Pure tests
cover name normalization and uniqueness, typed tab metadata, tab ordering,
neighbor selection, palette-row construction, Main invariants, and receiver
exclusions. State-store tests cover ordered persistence, exact per-session
release, close compaction, upgrade, and downgrade. Lifecycle bridge tests cover
native ID rotation for Claude, Codex, and OpenCode.

App-level tests cover successful and failed modal submission, fresh manual-tab
launch, restoration of two or more manual sessions, stale-evidence fallback,
Main relaunch, manual and skill close actions, receiver non-closeability, and
focus preservation. Existing receiver tab, durable completion, recovery, and
session-switching suites remain regression gates.

The completed change must pass formatting, the full release test suite, and
Clippy with warnings denied.

## Documentation and release

The implementation updates `docs/glossary.md`, `docs/features.md`,
`docs/architecture.md`, `docs/data-model.md`, `docs/integrations.md`,
`docs/keybindings.md`, and `docs/decisions.md`. It also updates lifecycle bridge
documentation and any source comments that still describe all additional tabs
as ephemeral.

The user-visible additive feature receives a pre-1.0 minor version bump when
implementation is committed. The design-only commit receives the repository's
required patch bump.

---
id: BR-14
title: Run receiver jobs in isolated resumable tabs
status: done
priority: high
assignee: jpsyx
labels: [feature, server]
estimate: 8
project: PROJ-1
milestone: MS-2
cycle:
parent:
github:
blocked_by: []
created: 2026-08-23
updated: 2026-08-25
---

# BR-14: Run receiver jobs in isolated resumable tabs

## Description

Consume the oldest ready durable job in a dedicated ephemeral brain-panel tab
with its own agent process. Receiver work never types into the main interactive
panel or another running receiver process, never waits for the main panel to
become idle, and never changes the user's active view or focus.

For a known conversation, launch a new process that resumes its stored native
frontend session and supplies the new job as the initial prompt. If that session
is unavailable, corrupt, belongs to another frontend, or cannot be resumed,
start a fresh native session whose prompt references the Brain-owned transcript.
Record the replacement binding and close the process/tab after the run reaches a
terminal lifecycle outcome.

## Acceptance criteria

- [x] The consumer atomically claims, rather than removes, the oldest ready job.
- [x] Every job runs in a dedicated remote-run tab and a newly launched agent
      process through `AgentController`.
- [x] A resumable matching conversation uses `Resume(session_id)` plus the job
      as the initial prompt for Claude, Codex, and OpenCode.
- [x] Missing or incompatible native history starts fresh with the bounded
      Brain transcript and updates the conversation's native binding.
- [x] No receiver path injects text into an existing process or waits for the
      main interactive session.
- [x] Remote tabs run in the background without stealing view, tab, or keyboard
      focus and close themselves after a terminal outcome.
- [x] A message arriving while a remote run is active remains durable and is
      considered only after the current run closes.
- [x] One workspace runs at most one receiver job concurrently in this release.
- [x] Red/green tests cover resume, transcript fallback, frontend changes,
      background focus, mid-run arrival, launch failure, and FIFO draining.
- [x] Brain-panel, skill-session, receiver, glossary, feature, integration,
      architecture, keybinding, decision, and testing docs stay consistent.

## Notes

### Pointers (as of 2026-08-23)

- `src/tui/app_skill_session/` is the closest working pattern for a fresh
  single-prompt process in an auto-closing tab. Extract or share the generic
  lifecycle without representing receiver runs as configured skill sessions.
- `src/tui/state/brain.rs` owns main and ephemeral tab controllers. Add a
  distinct receiver-run representation while preserving monotonic tab identity
  and failure-safe controller shutdown.
- `src/agent/frontend.rs`, `src/agent/session.rs`, and the Claude, Codex, and
  OpenCode adapters already accept a `SessionPlan` and initial prompt. Keep the
  launch decision behind `AgentController`.
- `src/tui/app_brain/receiver/dispatch.rs` is the legacy fresh-launch and
  warm-panel-injection coordinator. Replace its ownership rather than layering
  the new tab path alongside it indefinitely.

### Plan (2026-08-23)

#### Global constraints

- Follow strict red, green, refactor TDD for every behavior. Each production
  change starts with the smallest focused failing test, and the failure must be
  observed before implementation.
- Keep every frontend-specific command, resumability check, lifecycle detail,
  and transport action behind `AgentController`. Claude, Codex, and OpenCode
  must pass the same semantic contract tests.
- Receiver execution must never select a tab, move panel focus, replace the
  main controller, type into a running process, or wait for the interactive
  brain turn to become idle.
- Use narrow `AppServices` operations for durable receiver state and session
  registration. Do not expose the workspace database to the coordinator.
- A successful process spawn advances only to `launching`. BR-15 owns the
  authoritative `accepted` and `processing` observations. A terminal response
  artifact may close the run, but spawn, PTY output, and screen changes may not
  impersonate prompt acceptance.
- Preserve receiver routing, immutable acceptance-time identities,
  attachments, sync freshness, task reload, and provider response behavior.
  BR-17 will later make answer persistence and delivery retry independent.
- Do not log prompt, transcript, response, provider credential, sender, or
  recipient contents. Diagnostics may name opaque job, conversation, run,
  frontend, and lifecycle identifiers.
- Update every document named by the acceptance criteria in the same product
  change. Bump the pre-1.0 minor version because this is additive user-visible
  behavior, and keep `Cargo.lock` synchronized.

#### Task 1: Specify frontend-neutral receiver launch planning

Create a focused receiver-run planning module that converts a durable job and
conversation into one launch attempt. Start with table-driven failing tests for
all three frontends covering a matching resumable binding, a missing or invalid
native history, a frontend change, a resumability probe error, an empty
transcript, and a transcript larger than the prompt budget.

The green implementation must:

- Validate a matching binding with
  `AgentController::resume_candidate_exists`; use `SessionPlan::Resume` plus
  only the new job prompt when validation succeeds.
- Fall back conservatively to `SessionPlan::Fresh` when the binding is absent,
  belongs to another frontend, has missing or corrupt history, cannot be
  claimed, or its validation fails.
- Build a deterministic, UTF-8-safe bounded recovery prompt that retains the
  newest portable transcript context, clearly separates transcript from the
  current authenticated message, and includes attachment references without
  logging either body.
- Add or extend adapter contract tests proving that Claude, Codex, and OpenCode
  translate both `Resume(session_id)` and `Fresh(session_id)` with a non-blank
  initial prompt through their supported launch command.

Run the focused planning, prompt, and adapter tests in red and green phases
before moving to the tab model.

#### Task 2: Generalize ephemeral tabs and preserve focus

Refactor `BrainPanelState` around one monotonic ephemeral-tab collection with
distinct skill-session and receiver-run metadata. Keep the existing skill
session API and behavior intact, but add receiver-run insertion, observation,
removal, and controller access without representing a receiver run as a
configured skill.

Start with failing state and composed App tests proving:

- Skill and receiver tabs share stable, never-reused `SessionTabId` allocation
  and render in one deterministic tab-strip order.
- A rejected allocation shuts down the supplied controller and leaves the tab
  collection and counter unchanged.
- Adding, failing, completing, or removing a background receiver tab preserves
  the current main view, panel visibility, effective tab, and keyboard focus.
- Background receiver controllers participate in app shutdown and close
  independently from the main and skill-session controllers.

Extract generic tab state only as far as these tests require. Keep skill
completion signals and receiver lifecycle metadata in their own owners.

#### Task 3: Add durable launch and native-session ownership

Add narrow durable receiver-run operations to `AppServices` and the receiver
store. Start with failing tests for stale claim owners, atomic launch
preparation, unique remote instance registration, actual native-session
rotation, launch rollback, and binding replacement.

The green implementation must:

- Claim the FIFO head non-destructively and load its immutable job plus logical
  conversation. At most one live receiver claim may exist per workspace.
- Atomically verify the exact live owner and launch-eligible state before
  moving `claimed` or a due pre-acceptance launch retry to `launching`.
- Give each remote run its own hook instance and session-store ownership. Never
  reuse the shell's interactive instance, because its lifecycle hooks may
  rotate or release the main session lineage.
- Claim an exact matching resume session before launch. Register a fresh
  placeholder before launch so lifecycle hooks can rotate it to the actual
  native session created by Codex or OpenCode.
- Persist the replacement conversation binding only from the lifecycle-reported
  native session for the exact remote instance. Never persist a Brain-generated
  placeholder as the native Codex or OpenCode binding, and never rewrite the
  transcript merely to update the binding.
- Roll back or release the exact remote session owner, stop its controller, and
  record a bounded durable launch retry when planning, registration, allocation,
  or process spawn fails. The durable job must remain present.

Keep progressed stale jobs conservative. BR-14 may observe and report a
non-launch-eligible reclaimed state, but must not blindly rerun `accepted`,
`processing`, answer, or delivery work before BR-16 recovery policy exists.

#### Task 4: Run the durable FIFO consumer in background tabs

Replace the production receiver dispatch coordinator with a durable tick that
claims and launches work whenever receiver intent is enabled and no receiver
run is active. Start with composed failing tests for a busy main panel, FIFO
ordering, a second arrival during an active run, successful terminal close,
child exit without a valid completion, claim renewal, lost ownership, and
spawn failure without fixed sleeps.

The green implementation must:

- Preserve the sync-freshness gate before agent work, then create a new PTY and
  `AgentController` for the durable job regardless of main-panel activity.
- Insert the receiver tab in the background without selecting it. No receiver
  branch may call `open_or_focus_brain`, `queue_after_active_turn`,
  `type_text`, or `submit_now`.
- Renew the exact claim while the remote controller remains active and refuse
  lifecycle mutation after ownership is lost.
- Keep later arrivals durable and unclaimed while a receiver tab is active.
  After a terminal run closes, the next tick must consider the oldest ready
  job by `(received_at_unix_ms, job_id)`.
- Correlate the existing completion artifact to the exact remote run identity
  for terminal close and current reply behavior, without treating launch or
  screen activity as acceptance. A child exit lacking a valid completion is a
  pre-acceptance failure and returns through the durable retry path.
- Release the exact remote session owner, shut down the controller once, close
  the tab, reload task state, and preserve the user's active view and focus at
  every terminal outcome.

#### Task 5: Remove receiver execution from the interactive path and document BR-14

Delete or disconnect production warm-panel receiver reuse, main-panel takeover,
and in-memory queue dispatch so only the durable receiver-run coordinator can
start work. Retain generic interactive controller input APIs and any legacy
state still required for explicit BR-18 migration, but ensure no receiver call
site can reach them.

Start with failing architecture and regression tests that detect a receiver
reference to main-panel injection, a receiver wait on interactive activity, or
two simultaneous remote runs. Then update brain-panel, skill-session,
receiver, glossary, feature, integration, architecture, keybinding, decision,
data-model where the binding detail changes, and testing documentation. Record
the interim lifecycle boundary honestly: BR-14 isolates and launches work;
BR-15 adds exact acceptance/progress evidence, BR-16 adds durable recovery,
and BR-17 separates answer persistence from delivery.

Finish with privacy review, `cargo fmt --all --check`, focused release tests,
`cargo test --release`, and
`cargo clippy --release --all-targets -- -D warnings`. Commit only after every
gate passes.

### Self-review (2026-08-23)

- Scope is split at the project boundaries. BR-14 owns isolated launch, native
  continuity, background tabs, and coarse terminal cleanup. BR-15 still owns
  exact prompt acceptance and progress; BR-16 owns stale accepted-job recovery;
  BR-17 owns atomic answer/transcript persistence and delivery retries; BR-18
  owns final legacy-state migration and cleanup.
- The plan does not persist fresh `SessionPlan` placeholders as native bindings.
  Claude can use Brain's supplied ID, but Codex and OpenCode report their real
  IDs through lifecycle hooks, so the exact remote instance is the only safe
  binding source.
- The plan does not infer acceptance from spawn, PTY availability, output, or
  focus. A completion artifact may establish a terminal run, while earlier
  durable acceptance remains BR-15 work.
- Receiver tabs share generic controller storage and tab identity with skill
  sessions, but keep separate metadata and lifecycle rules. This avoids both a
  second tab system and the false claim that remote jobs are configured skills.
- The coordinator remains synchronous only at narrow state calls. If measured
  database waits can exceed the TUI budget, the implementation must move the
  impure claim shell behind a bounded worker while keeping decisions pure.
- `/new` and `/restart` durability are deliberately preserved, not silently
  redefined. If current durable state lacks the exact mutation required for a
  control command, keep that command on an explicit safe path and record the
  remaining final cutover in BR-18 rather than deleting behavior accidentally.

### Log

- 2026-08-23 created from PROJ-1 planning.
- 2026-08-23 unblocked after BR-12 and BR-13 shipped the durable model and
  ingress acceptance boundary. This is the next PROJ-1 implementation task.
- 2026-08-23 started with a current-code architecture review and a five-task
  implementation plan. Self-review resolved native-session binding, focus,
  claim-state, and BR-15/BR-16 boundary risks before production work.

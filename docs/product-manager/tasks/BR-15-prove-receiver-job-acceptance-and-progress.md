---
id: BR-15
title: Prove receiver job acceptance and progress
status: in-progress
priority: high
assignee: jpsyx
labels: [enhancement, server]
estimate: 8
project: PROJ-1
milestone: MS-3
cycle:
parent:
github:
blocked_by: []
created: 2026-08-23
updated: 2026-08-25
---

# BR-15: Prove receiver job acceptance and progress

## Description

Give every remote run an opaque job token and observe whether that exact prompt
entered the frontend's conversation. Process spawn, PTY availability, typed
bytes, or screen activity alone must never count as prompt acceptance.

Expose one frontend-neutral observation contract through `AgentController`.
Each adapter may use its strongest reliable prompt-submit event, transcript,
rollout, or session evidence, but it must correlate evidence to the exact job
token and advance through a bounded cursor rather than repeatedly scanning an
entire growing history. Completion remains a separate token-matched fact.

## Acceptance criteria

- [ ] Every launch carries one unique job token in trusted launch metadata and
      in the exact prompt content that frontend evidence can correlate.
- [ ] `launched`, `accepted`, `progressing`, and `completed` are distinct
      observations with explicit persisted transitions.
- [ ] An exact prompt-submit or transcript event is required before a job is
      marked accepted.
- [ ] Claude, Codex, and OpenCode provide equivalent semantic observations
      behind `AgentController`; frontend storage grammar does not leak into the
      receiver coordinator.
- [ ] Observation is incremental and bounded for growing histories, and handles
      truncation, missing files, malformed entries, delayed writes, and session
      rotation conservatively.
- [ ] A valid token-matched completion may advance directly even when an
      intermediate observation was missed.
- [ ] Diagnostic records name the job and lifecycle boundary without logging
      prompt contents, provider credentials, or private response text.
- [ ] Red/green tests cover unobserved launch, exact acceptance, unrelated old
      messages, progress, malformed/ambiguous evidence, rotation, and missed
      intermediate observations without fixed sleeps.
- [ ] Agent, receiver, integration, architecture, decision, data-model, and
      testing docs describe the evidence contract.

## Notes

### Pointers (as of 2026-08-25)

- `src/agent/frontend.rs` and `src/agent/controller/mod.rs` are the semantic
  boundary for one frontend-neutral observation request and result. Keep all
  Claude, Codex, and OpenCode event grammar behind that facade.
- `src/tui/app_brain/receiver/{launch,active}.rs` own the exact launch and poll
  boundaries. The BR-14 coordinator currently moves to `launching` before
  spawn and completes from an exact response artifact, but has no post-spawn
  `launched` state or acceptance/progress observation.
- `src/state/receiver/{schema,job_state}.rs` and
  `src/state/receiver/store/{claim,completion}.rs` own the durable state and
  exact-owner transactions. Schema v8 has no job token, observation cursor, or
  evidence timestamps.
- `scripts/agent_session_{start,stop}_hook.py` and
  `scripts/opencode_brain_plugin.js` already normalize session and completion
  lifecycle facts. Extend these bridges with content-free observation facts;
  do not make the TUI parse a frontend transcript.
- Codex's documented `UserPromptSubmit` hook supplies the exact prompt,
  session ID, and turn ID before dispatch, while its documentation explicitly
  says the transcript file format is not stable. Prefer that supported event
  over rollout grammar for acceptance:
  <https://learn.chatgpt.com/docs/hooks#userpromptsubmit>.

### Plan (2026-08-25)

#### Global constraints

- Follow strict red, green, refactor TDD. Observe every focused failure before
  adding the production behavior it demands, and do not use fixed sleeps.
- Use one opaque random job token, distinct from the public job ID, for the
  complete durable job lifetime. Persist it before launch, put it in trusted
  launch metadata, and append one deterministic marker to the bounded initial
  prompt before command budgeting.
- A process spawn, session start, PTY snapshot, terminal bytes, tool event
  without a prior exact token match, or unrelated historical message must
  never establish acceptance.
- Normalize frontend lifecycle facts into a Brain-owned fixed-size observation
  snapshot. The snapshot may contain only opaque identity, lifecycle phase,
  session/turn identifiers, timestamps, and a monotonic revision. It must never
  contain prompt, tool input/output, response text, sender, recipient, or
  provider credentials.
- Keep the observation read behind `AgentController`. The receiver coordinator
  may handle only neutral phases and opaque cursors, never Claude JSONL, Codex
  rollout records, or OpenCode event names.
- Keep observations conservative. Missing, delayed, truncated, malformed,
  wrong-token, wrong-instance, wrong-session, or regressed evidence leaves the
  durable state unchanged. A valid exact completion may skip acceptance or
  progress without inventing either timestamp.
- Preserve BR-14 focus, session ownership, attachment, shutdown, and cleanup
  invariants. BR-16 still owns policy for an expired or stalled job after
  launch; BR-17 still owns durable answer/transcript persistence and delivery
  retries; BR-18 still owns final legacy-state removal.
- Add automatic receiver schema migration with both `up` and `down`, including
  deterministic reconciliation for existing v8 databases. Help and version
  must remain side-effect free.
- Update the agent, receiver, feature, integration, architecture, decision,
  data-model, and testing docs in the same product change. Because BR-15 is
  additive before 1.0, move the current 0.79.0 release to 0.80.0 and keep the
  lockfile synchronized.

#### Task 1: Persist opaque job identity and explicit observation state

Start with failing pure model, schema, migration, and store tests for a unique
job token, post-spawn `launched`, accepted, progressing, and completed facts,
plus an observation revision tied to one exact remote instance.

The green implementation must:

- Add a validated opaque `ReceiverJobToken` and generate it when ingress first
  creates a durable job. Existing v8 rows gain unique tokens during automatic
  migration without changing job IDs or payload bytes.
- Separate pre-spawn `launching` from post-spawn `launched`. Map the neutral
  `progressing` observation to the existing durable `processing` state, and
  persist launched, accepted, progressing, and completed timestamps without
  fabricating missed intermediate timestamps.
- Persist the current observation instance and bounded cursor/revision. A new
  pre-acceptance retry may reset only the prior run's cursor; accepted and later
  work stays outside BR-15 restart policy.
- Add exact-owner transactions for post-spawn launch commit and monotonic
  observation application. Reject stale owners, instances, sessions, cursors,
  phase regression, and expired claims without partial mutation.
- Extend completion so a token-matched terminal fact may commit from launched,
  accepted, or processing while atomically recording completed evidence.
- Provide schema-v9 `up` and `down` operations and reconciliation tests for
  fresh, v8, partially upgraded, damaged, and already-current state stores.

#### Task 2: Produce normalized evidence from every frontend lifecycle

Start with failing hook, plugin, installer, and health tests. Use synthetic
payloads only; no fixture may store real prompt or response content.

The green implementation must:

- Add one Brain-owned observation bridge with an atomic, owner-only,
  fixed-size snapshot protocol. Revision updates must be monotonic and safe
  across duplicate or concurrent hook delivery.
- Register `UserPromptSubmit` and `PostToolUse` for Claude and Codex through the
  registry-declared lifecycle installer. Acceptance requires an exact match
  between the trusted environment token and the marker in that submitted
  prompt. Progress requires a prior accepted snapshot for the same token,
  instance, and session.
- Extend the OpenCode plugin to recognize the exact marker from incremental
  user-message part events and record progress from its post-tool event. Keep
  only bounded per-run correlation state; do not fetch or rescan complete
  message history for acceptance or progress.
- Carry `BRAIN_RECEIVER_JOB_TOKEN` and the exact observation path only for
  receiver runs. Interactive and skill-session launches remain untracked and
  do not gain receiver evidence authority.
- Add the job token to the stop-hook completion artifact and OpenCode bridge.
  The completion file remains the separate source of private final text; the
  observation snapshot records only the completed boundary.
- Preserve unrelated user hooks and plugin behavior, update health checks for
  every managed artifact, and make startup reconciliation self-heal stale
  installed bridges.

#### Task 3: Add the bounded `AgentController` observation contract

Start with table-driven adapter contract tests for Claude, Codex, and OpenCode
using the same neutral request and expected result.

The green implementation must:

- Add opaque `AgentObservationCursor`, `AgentObservationPhase`, request, and
  result types. The public controller operation delegates to the selected
  frontend and returns only launched/accepted/progressing/completed semantics,
  exact session identity, and the next cursor.
- Read at most one fixed-size normalized snapshot per poll and enforce strict
  byte, field-count, identifier, and revision bounds before parsing. A cursor
  that already covers the revision returns no new fact in constant work.
- Treat missing and delayed files as pending; malformed, truncated, wrong-path,
  wrong-token, wrong-instance, wrong-session, revision-regressed, and ambiguous
  snapshots as conservative errors that reveal no private content.
- Prove session rotation explicitly. Evidence for the lifecycle-reported native
  session is eligible only when the exact remote instance currently owns that
  session; a placeholder or prior rotated session cannot advance the job.
- Add characterization and structural tests that fail if production receiver
  code names frontend types, transcript formats, rollout paths, OpenCode event
  names, or bypasses `AgentController` for observation.

#### Task 4: Wire polling into the durable receiver coordinator

Start with composed App tests for unobserved launch, exact acceptance,
unrelated old tokens, progress, delayed and malformed evidence, rotation,
missed intermediate observations, completion-first delivery, owner loss, and
arrival of a second FIFO job while the first is active.

The green implementation must:

- Append the exact marker before BR-14 prompt budgeting, attach trusted token
  metadata, launch the isolated controller, then atomically move `launching` to
  `launched` only after the exact controller/tab exists and ownership is still
  current.
- On each active tick, renew the exact claim, resolve the lifecycle-reported
  current session, ask that tab's controller for the next observation, and
  apply the result through one fresh-time exact-owner transaction.
- Persist accepted and processing only from normalized token-matched evidence.
  Apply multiple phases from one newer snapshot atomically and advance the
  cursor once, so a missed poll cannot expose a half-applied lifecycle.
- Validate the response artifact's job token in addition to the existing
  workspace, actor, channel, frontend, instance, session, and response
  identities. A valid completion may finish directly from launched or accepted
  while leaving unobserved intermediate timestamps empty.
- Keep child exit or orderly shutdown after `launched` conservative when no
  terminal evidence exists. Clean local resources, retain durable facts, and
  leave stalled-work policy to BR-16 instead of silently replaying a possibly
  accepted prompt.
- Emit content-free diagnostics naming only opaque job/instance identifiers,
  frontend, prior phase, observed boundary, and stable error category.

#### Task 5: Close parity, privacy, migration, and documentation gaps

Add end-to-end characterization for all three frontends and architecture guards
before removing any temporary seams. Cover duplicate hook delivery, event
reordering, observation-file replacement, cursor saturation, token collision
rejection, bounded marker overhead, process restart, and current-main feature
regressions.

Update the documented lifecycle and `BRAIN_*` environment contract, schema-v9
data model, fixed-size evidence protocol, explicit state mapping, privacy
boundary, and BR-16/17/18 exclusions. Finish with:

- a privacy review proving no private body enters observations, diagnostics,
  test names, snapshots, or debug formatting;
- `cargo fmt --all --check`;
- focused release tests for state, migrations, hooks/plugins, adapters,
  controller observation, and composed receiver behavior;
- `cargo test --release -- --test-threads=1`;
- `cargo clippy --release --all-targets -- -D warnings`;
- structural no-bypass, no-fixed-sleep, unsafe-code, em-dash, and diff checks.

Commit each reviewed SDD task independently, then require one most-capable
whole-branch acceptance review before integration.

### Self-review (2026-08-25)

- The plan uses a supported exact submit event for Codex instead of its
  unstable transcript wire format. Claude uses the equivalent managed hook,
  while OpenCode normalizes its own incremental plugin events. The coordinator
  never guesses which frontend it is observing.
- The fixed-size snapshot is the bounded cursor. It avoids a growing event log
  and makes every poll constant work, while the monotonic revision preserves
  delayed-event and missed-poll behavior.
- `launching` and `launched` are deliberately separate. BR-14's pre-spawn retry
  remains safe, but post-spawn ambiguity is no longer mislabeled as a failed
  launch. BR-16 decides how to recover a stale launched, accepted, or processing
  job after Brain or the machine disappears.
- Completion remains independent. It may skip accepted/progressing if their
  hooks were delayed or missed, but it cannot synthesize those observations and
  must match the exact opaque token plus the existing lineage.
- A tool event cannot establish acceptance. The observation producer ignores
  progress until the same token, instance, and session already has exact submit
  evidence.
- The token is correlation identity, not authentication. Durable claim owner,
  remote instance, current locked session, immutable actor/workspace scope, and
  fresh transaction time remain the authorization boundary.
- Prompt budgeting happens after marker insertion, so the new correlation
  requirement cannot revive BR-14's shell-argument overflow. Debug output and
  diagnostics redact the prompt and token value.
- BR-17 is not pulled forward. BR-15 adds a completed observation and token to
  the current completion artifact, but does not yet make the answer/transcript
  write or provider delivery independently durable.

### Log

- 2026-08-23 created from PROJ-1 planning and absorbs BR-10's exact-submission
  observation requirement.
- 2026-08-25 unblocked after BR-14 shipped isolated durable tabs. Current-code
  review replaced stale transcript and PTY pointers with a token-matched,
  fixed-size lifecycle evidence protocol and a five-task TDD plan.

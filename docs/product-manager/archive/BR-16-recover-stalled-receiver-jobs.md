---
id: BR-16
title: Recover stalled receiver jobs without blocking the queue
status: done
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
updated: 2026-08-27
---

# BR-16: Recover stalled receiver jobs without blocking the queue

## Description

Add a recurring durable reconciler that turns lifecycle observations into
bounded recovery decisions. A job never observed as accepted can be terminated
and safely requeued. A job proven accepted but no longer progressing receives
one automatic recovery attempt by launching a new process that resumes the same
logical session and names the same job. A second failure records a terminal
failure, notifies the sender, and releases the queue for later work.

Use persisted claim and progress leases so the same rules reconcile jobs after
a TUI, shared server, agent process, or machine restart. Event delivery may make
the common path immediate, but polling must recover missed events and stale
ownership.

## Acceptance criteria

- [x] Launch, acceptance, progress, and recovery leases are explicit,
      persisted, and evaluated through pure decisions with injected clocks.
- [x] A launch that never proves acceptance is terminated and requeued without
      consuming its one accepted-job recovery attempt.
- [x] A job proven accepted but stalled receives exactly one automatic recovery
      attempt in the same logical/native session when resumable.
- [x] Recovery instructions name the same job and ask the resumed conversation
      to reconcile prior work rather than blindly repeat side effects.
- [x] A second accepted-job failure records a terminal failure, sends the
      sender a clear unavailable response, and allows the next job to run.
- [x] Process exit, missing evidence, corrupt native history, and TUI or machine
      restart use deterministic recovery rules rather than leaving an active
      claim indefinitely.
- [x] Startup reconciles stale claims and answer/delivery states before claiming
      new work.
- [x] Progress-renewed leases have an absolute upper bound so continuously
      ambiguous state cannot block the queue forever.
- [x] Red/green tests cover every timeout boundary, missed event, restart point,
      safe requeue, one recovery, terminal failure, sender notice, and queue
      advancement without wall-clock sleeps.
- [x] BR-10's old screen-based inactivity watchdog is removed or narrowed to
      non-authoritative diagnostics, with applicable docs updated.

## Notes

### Pointers (as of 2026-08-26)

- `src/state/receiver/{model,schema,store}.rs` owns schema v9, persisted
  lifecycle evidence, claims, pre-acceptance retries, and the exact-owner
  transactions that BR-16 must extend. The current claim lease can be renewed
  forever and is not a lifecycle deadline.
- `src/tui/app_brain/receiver/{dispatch,active,launch,resume,shutdown}.rs` owns
  recurring polling, process cleanup, and native resume validation. A fresh App
  currently fences expired launched or observed work but deliberately leaves it
  unchanged, which blocks the FIFO until this task adds recovery.
- `scripts/receiver_observation_bridge.py`,
  `scripts/opencode_brain_plugin.js`, and `src/agent/observation/` normalize one
  accepted boundary and one progressing boundary today. Repeated exact
  `PostToolUse` events must become bounded progress pulses before a progress
  lease can be renewed safely.
- `src/server/reply/mod.rs::unanswered_notice` and
  `src/tui/app_brain/receiver/reply.rs` already provide the channel-safe
  unavailable response. BR-16 needs a durable dispatch intent, while BR-17
  remains responsible for provider acknowledgement and general delivery retry.
- `docs/product-manager/archive/BR-10-*.md` records the superseded recovery
  proposal and the failure cases this task absorbs.

### Plan (2026-08-26)

#### Global constraints

- Follow strict red, green, refactor TDD. Every behavior starts with the
  smallest focused failure, uses injected Unix-millisecond clocks, and never
  waits on wall time.
- Keep ownership and lifecycle time separate. The 30-second claim lease only
  fences writers; persisted lifecycle deadlines decide whether work is stale,
  and claim renewal must never extend those deadlines.
- Use fixed initial policy values: two minutes for pre-spawn launch work, 90
  seconds from launch to exact acceptance, five minutes without exact progress,
  and 30 minutes as the absolute accepted-work limit. A progress pulse renews
  the five-minute lease but clamps it to the absolute limit. Evaluate every
  deadline at `now >= expires_at`.
- Preserve the existing three-attempt pre-acceptance budget. An unaccepted
  launched process consumes that budget and may be requeued, but never consumes
  the single accepted-job recovery attempt.
- Give accepted work at most one recovery launch. It must use the same frontend
  and an exact validated native session. If native history is absent, corrupt,
  locked by another live owner, or otherwise not resumable, fail closed instead
  of replaying side effects from portable context.
- Put all frontend operations behind `AgentController`. The policy and durable
  store may know only neutral observations, opaque identities, and semantic
  effects.
- Persist only content-free recovery facts and a pending unavailable-notice
  intent. The notice body remains a compiled channel-safe constant and provider
  acknowledgement or ambiguous delivery remains BR-17 work.
- Reconcile before ordinary FIFO claims on startup and every receiver tick.
  One immediate transaction must validate the complete stale snapshot and
  publish at most one semantic effect, so concurrent TUIs cannot both recover
  or fail the same job.
- Add automatic receiver schema migration v10 with both `up` and `down`, plus
  deterministic repair for partial or damaged upgrades. Help and version stay
  side-effect free.
- Update feature, architecture, integration, data-model, decision, and testing
  docs. The first additive product commit moves 0.83.9 to 0.84.0; every later
  product commit follows the repository version-bump rule.

#### Task 1: Model explicit lifecycle leases and pure recovery decisions

Start with failing table-driven policy and state tests for every state, exact
deadline boundary, future-skewed evidence, saturated arithmetic, and separate
pre-acceptance and accepted-recovery budgets.

The green implementation must:

- Add a pure `ReceiverRecoverySnapshot -> ReceiverRecoveryDecision` seam with
  semantic decisions for wait, safe pre-acceptance requeue, same-session
  recovery, terminal failure, and incomplete legacy completion state.
- Persist launch, acceptance, progress, recovery, and absolute-work expiries,
  the latest exact progress pulse, recovery count, current attempt kind, and a
  pending unavailable-notice bit. Keep first accepted/progress facts distinct
  from the current attempt's observation cursor so a new recovery instance can
  start at revision zero without erasing lifetime evidence.
- Establish the launch lease when an ordinary or recovery run is claimed,
  establish acceptance at the exact post-spawn launch commit, and establish
  progress plus the immutable absolute limit only after exact acceptance.
- Represent recovery as its own persisted attempt, not as an increment of the
  pre-acceptance or future delivery retry counters. Preserve the job token,
  conversation, immutable inbound response identity, and first lifecycle facts.
- Add v10 `up` and `down` operations. Upgrade valid v9 active rows
  conservatively from their stored evidence and timestamps; ambiguous active
  rows receive a finite immediately reconcilable deadline rather than an
  unbounded lease. Downgrade every new ambiguous attempt to a non-replayable
  terminal v9 state.

#### Task 2: Turn repeated frontend activity into bounded progress pulses

Start with failing bridge, plugin, installer, adapter, cursor, and privacy tests
for a second and later token-matched `PostToolUse` event.

The green implementation must:

- Extend the fixed-size normalized observation snapshot with a monotonic latest
  progress timestamp while retaining the first accepted and first progressing
  boundaries. Duplicate or reordered pulses must be idempotent or rejected.
- Let Claude, Codex, and OpenCode publish repeated progress only after exact
  acceptance for the same token, instance, and native session. Subagents,
  unrelated turns, wrong tokens, and prior sessions remain ineligible.
- Extend `AgentController`'s neutral observation result and opaque cursor so one
  bounded read can return a newer progress pulse even when no new lifecycle
  phase exists. Frontend event names and snapshot grammar remain hidden from the
  receiver coordinator.
- Apply a pulse through an exact-owner, exact-instance, exact-session,
  monotonic-revision transaction. Use fresh authorization time to renew the
  durable progress lease and clamp it to the absolute limit; producer time is
  retained only as evidence and cannot grant extra runtime.
- Keep snapshots content-free and at most 4096 bytes, preserve owner-only atomic
  replacement, self-heal installed hooks/plugins, and cover revision
  saturation without disabling terminal completion.

#### Task 3: Add atomic recurring reconciliation and bounded transitions

Start with failing store tests for expired live and expired-owner rows, exact
CAS races, restart reopen points, FIFO blocking, notice intent, and every
decision returned by the pure policy.

The green implementation must:

- Scan the oldest blocking nonterminal job before any ordinary claim, build its
  validated recovery snapshot, evaluate the pure policy, and apply the decision
  in the same immediate transaction only if every state, owner, instance,
  attempt, deadline, and observation revision still matches.
- On an unaccepted timeout, clear the stale run registration and observation
  cursor, release ownership, schedule the existing bounded pre-acceptance retry,
  and keep the accepted-recovery count unchanged. Exhaustion becomes terminal.
- On the first accepted stall, preserve lifetime evidence and native binding,
  create one due recovery attempt with a deadline capped by the absolute limit,
  and release the old owner. A second stall, absolute expiry, or unsafe resume
  condition becomes terminal with a pending unavailable notice.
- Fence late observations, completions, renewals, and process exits from every
  superseded instance. A valid exact completion that commits before the stale
  transaction wins; a stale transaction that wins prevents the old run from
  committing later.
- Deterministically terminalize v9-era `answer-ready` or `delivering` rows that
  have no durable answer representation, then leave durable answer and delivery
  retries to BR-17. Terminal rows never block the next FIFO job.

#### Task 4: Launch one same-session recovery and execute reconciler effects

Start with composed App tests for live timeout cleanup, TUI and machine restart,
missed events, process exit, unavailable history, recovery acceptance and
progress, a second stall, sender notice, and next-job launch.

The green implementation must:

- Run reconciliation before restart controls and ordinary claims on every
  enabled receiver tick. When the winning effect names a local tab, shut down
  that exact controller, release its registration, remove only its files, and
  let durable fencing handle absent tabs after restart.
- Claim a due recovery ahead of later FIFO work and require
  `AgentController::resume_candidate_exists` plus exact registration of the
  persisted native session. Never fall back to a fresh portable session for an
  accepted job.
- Build a bounded recovery-only initial prompt that names the same opaque job,
  tells the resumed conversation to inspect prior work and avoid repeating
  completed side effects, requests completion of the pending response, and ends
  with the existing exact job-token marker. Do not resend the original message
  body as a new instruction.
- Give the recovery launch a fresh remote instance and observation file while
  preserving the logical conversation, job token, first evidence, and native
  session. Apply the same 90-second acceptance and five-minute progress rules
  within the unchanged absolute limit.
- Persist terminal failure and unavailable-notice intent before provider IO.
  Dispatch the existing immutable SMS or email notice, record successful
  handoff with an exact CAS, and retry a missed handoff after restart. Do not
  wait for or infer provider acknowledgement in BR-16.
- Release the queue independently from local cleanup or notice handoff, while
  keeping content-free diagnostics for job, attempt, boundary, and stable
  reason only.

#### Task 5: Close restart, privacy, architecture, and documentation gaps

Add end-to-end characterization for Claude, Codex, and OpenCode before removing
temporary seams. Cover exact deadline minus one and equality, repeated pulses,
continuously active absolute expiry, claim expiry before and after lifecycle
expiry, duplicate reconcilers, failed cleanup, process restart at every durable
boundary, absent or corrupt native history, notice handoff races, and FIFO
advancement.

Update the lifecycle policy, schema-v10 data model, repeated observation pulse,
same-session recovery prompt, restart ordering, privacy boundary, and BR-17/18
exclusions. Remove any remaining receiver screen-activity or inactivity timeout
authority and add structural tests preventing its return. Finish with:

- a privacy review proving no prompt, answer, transcript, sender, recipient, or
  provider credential enters observation, recovery metadata, diagnostics, or
  debug formatting;
- `cargo fmt --all -- --check`;
- focused release tests for policy, schema/migrations, store transactions,
  hooks/plugins, controller observations, and composed receiver recovery;
- `cargo test --release -- --test-threads=1`;
- `cargo clippy --release --all-targets -- -D warnings`;
- structural no-bypass, no-fixed-sleep, unsafe-code, em-dash, module-shape,
  privacy, version, and diff checks.

Commit each reviewed SDD task independently, then require one most-capable
whole-branch acceptance review before integration.

### Self-review (2026-08-26)

- The original pointers referenced the removed injection runtime and
  screen-inactivity watchdog. The revised plan starts from the BR-15 durable
  coordinator and explicitly prevents terminal activity from becoming
  authoritative again.
- BR-15 records only the first progressing boundary. A renewable progress lease
  would otherwise be based on silence, so Task 2 first adds repeated exact
  progress pulses with constant-size evidence and bounded reads.
- Claim expiry is only a writer fence. The plan gives lifecycle deadlines their
  own persisted fields, prevents claim renewal from extending them, and uses an
  absolute cap to stop endless ambiguous progress.
- Pre-acceptance retry and accepted recovery are deliberately separate budgets.
  A missing submit event can requeue safely under the existing three-attempt
  cap, while a proven accepted job gets exactly one cautious same-session
  recovery.
- Accepted recovery never uses a fresh transcript fallback. Without exact
  native history Brain cannot know which side effects already happened, so a
  clear terminal failure is safer than replaying the authenticated message.
- A new recovery instance needs a new cursor without erasing the first accepted
  facts. The plan therefore separates lifetime evidence from current-attempt
  cursor fields instead of resetting BR-15 history in place.
- The unavailable notice gets a minimal durable dispatch intent because a crash
  between terminal state and notification would violate BR-16. Provider
  acknowledgement, idempotency, ambiguous delivery, completed answer storage,
  and general response retry remain BR-17 responsibilities.
- Reconciliation runs before claims and uses one compare-and-swap transaction.
  This resolves both fresh-App recovery and the race where exact completion and
  timeout occur at the same instant without allowing duplicate recovery.
- Legacy `answer-ready` and `delivering` states cannot be safely resumed before
  BR-17 because no durable answer exists. Terminalizing only those incomplete
  rows is deterministic and keeps BR-17's answer-ready recovery path distinct.
- The fixed timeout values are policy constants, not user-facing TUI state, so
  they do not introduce a CLI or command-palette parity requirement.

### Log

- 2026-08-23 created from PROJ-1 planning and absorbs BR-10's bounded recovery
  requirements.
- 2026-08-26 unblocked after BR-15 shipped exact token-matched lifecycle
  evidence. Current-code review replaced stale injection pointers with a
  five-task plan for explicit leases, repeated progress pulses, atomic
  reconciliation, one same-session recovery, and durable failure-notice intent.
- 2026-08-27 completed in Brain 0.84.22 and merged to `main` as `bb4584a`.
  Receiver lifecycle deadlines are durable and bounded, exact progress renews
  only within the absolute limit, one accepted stall can resume its exact
  native session, unsafe or repeated recovery fails closed, unavailable notice
  intent survives restart, and the FIFO advances independently from cleanup.
  The final integrated rereview reported no Critical, Important, or Minor
  findings across recovery authority, migrations, all-frontends parity,
  privacy, architecture, and BR-17/18 scope boundaries.

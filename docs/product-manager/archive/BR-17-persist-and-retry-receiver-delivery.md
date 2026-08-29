---
id: BR-17
title: Persist and retry receiver response delivery separately
status: done
priority: high
assignee: jpsyx
labels: [enhancement, server]
estimate: 5
project: PROJ-1
milestone: MS-3
cycle:
parent:
github:
blocked_by: []
created: 2026-08-23
updated: 2026-08-29
---

# BR-17: Persist and retry receiver response delivery separately

## Description

Persist the completed agent answer and transcript update before attempting an
SMS or email response. Provider delivery is a separate retryable phase whose
failure never reruns the agent. Restart reconciliation must resume delivery
from the recorded answer and preserve the job's acceptance-time sender,
recipient, subject, lineage, and authorization context.

## Acceptance criteria

- [x] A token-matched completion atomically records answer readiness and the
      conversation transcript before provider delivery begins.
- [x] SMS and email delivery use the job's immutable acceptance-time response
      identity and cannot widen recipients after later config changes.
- [x] Provider failure records retry state and never returns the job to agent
      processing.
- [x] Restart during answer recording, delivery, provider acknowledgement, or
      final state commit reconciles without losing an answer or deliberately
      issuing duplicate agent work.
- [x] Delivery retries are bounded and idempotent where provider identifiers
      permit; ambiguous acknowledgement is recorded and surfaced explicitly.
- [x] A terminal delivery failure notifies through any remaining safe channel
      when possible, records diagnostics, and allows later jobs to proceed.
- [x] Email subject/message lineage and allowed thread participants remain
      intact; SMS formatting and length behavior remain unchanged.
- [x] Red/green tests cover response persistence, provider retry, crash points,
      authorization changes, duplicate acknowledgements, and terminal failure.
- [x] Response, transcript, integration, architecture, data-model, feature,
      decision, and testing docs describe the phase boundary.

## Notes

### Pointers (as of 2026-08-27)

- Historical pointer, superseded by Task 2: exact completion previously moved
  directly to `done` and called the process-local reply worker. It now commits
  answer readiness, transcript advancement, immutable delivery data, and
  durable cleanup authority atomically before post-commit cleanup.
- `src/tui/app_brain/receiver/artifact.rs` validates token, session, response,
  frontend, workspace, actor, channel, and completed status, but reads an
  unbounded content-bearing file. Keep the exact identity gate and add a finite
  read and answer bound before durable storage.
- `src/server/delivery.rs` queues opaque `Result<()>` work and discards provider
  response bodies. It must expose a typed, redacted result that distinguishes
  acknowledged provider IDs, definite rejection, safe retry, and ambiguous
  acknowledgement without moving credentials into the database.
- `src/tui/app_brain/receiver/reply.rs` renders final SMS and email payloads
  from immutable `InboundJob` fields. Move that pure shaping into the durable
  delivery intent so retries cannot reread config or change recipients,
  subject, lineage, SMS truncation, plain text, or HTML.
- `src/tui/app_brain/receiver/notice.rs` and
  `src/state/receiver/store/unavailable_notice.rs` provide a minimal durable
  handoff for BR-16 terminal notices. Fold this into the general delivery lane
  so a local queue acknowledgement is no longer mistaken for provider success.
- Resend documents a 24-hour `Idempotency-Key` contract for `POST /emails` and
  returns a stable email ID. Twilio returns a Message SID from a successful
  create but documents no equivalent create idempotency key. Retry and
  ambiguity policy must preserve that provider difference behind one typed
  delivery interface.

### Plan (2026-08-27)

#### Global constraints

- Follow strict red, green, refactor TDD. Every state transition, retry
  deadline, crash boundary, and provider classification starts with a focused
  failing test and an injected Unix-millisecond clock. Tests never wait for
  wall time or call a real provider.
- Add one durable receiver-delivery outbox owned by the workspace state DB.
  Answer content, portable transcript advancement, immutable rendered payload,
  provider attempt state, and job phase change must cross explicit SQLite
  transaction boundaries. Provider credentials remain machine-local and are
  read only by the delivery adapter at call time.
- Keep agent execution and provider delivery as independent lanes. Recording an
  answer releases the agent claim and removes the receiver tab. A delivery
  retry can never transition a job back to `claimed`, `launching`, `launched`,
  `accepted`, or `processing`, and it cannot block the next queued agent job.
- Persist a stable delivery ID and byte-identical rendered envelope before IO.
  Email retries reuse that ID as Resend's idempotency key while it remains
  within the documented 24-hour window. Twilio retries only failures proven to
  occur before provider acceptance; transport or commit uncertainty becomes an
  explicit terminal ambiguous acknowledgement rather than a possible duplicate
  SMS.
- Use one initial provider attempt plus at most three retries at one minute,
  five minutes, and 30 minutes, all computed with saturating arithmetic. A
  retry is due at `now >= retry_at`. Permanent validation, authorization, or
  credential failures terminate immediately. Provider-specific policy may
  shorten this budget but may never extend it.
- Persist the exact acceptance-time response destination and answer-time
  rendered body. Later user, env, config, receiving-address, formatter, or
  thread changes cannot widen recipients or change the payload under an
  existing delivery ID. An empty trusted email recipient set records a terminal
  authorization failure and performs no provider IO.
- Bound completion artifact reads and stored answer size at 256 KiB. Reject
  oversized, truncated, malformed, blank, or identity-mismatched artifacts
  without logging content. Portable transcript rendering remains deterministic
  and append-once; existing prompt-side suffix bounding continues to control
  recovery context size.
- Add automatic receiver schema v12 migration with `up`, `down`, idempotent
  repair, concurrent-writer tests, and privacy-preserving downgrade rules.
  Help and version remain side-effect free. The first additive product commit
  moves 0.84.22 to 0.85.0; later product commits follow the repository version
  rule.
- Keep all Claude, Codex, and OpenCode completion capture behind the existing
  `AgentController` and exact completion artifact contract. The delivery lane
  is frontend-neutral and receives only an authorized answer plus immutable job
  identity.
- Update architecture, features, integrations, data model, decisions, testing,
  and any affected glossary or config text. No delivery content, sender,
  recipient, provider credential, transcript, or response body may enter Debug,
  diagnostics, logs, status summaries, or assertion failures.

#### Task 1: Model durable answers, immutable delivery envelopes, and policy

Start with failing pure model, renderer, policy, schema, migration, downgrade,
and privacy tests.

The green implementation must:

- Add redacted types for a stable `ReceiverDeliveryId`, semantic response kind,
  immutable SMS or email envelope, delivery state, exact attempt identity,
  acknowledged provider reference, retry metadata, ambiguity reason, and
  content-free public status.
- Store each answer in one `receiver_deliveries` row uniquely tied to its job.
  The row contains the stable delivery ID, job token, response kind, serialized
  immutable envelope, attempt state and count, retry time, finite claim,
  first-attempt time, provider reference, redacted error category, and created
  and updated times. Raw credentials and mutable user/config snapshots are
  forbidden.
- Add a pure response-intent renderer that derives recipients and reply lineage
  only from the accepted `InboundJob`, then freezes channel-shaped SMS or email
  text and HTML. Preserve `SMS_LIMIT`, Markdown stripping, subject prefixing,
  `In-Reply-To`, `References`, and the exact accepted recipient set.
- Add a pure delivery decision over provider capability, attempt count, first
  attempt time, result class, and current time. Distinguish acknowledged,
  definitely not accepted and retryable, permanently rejected, and ambiguous.
  Resend ambiguity may retry with the same envelope and key within 24 hours;
  Twilio ambiguity becomes explicit terminal ambiguity.
- Add schema v12 `up`, repair, and `down`. Upgrade existing jobs with no answer
  row unchanged. Downgrade acknowledged deliveries to `done`; downgrade ready,
  retrying, delivering, failed, or ambiguous deliveries to non-replayable
  terminal jobs while retaining the portable transcript and deleting the v12
  outbox only after the old schema is valid.

#### Task 2: Atomically record exact completion and advance the transcript

Start with failing store and composed App tests for exact identity, append-once
transcripts, duplicate artifacts, concurrent completion, store failure, and a
crash immediately before and after commit.

The green implementation must:

- Replace direct `done` completion with one immediate transaction that validates
  the exact live owner, unexpired claim, job token, conversation, instance,
  native session, registration, lifecycle observation, and current processing
  state.
- In that transaction, render the new portable transcript from the prior
  transcript, immutable authenticated inbound prompt, and authorized assistant
  answer; insert the unique answer and delivery row; replace the native binding;
  merge exact lifecycle evidence; transition the job to `answer-ready`; and
  release the agent claim. No provider work can begin before commit.
- Make repeated completion artifacts idempotent by job and delivery identity.
  A byte-identical replay returns the existing answer-ready outcome without
  appending transcript text again. A conflicting answer, token, session,
  response instance, actor, workspace, channel, or envelope fails closed.
- Remove the exact receiver tab, release registration, clean artifacts, trigger
  completion sync, and allow the next ordinary queue claim only after the
  durable answer transaction wins. Cleanup or sync failure cannot erase or
  rerun the answer.
- Replace the unbounded artifact reader with owner-only, regular-file,
  no-symlink, exact-size-bounded snapshot validation. The artifact message is
  held only long enough to commit and is never copied into diagnostics.

#### Task 3: Add the separately claimed provider delivery worker

Start with failing adapter, store, and App-service tests for every result class,
claim race, expiry, restart, stale result, queue saturation, and immutable
payload retry.

The green implementation must:

- Claim the oldest due `answer-ready` or retrying delivery with its own finite
  writer lease and exact attempt ID, independent of the agent FIFO claim. Move
  the job and outbox to delivering in the same transaction and return a
  content-redacting delivery command.
- Replace fire-and-forget `Result<()>` dispatch with a bounded background
  executor that returns one typed provider result to the TUI. Test seams inject
  deterministic results; production provider calls remain off the render and
  event-loop threads.
- Parse and validate Resend email IDs and Twilio Message SIDs from bounded
  response bodies. Supply the stable Resend `Idempotency-Key`. Classify local
  preflight failures, HTTP rejection, timeout, malformed success, process exit,
  and result-channel loss without exposing provider response bodies.
- Apply each provider result through one exact workspace, job, token, delivery,
  attempt, owner, and unexpired-lease compare-and-swap. Acknowledgement stores
  the provider reference and marks delivery and job done. A safe retry records
  `retrying` plus its deadline. Permanent or ambiguous results record terminal
  failure details and let later jobs continue.
- Ignore stale or duplicate results idempotently. Claim expiry after a process
  or machine restart is reconciled from durable attempt facts: Resend may
  replay safely with its key inside 24 hours; Twilio becomes ambiguous unless
  durable evidence proves provider IO never began.

#### Task 4: Unify notices, fallback, restart reconciliation, and visibility

Start with failing composed tests for BR-16 unavailable notices, restart and
new-session acknowledgements, terminal final-answer failure, safe fallback
selection, no available fallback, restart at every delivery phase, and agent
queue advancement during delivery retry.

The green implementation must:

- Route all receiver-owned outbound replies through the same durable intent and
  provider-result seam. Convert BR-16's pending unavailable-notice bit and local
  handoff acknowledgement into outbox rows, and preserve transactional control
  command semantics while making their acknowledgement delivery durable.
- Add a pure fallback planner that can use only alternate destinations already
  authenticated and frozen at acceptance, excludes the failed provider and all
  already-attempted recipients, and emits at most one short channel-safe notice.
  Current single-channel jobs usually have no alternate, in which case Brain
  records that fact and never consults changed config to invent one.
- Reconcile delivery work before and after ordinary receiver dispatch on each
  enabled tick and on a fresh App. Answer-ready and retrying deliveries do not
  block new agent work; exact active delivery claims cannot be duplicated by
  concurrent TUIs.
- Expose redacted queue depth and delivery phase through existing receiver
  status and diagnostic models without printing answer, transcript, sender,
  recipient, payload, provider response, or credential data. Use stable reason
  codes for retry exhaustion, permanent rejection, and ambiguous
  acknowledgement.
- Remove the obsolete background reply APIs and minimal notice handoff state
  only after every call site uses the durable outbox. Keep provider formatting
  and credential access in their existing server ownership layer.

#### Task 5: Close crash, all-frontends, privacy, and documentation gaps

Add final characterization for Claude, Codex, and OpenCode before removing test
seams. Cover crash before answer commit, after answer commit, before provider
spawn, during provider IO, after provider acknowledgement, before final commit,
after final commit, duplicate completion, duplicate acknowledgement, Resend
same-key replay, Twilio ambiguity, retry exhaustion, concurrent claims, config
and user changes, no trusted email recipients, email threading, SMS limits,
sync failure, cleanup failure, and next-job progress.

Update the response phase boundary, schema-v12 data model, provider capability
differences, immutable envelope, retry schedule, ambiguity policy, fallback
limits, restart ordering, status vocabulary, privacy boundary, and BR-18 cutover
assumptions. Finish with:

- a privacy review proving content-bearing artifact, answer, transcript, and
  envelope fields remain inside owner-only files or SQLite rows and every
  formatter, error, log, and diagnostic is redacted;
- `cargo fmt --all -- --check`;
- focused release tests for model/policy, schema migrations, atomic completion,
  outbox claims, provider adapters, App reconciliation, control replies, and
  all-frontends completion capture;
- `cargo test --release -- --test-threads=1`;
- `cargo clippy --release --all-targets -- -D warnings`;
- structural no-bypass, no-fire-and-forget-reply, no-fixed-sleep, unsafe-code,
  em-dash, module-shape, privacy, version, and diff checks.

Commit each reviewed SDD task independently, then require one fresh whole-branch
acceptance review before integration.

### Self-review (2026-08-27)

- The original plan assumed the final reply and transcript already had separate
  files and stores. Current code proves the opposite: exact completion marks
  `done`, does not update the portable transcript, and only queues provider IO.
  Task 2 therefore makes the SQLite transaction, not the local worker queue, the
  answer-readiness boundary.
- Persisting only the answer would let retries rederive recipients and content
  after configuration or formatter changes. The revised plan freezes the
  rendered SMS or email envelope with a stable delivery ID before any provider
  call, which also keeps Resend idempotent payloads byte-identical.
- Provider calls do not have equivalent replay safety. Resend's documented
  24-hour idempotency key supports bounded safe replay. Twilio supplies a
  Message SID only after a successful create and exposes no matching create
  idempotency key, so transport or commit uncertainty must be visible as
  ambiguous instead of retried blindly.
- A delivery claim cannot prove whether provider IO began across every crash
  boundary. The plan records exact attempt identity and start facts, then uses
  provider capability to reconcile expired claims conservatively. This
  preserves the answer and avoids deliberate duplicate agent work even where
  exactly-once external delivery is impossible.
- The existing job enum already names answer-ready, delivering, and retrying,
  but the FIFO currently treats unfinished work as one lane. The plan explicitly
  releases agent ownership at answer commit and makes delivery claims
  independent so a provider outage cannot block later prompts.
- BR-16's unavailable notice records only local queue handoff. Leaving it
  outside the general outbox would retain the same acknowledgement hole BR-17
  is intended to close, so Task 4 migrates notices and control replies rather
  than building a final-answer-only exception.
- Safe fallback cannot come from mutable user or env configuration without
  widening authority after acceptance. The fallback planner therefore consumes
  only frozen alternate identities. With today's single-channel inbound model,
  no fallback is normally available, and explicit diagnostics are safer than
  retrying the failed provider recursively.
- The portable transcript must include each authenticated user turn and exact
  assistant answer once. Transactional append and a unique delivery row prevent
  duplicate completion artifacts from duplicating context. Prompt-side suffix
  truncation already bounds recovery input without destroying the durable
  transcript.
- Completion artifact content was exact but unbounded. The 256 KiB read and
  answer limit closes that resource boundary before new durable content is
  accepted while remaining far above SMS and normal email responses.
- Retry policy constants affect background delivery behavior, not toggleable
  live TUI configuration, so they do not introduce a CLI and command-palette
  parity requirement.

### Log

- 2026-08-23 created from PROJ-1 planning.
- 2026-08-27 unblocked after BR-16 shipped. Current-code and provider-contract
  review replaced stale file pointers with a five-task plan for atomic answer
  and transcript persistence, immutable delivery envelopes, a separately
  claimed provider outbox, bounded provider-aware retries, explicit ambiguity,
  durable notices, and redacted restart visibility.
- 2026-08-29 completed and merged to `main` in `5db5b3d`. Brain 0.85.28
  persists answers and transcripts atomically, delivers immutable responses
  through a separately claimed provider outbox, reconciles every durable phase,
  and retains fail-closed cleanup authority across crashes and replacement
  races. The final serial release suite passed 3,409 tests, strict Clippy
  passed, and the installed binary was verified at 0.85.28.

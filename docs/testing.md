# Testing

> **Iron law:** No production code lands without a failing test written
> first. See [AGENTS.md](../AGENTS.md) → "Red/Green TDD" for the contract.

## Red/Green/Refactor — the loop we actually follow

1. **RED.** Write the smallest test that captures the next behavior.
   Run it. Watch it fail (compile error or assertion). A test you never
   saw fail proves nothing.
2. **GREEN.** Write the simplest production code that turns *that* test
   green. Don't add behavior the current red test doesn't demand. Don't
   widen the test surface between red and green.
3. **REFACTOR.** With the bar green, clean up names / structure / dupes.
   Re-run to stay green.

Then the next red. The point of red-first isn't ceremony: it's that a test
which has never failed isn't known to test anything. If a bug ships, the
first move is a failing test that reproduces it, *then* the fix.

## What we test

- **Pure decision logic.** `paths::parse_config_root` (every JSON
  branch), `paths::expand_tilde_with_home`, `open_target::is_textlike`
  (text vs blob, case-insensitivity, extensionless), `finder_target`,
  `open_target::is_markdown` (strictly `.md`) and `pdf_output_path`
  (colocated same-stem `.pdf`).
- **Workspace CLI decisions.** Clap and binary tests cover every placement of
  the raw `--brain/-b` selector, including after delegated task positionals,
  the long equals form, the `--` terminator, and duplicate/missing-value errors,
  plus the complete optional-value management grammar.
  `workspace::command::mutate` tests tilde/relative root normalization and each
  registry-only decision, including a removal variant with no filesystem path
  or delete operation. Hermetic CLI tests force first/later create persistence
  failures and assert exact registry preservation plus preservation of every
  invocation-created root-chain path with actionable manual cleanup. A
  filesystem release barrier starts multiple compiled-binary writers together
  and asserts every successful registration survives. Store tests cover typed
  lock timeouts plus abrupt-exit recovery of a zero-length schema-free lock
  artifact. Provision seams inject partial creation and a replacement path,
  then assert no created directory is removed, only invocation-created paths
  are listed, and the original error remains recoverable through its source
  chain.
  Duplicate same-record aliases are checked case-folded with byte preservation.
  `workspace::command::prompt` tests which missing values require `/dev/tty`,
  plus EOF cancellation, required-value retry, optional blank names,
  multi-answer collection, and contextual read/write failures through injected
  `BufRead`/`Write` seams. Registry-only preflight tests snapshot legacy env and
  pointer bytes plus the complete root/config trees across create/attach EOF,
  prove migration is never called after cancellation, and prove complete flag
  forms perform no terminal IO. The real `/dev/tty` opener stays thin.
- **Workspace bootstrap and readiness.** An exhaustive pure invocation table
  proves every route is `None`, `InternalNoPrompt`, `RegistryOnly`,
  `ReadOnlyWorkspace`, or `ReadyWorkspace`. Manifest tests reject unknown
  fields and schema/version
  incompatibility, and UUID disagreement. Injected `BufRead`/`Write` tests
  prove interactive repair persists and continues the original command, while
  compiled headless tests prove the exact repair commands and that no terminal
  prompt is attempted. Create/attach tests cover manifest ordering/adoption,
  invalid and colliding identities, registry-byte preservation, and manifest
  survival when later registry persistence fails. The first-create regression
  explicitly proves that create leaves `local_user_id` empty and the next
  ordinary command is the setup boundary.
- **Workspace runtime isolation.** `tests/workspace_runtime_isolation.rs`
  is a small integration entry point with cohesive `portable_stores`,
  `runtime_state`, and `env_identity` modules plus shared fixture support. It
  constructs two records and contexts under one temporary home, then performs
  real selected writes through env/config/personalization, session response,
  state DB, TUI and sync locks, sync Journal, Reporter current state/log, and
  CSV-baseline APIs. It snapshots the peer registry record plus portable and
  runtime file trees byte-for-byte, repeats selected writes after changing the
  default, and tests same-name/different-UUID rejection. Focused tests also pin
  alias-to-canonical detached sync arguments after alias removal/default change,
  the typed actor integration env, selected reindex `BRAIN_ROOT`, and
  request-UUID habits GET/POST isolation.
- **Sync runtime path contract.** `tests/sync_workspace_paths.rs` directly
  separates every sync artifact for two fixed UUIDs, holds both workspace locks
  concurrently while rejecting a second same-UUID acquire, and proves journal
  rows and current state are invisible through the peer workspace's paths.
  The gated real-rclone helper in `tests/sync_local.rs` uses the production
  UUID-derived workdir and reporter paths, so transport coverage cannot drift
  back to an adjacent test-only cache.
- **Remote sync identity contract.** `sync::identity` unit tests exhaust the
  pure absent, matching, mismatched, malformed, and incompatible decisions and
  an injected rclone boundary pins probe/publication/read-back ordering.
  `tests/sync_workspace_identity.rs` composes two selected records, proves
  cross-workspace refusal before every data command for sync, repair, and
  check, and uses a gated local-rclone remote to verify exact setup publication
  and read-back. `tests/sync_local.rs` starts from matching manifests so the
  transport suite exercises the production gate.
- **Workspace documentation contract.** `tests/workspace_docs.rs` runs the
  compiled binary's root, workspace, and nested alias help, then checks only
  stable command names and selector spellings against the current README/docs.
  It also pins the registry, portable manifest, and UUID-cache locations;
  rejects command-like instructions that write structural `root` through
  `brain env`; and requires the prompt-based/non-sandbox disclaimer plus the
  invariant that changing the default workspace never changes access mode. It
  deliberately avoids snapshots and punctuation-heavy prose.
- **Hook integration.** `tests/hook_integration.rs` plus its focused
  `hook_integration/{atomic,installer}.rs` modules run the real Python
  SessionStart hook against a temporary SQLite DB and the real shell installer
  against temporary homes/roots. They cover the typed workspace/actor
  identity plus session attribution contract, selected-root argument and
  `BRAIN_ROOT` precedence, project-relative commands, actor-scoped Claude and
  Codex rotation, atomic target-claim serialization, rollback and retry after
  an injected mutation failure, equal opaque IDs with conflicting immutable
  attribution, schema-v2 row preservation, and malformed/ambient no-op
  behavior. The hook-installer
  unit tests live in `src/command/server/receiver/hooks/tests.rs`; they pin the
  exact installed Codex JSON command schema, execute the
  actual configured start and stop commands as one attributed lifecycle, and
  proves stale deployed scripts are refreshed. A regression test runs the real
  Claude Stop hook on a payload with **no** `last_assistant_message` but a
  `transcript_path` present and proves it still publishes the response artifact
  by recovering the final assistant text from the transcript — delivery must
  not hinge on that one optional field. Further unit tests prove locked
  concurrent JSON mutations retain both workspace registrations and unrelated
  settings, always leave parseable JSON, and preserve original bytes when an
  atomic replacement fails. TUI setup tests prove a held workspace singleton
  prevents hook refresh.
  `tests/stop_hook_actor.rs` proves the stable response ID and actor/channel
  completion contract for a Codex-style `thread_id` payload. It also pauses a
  real Stop hook after payload parsing, rotates the same live Claude lineage
  through the real SessionStart hook, and proves the stale completion is
  rejected after serialization. A deterministic publication-failure fixture
  proves both Claude and Codex retain `active` state and leave no staged file.
- **The config store (`settings/vars.rs`).** Schema resolution against an explicit
  map (defaults vs overrides — never the real store), the `config list` table
  layout and coloring, value coercion (`4`→number), name normalization, the
  `markdown-to-pdf` prerequisite message wording, and mining an executable path
  out of shell output (a function-wrapper body, ignoring terminal noise).
- **The fuzzy picker's brains.** `HaystackBuf` slug normalization and the
  char→byte highlight map, `char_positions_to_byte_positions`, `refilter`
  (empty query keeps all; substring across stripped slugs; no-hit yields
  no matches; highlight bytes recorded), section grouping in
  `build_display_rows`, and navigation clamping (`move_*`, `page_*`,
  `selected_path`).
- **Menu navigation.** `handle_key` as a pure state machine: movement
  saturation, ctrl-jk, digit jump in/out of range, Enter confirm, Esc /
  Ctrl-c cancel. Plus structural guards: the layout toggle is the last row,
  and every `Choice` appears exactly once (including `CreatePdf` when a
  markdown target is present). Also the two contextual rows: "Create PDF"
  appears only with a `.md` target, leads the list, and carries `^G`;
  "Delete" appears with any target, **trails** the list, and carries `^D`.
  Both elide a long filename through the shared `truncate_label_filename` /
  `LABEL_MAX_FILENAME` (`create_pdf_label` and `delete_label` line up).
- **The confirmation modal** (`confirm.rs`). `handle_key` as a pure state
  machine: PDF is Yes-by-default so Enter accepts while Delete is No-by-default
  so a stray Enter cancels, toggling flips it, `y`/`n` answer directly,
  `Esc`/`Ctrl-c` cancel; plus each `ConfirmKind` carries its own accent
  (green/red), title, and question, the selected button carries the accent
  fill, and the modal shows just the file name.
- **The picker's confirm wiring** (`picker/selection.rs`). `open_confirm` /
  `open_delete_confirm` raise the PDF modal on a `.md` selection (no-op
  otherwise) and the Delete modal on any selection; confirming converts a PDF
  in place or trashes and `drop_path`s the entry (the shell stays open), and
  `reload_entries` / `drop_path` keep the query while updating the list.
- **The palette layout label** (`menu/model.rs`). `layout_choice_label` names the
  opposite side; the toggle row is searchable and appears exactly once.
- **Render helpers.** That `entry_line` preserves the full text, coalesces
  a highlighted run into one correctly-colored span, and paints the
  selection background; that headers/empty-states carry the right text.
- **Filesystem collection** (integration). `entry::collect` against real
  temp trees: bucket tagging, `~/brain/...` rewriting, hidden-file
  skipping, root-skipping, tolerance of a missing bucket.
- **The session store** (`state.rs`, in-memory SQLite). Scoped resume
  ordering, exact composite-scope `claim` win/lose, registration + `release`
  round-trip, `completed` to `active` reactivation for both frontends, exact
  composite-scope `reap_dead_locks` with an injected pid-liveness predicate,
  preservation of equal opaque IDs across scopes, the
  two-shells-take-distinct-sessions invariant, and `panel_side` round-trip +
  flip. `open_in_memory` / `with_pid_alive` are the test seams
  (deterministic clock + injectable pid probe), so no real process or wall
  clock is involved.
- **Launch builders** (`session.rs`). `AgentKind`, `Plan::decide` (resume vs
  fresh), `build_llm_command` (Claude `--resume`/`--session-id`, Codex `resume`
  and no Claude flags for fresh launches, shell-quoting, and typed rejection of
  blank compatibility-plan session IDs), and `env_for`. The command matrix
  lives here once; the integration characterization suite keeps only its real
  hook and environment boundaries.
- **OpenCode fail-fast smoke boundary.** `tests/opencode_smoke.rs` covers
  `--open-code`, normalized `-oc`, the typed mutually exclusive selection
  error and exact plain rendering, early process rejection, direct adapter
  rejection, and every controller lifecycle/input/terminal-control rejection
  against an instrumented transport with no side effects. Env units cover the
  reserved command value. No test launches OpenCode.
- **Portable advisory access policy.** `tests/workspace_access_policy.rs`
  proves first/later create and attach defaults, valid-v2 upgrade seeding,
  strict typed status, trusted config mutation, and default-switch byte
  preservation. Access-store unit tests prove malformed-byte preservation and
  live-file continuity, temporary cleanup, and successful retry across an
  injected pre-replace interruption.
  `tests/access_boundary.rs` pins the exact non-sandbox prompt fragments,
  unrestricted absence, immutable inbound separation, all actor/session/triage
  contexts, honest themed status, and the deliberately bypassable literal-path
  warning. `tests/agent_access_adapter.rs` proves Claude system-prompt and Codex
  developer-instruction installation, selected cwd, the explicit minimal
  environment, and real shell argv termination for option-looking prompts.
  App-level controller tests capture the actual fresh/resumed main-panel,
  authenticated SMS/email, and triage launch specs for both frontends, including
  exact trusted policy, cwd, separate prompt, actor, and channel. A nested-process PTY test proves unrelated inherited workspace
  secrets do not reach the child after `env_clear`; a temporary-HOME profile
  regression proves the non-profile shell cannot recreate a filtered secret.
- **Workspace capabilities.** `tests/workspace_capabilities.rs` separates
  portable logical selection from selected-record machine material, pins the
  missing-versus-empty skill defaults, normalized/invalid logical names,
  malformed transport data, unavailable credentials, and skill sources. It
  verifies Claude's owner-only strict MCP JSON and conservative direct-command
  evidence, Codex's secret-free documented per-call overrides against the
  installed parser, collision-free stdio secret remapping, honest enforcement
  reports, exact symlink-free actor/root-local skill rendering without
  global-registry mutation, canonical machine-source containment, parent-link
  retarget rejection, lifecycle cleanup, safe symlink unlinking, cache-root and
  actor-ancestor sentinel preservation, and redacted status/Debug output.
  Setup-seam tests prove unrestricted startup does not parse unused malformed
  capability lists for either frontend while mode/live fields and all
  workspace-only capability fields stay strict. App-level tests prove
  unrestricted launch assembly does not parse unused malformed capability data
  and both workspace-only main and triage requests attach the same plan.
  Controller unit tests exercise the complete access-mode/capability-plan
  matrix and prove only unrestricted-without-plan and matching
  workspace-only-with-plan reach frontend or transport work. App launch tests
  also prove malformed capability configuration leaves a free resumable
  session unclaimed and clears the attempted response identity.
- **The new-tab opener** (`open_target.rs`). `edit_shell_command` (cd +
  editor, quoting) and `iterm_new_tab_applescript` (embeds the command,
  escapes `"`/`\`).
- **The brain shell's pure bits** (`tui/`). `startup_focus` (the shell
  lands in the search panel at startup), `focus_left`/`focus_right`
  (focus follows the layout swap), `panel_borders` (the right panel owns the
  divider), and `key_to_bytes` (non-semantic key → terminal byte encoding).
  Recording frontend/transport tests under `tui/app_brain/tests/` cover
  `AgentController` and its App consumers: failed fresh registration prevents
  launch, Enter calls semantic submit and reactivates the scoped store row,
  injected work queues and reactivates after a controller-owned two-tick delay,
  `Ctrl-N` targets the effective main or triage tab, shutdown fires once, and
  agent exit closes only the panel. It also proves half-page scroll targets the
  visible triage controller and whole-shell teardown explicitly shuts down both
  controllers. The actual `App::open_triage_tab` path uses
  the selected adapter, includes only ephemeral hook metadata, and creates no
  session row. Prelaunch validation tests prove capability and response
  identity errors happen before a resumable claim and clear the attempted
  response identity. Fallback completion captures the transport snapshot with
  the controller's initiating actor/channel before teardown.
- **Receiver dispatch state.** `tui/receiver_state.rs` proves that an idle
  open panel switches to queued receiver work, an active submitted turn waits,
  a same-channel warm panel is reused, a different channel replaces it, and a
  warm receiver lease never hides interactive Stop-hook completion. Failed
  launches retain their message and retry backoff deadlines are honored.
  `sync/freshness.rs` tests the strict two-hour message threshold;
  `sync/journal.rs` proves push-only/aborted rows do not refresh it.
  `server/delivery.rs` verifies that provider delivery is dispatched off the
  TUI thread. The app receiver tests mutate `users.json` and the machine
  registry after queue acceptance, then prove Claude and Codex both launch
  through `AgentController` with the captured actor, channel, response email,
  and allowed participant recipients.
- **Receiver admission and workspace isolation.**
  `tests/receiver_workspace_isolation.rs` uses focused fixture and model
  support modules with deadline-bounded polling and no fixed sleeps. It drives
  signed SMS through the real shared process and exact live TUI socket. A
  second real-process fixture registers two live workspaces with distinct
  credentials and actors for the same normalized sender, rejects a deliberately
  cross-signed ingress, then proves each request enters only its exact socket.
  The suite also rejects an unknown sender and cross-workspace frames, verifies
  the 1 MiB body cap, and proves disabled or missing targets return one
  channel-specific unavailable response while another workspace keeps the
  process alive. It also covers absent-process silence, failed sockets, a full
  64-job queue,
  enqueue acknowledgment, and rollback when the acknowledgment write fails.
  The rollback case performs the complete frame, `prepared`, `commit`, and
  peer-close sequence, then polls the production socket and requires an empty
  queue. A signed Resend event submitted while its exact TUI is unavailable is
  replayed after re-registration; the replay must remain outside the queue and
  must not reach the Receiving API.
  Pure tests under `server/receiver/dispatch/tests/` pin the synchronized late
  revocation boundary separately from transactional provider-ID state: failed
  handoffs retain no ID, in-flight duplicates are not
  acknowledged, successful duplicates are idempotent, and the 1024-entry cache
  is scoped by workspace and channel. Barrier-driven dispatch tests disable
  exact live authority after actor resolution and prove revalidation prevents
  any socket handoff. Typed provider tests preserve ignored-event 202 and
  upstream 502 outcomes. Injected Resend fetch tests cap both provider
  responses, and a counting reader proves only one proof byte beyond the limit
  is consumed.
- **Workspace-specific receiver setup.**
  `tests/receiver_setup_workspace.rs` runs provider-free, local CLI fixtures
  against two selected workspaces. It proves distinct Twilio and Resend secrets
  remain in their exact machine records, numeric-looking secrets remain strings,
  output contains the stable `/w/<ingress>/<channel>` URLs without secret
  values, and setup writes portable user mappings without rotating either
  manifest. It also proves channel-specific address requirements and ingress
  stability across rename, alias, default, and a second-machine attach. The
  command-owner test records that setup and set both notify only the selected
  UUID through the existing-process reload seam. Security regressions spawn the
  real binary with separate and assignment-style secret/address arguments,
  then inspect both the mode-`0600` run log and `--verbose` output. Validation
  cases cover supplied and existing selected-record values, malformed public
  origins, both provider sender forms, guided clearing, conditional channels,
  and redacted failures. Injected failures after the provider write, users
  write, and every Claude/Codex hook write prove exact byte restoration, peer
  preservation, rollback-error aggregation, and no live reload after failure.
- **Shared-server lease state.** `server/lifecycle/table.rs` uses injected
  monotonic instants and timing values to prove that different workspace leases
  coexist, duplicate live workspace, ingress, or lease identities fail, an
  already-expired incoming lease is rejected, heartbeats renew only their
  matching lease, expiry cannot expose stale routing data, and final orderly or
  expired removal requests shutdown. Late heartbeat/update errors and a
  rejected registration also preserve a latched final-expiry shutdown for the
  next watchdog tick. It drives every `LeaseTable::apply` action branch
  directly. The suite also proves
  that a previously registered ingress stays distinguishable as `NoLiveTui`
  after its lease is gone, rather than becoming `Unknown`. These tests have no
  process, socket, filesystem, or sleep dependency.
- **Shared-server process lifecycle.** `tests/server_lifecycle.rs` exercises
  the compiled binary under a temporary home. Pure startup tests cover live
  reuse, stale cleanup, one elected starter, and losing contenders. Process
  tests prove the hidden loop rejects a missing generation token, two distinct
  workspace leases coexist, the first orderly unregister leaves the process
  reachable, the final unregister exits and removes generation artifacts, and
  SIGTERM takes the same guarded cleanup path. A Unix-socket startup gate
  synchronizes SIGTERM immediately after state publication, and an occupied
  HTTP port proves the pre-publication cleanup owner removes an already-bound
  control socket. Barrier-driven unit races prove stale reaping and child
  adoption exclude contenders through exact identity transfer. A synchronized
  pre-adoption child-loss race proves the parent retains exact cleanup until
  adoption. A barrier-held advisory mutex proves that cleanup survives brief
  contention after child loss through its explicit bounded operation, while
  the adoption race proves that cleanup cannot remove the child's transferred
  token. Controlled unreadable and malformed token artifacts prove that both
  cleanup inspections propagate errors, preserve the artifact, and allow the
  same handoff value to remove a restored exact parent token on retry. An
  elected child that never receives its first registration exits within its
  bounded bootstrap deadline. Two fake TUI clients also register distinct
  workspaces, observe a deliberately killed generation, enter recovery together
  through injected heartbeat clocks and an explicit barrier, converge on one
  replacement generation, re-register, and drive final-unregister cleanup. A
  second barrier-driven race removes the final old lease after discovery but
  before startup registration and proves the bounded handshake elects and
  registers with the replacement. All external process observation uses bounded
  condition polling.
  `server/lifecycle/watchdog.rs` injects expiry and bootstrap instants directly,
  so a crashed final lease remains live immediately before TTL, requests
  shutdown exactly at TTL, and leaves an empty table without a timing sleep.
  The no-first-registration decision uses the same injected-clock boundary.
- **Opaque-ingress workspace routing.** `server/router.rs` exhaustively proves
  the exact method/component grammar for provider SMS/email and
  lease-capability local habits/triage endpoints, including query stripping
  and rejection of global, malformed, or
  extra-component paths. `tests/server_workspace_routing.rs` injects lease
  instants to prove only a live enabled lease resolves to its revalidated
  workspace context, while disabled, unknown, and known-without-live-TUI routes
  remain distinct. `tests/habits_workspace_routing.rs` drives the
  compiled shared process with two fake live TUIs and distinct manifests. It
  proves each ingress renders and mutates only its own habits, triage completion
  lands only in the selected UUID cache, unknown routes never fall back or emit
  provider acknowledgements, and disabling or removing one live lease leaves
  its peer routable. Focused sibling modules cover route isolation, body
  ordering, and CLI URL identity. Partial unknown and disabled POSTs prove the
  server responds before body completion while the control socket stays
  responsive; oversized accepted local actions prove the 16 KiB cap. A real
  `brain habits -b` call and TUI URL helpers prove a later manifest mismatch
  cannot replace the ingress accepted for the selected live registration.
  A channel-blocked context loader proves registry and manifest IO never holds
  the control mutex: snapshot, heartbeat, and unregister remain responsive,
  and the loaded ticket is rejected after unregister. Pure revision and
  blocked-route tests prove heartbeat renewal preserves a ticket, while
  disable/re-enable and identical same-ID unregister/re-register ABA
  transitions reject the pre-revocation ticket. Maximum-revision tests prove a
  staged job-socket races cover disable, unregister, and disable-enable ABA
  after final revalidation but before commit acknowledgment; every losing
  admission returns unavailable and leaves the TUI queue empty. Failed
  enablement update or receiver-changing registration replay leaves the
  whole lease table unchanged and cannot revive or extend authority.
  A synchronized test hook on the real `SharedReceiverPipeline` revokes
  authority after production final revalidation and authorization but before
  the staged socket commit. Disable, unregister, and disable-enable ABA each
  cancel the admission and leave the live TUI queue empty. Mutation coverage
  that removes authorized-state cancellation makes the real pipeline accept
  and enqueue, so the regression cannot pass through a copied test decision
  path.
  The same synchronized production race expires the exact lease through the
  watchdog/control seam after authorization and before commit. A committed
  admission held past an injected short control deadline proves unregister
  returns bounded rejection and cannot mutate authority later.
  A route lookup at exact expiry cannot consume the lease ahead of watchdog
  revocation, and an injected final-admission clock proves exact TTL rejects
  commit before the next watchdog tick. A second real-pipeline gate pauses
  after commit-side persisted-intent IO, advances the injected clock to exact
  expiry, and proves the real job socket remains empty. A third real-pipeline
  race holds the control mutex across that IO boundary, advances to exact
  expiry while commit waits for control, and proves the clock is sampled only
  after lock acquisition. Its commit probe requires both the COMMITTED state
  and the still-held mutex, making pre-lock clock and post-unlock CAS mutations
  fail.
  Exact status tests distinguish a
  live disabled lease from an accepting lease. Actual parsed CLI start/stop and
  startup `--with-receiver -b` paths, plus keyboard-driven tasks and search
  palettes, prove both persistence directions and exact-workspace refresh
  wiring. A failed injected refresh proves committed intent remains successful
  and visible with a warning.
  Fixed-worker admission tests hold four partial bodies, prove a fifth
  connection waits while control remains responsive, and open 24 incomplete
  request heads without increasing the server thread count. An injected
  second-worker spawn failure proves the start gate rolls back before any
  partially received body is consumed.
  Injected-clock request tests advance the parse deadline across successful
  head and body bytes and the later response, proving drip progress cannot
  renew it without relying on sleeps. Separate injected-clock cases prove an
  expired parse phase cannot be revived, the two-second handoff cutoff leaves
  the response reserve open, and synchronized expiry after provider work does
  not enter the job socket. Real Unix-stream step-clock tests advance the same
  handoff deadline between successful frame bytes and acknowledgment bytes,
  proving continuous progress cannot renew it. Parser tests reject
  conflicting or repeated framing, unsupported transfer codings, invalid
  field names, malformed chunk sizes, forbidden framing trailers, and bounded
  chunk/trailer violations while accepting the exact supported chunked form.
  Framing-value cases reject vertical tab, form feed, and non-ASCII whitespace
  on both `Content-Length` and `Transfer-Encoding`, while accepting `SP` and
  `HTAB` optional whitespace. Extension-bearing chunks remain an explicit
  unsupported safe-subset case.
  Server observation uses deadline-bounded polling; lifecycle decisions use
  injected clocks rather than fixed timing sleeps.
  Receiver setup lock tests inject both clock and poll behavior: a held lock is
  released only as the poll advances to the deadline, and a free lock starts
  with an already elapsed deadline. Both must return timeout without snapshot
  or mutation.
  The complete real-process E2E launches personal and family fake TUIs into one
  generation, accepts exactly one signed message into each exact queue,
  orderly-closes family while retaining observable recording history, proves
  one unavailable family request adds no family or cross-routed personal job,
  accepts a fresh personal message in the same generation, then closes personal
  and bounded-polls process exit plus generation-artifact removal. The test
  injects an extra cross-routed job into a copy of the history and proves its
  exact-route assertion rejects that mutation. It contains no fixed sleep.
- **Literal read-only status.** `tests/status_read_only.rs` runs the compiled
  `brain server status` and selected `brain receiver status` commands. It
  snapshots every file type, Unix mode, regular-file byte sequence and SHA-256,
  symlink target, and recursively traversed referent before and after. Referent
  traversal records cycles rather than following them forever. The suite covers
  absent and active servers, symlinked machine/workspace paths, eight concurrent
  status commands, live control failure, and generation replacement. Socket and
  other entries include Unix type, device, inode, ownership, size, link count,
  and modification/change times, so same-mode replacement is visible. Before
  spawning the eight active probes, the test snapshots every candidate run log;
  it retains every child PID and proves that each exact matching log subset is
  unchanged afterward, including pre-existing names from PID reuse. It also
  compares the exact control-socket identity, checks that absent-server probes
  create no server state, and pins all four receiver status rows. Mutation tests
  replace a socket with a same-mode socket and rewrite a same-size reused-PID
  log, proving both observers fail on the defects they guard. Exact lease-table
  tests prove both process and workspace status leave expired leases, authority revisions,
  and shutdown state untouched for the watchdog. This catches accidental
  migration, config initialization, users transaction locks, skill rendering,
  state-DB/render-stamp writes, election, control-error suppression, and status
  pruning.
- **Shared-server control protocol.** `tests/server_control.rs` is split into
  focused codec, registration, and transition suites. It covers bounded
  newline-delimited JSON round trips, malformed and oversized rejection,
  multiple-frame rejection through a real stream, half-close handling,
  authoritative root and manifest validation, cross-workspace and unbound
  job-socket rejection, heartbeat, receiver-enable update, unregister,
  non-sensitive snapshot, exact live-workspace ingress lookup, and stale
  generation refusal. The exact workspace-status request reports live lease
  receiver enablement without exposing peer workspace state. Deterministic real
  Unix-stream codec tests inject deadline observations between successful byte
  reads, successful byte writes, and flush, proving continuous progress cannot
  extend the total budget. A saturated real Unix listener proves the safe
  connector completes within a bound without a helper thread; an expired
  request deadline proves server-side job-listener validation uses that same
  connector. A simulated lost accepted response proves an exact registration
  retry succeeds while competing lease IDs and changed identities remain
  rejected. The pure heartbeat classifier proves both missing and stale
  generations enter recovery.
  Real elected-starter wrappers exit once before publication with the token
  both retained and removed, then exec the real binary. Both cases prove
  retained-`Child` observation re-enters election inside the original deadline,
  without fixed sleeps or PID zombie assumptions.
  Production election followed by SIGKILL retains no external child handle;
  heartbeat recovery proves the published-child waiter reaps retained- and
  removed-token cases before replacement election.
  Injected parent handoff cleanup failure after publication proves waiter
  ownership transfers first, the cleanup error remains visible, and later
  retained- and removed-token SIGKILL recovery still replaces the generation.
- **Automatic sync safety.** `sync/args.rs` proves watcher pushes use one-way,
  non-deleting copy arguments; CSV/counter tests prove push-only reconciliation
  does not write remote-only state locally. UUID collision tests prove stable
  winners, mirror-order convergence, idempotence, composite dependency
  and free-text `see_also` rewrites, URL/substr preservation,
  deleted-reference fallback, project reverse-link
  regeneration, whole-operation schema refusal, retryable metadata
  publication, and task/habit counter floors through the real allocator. The
  CSV integration regression verifies an unchanged second pass performs no remote write, and
  `sync/trigger.rs` verifies completed detached children are reaped.
  `sync/check.rs` separately proves schema-aware read-only identity, hybrid
  legacy compatibility, labeled baseline/local/remote parse refusal, themed
  warning output, and byte-stable refusal across every task-related store.
  `tests/watch_local.rs` exercises the real watcher callback in the default
  suite: macOS validates the one-second polling fallback, while other platforms
  use notify's recommended native backend.
- **PTY transport and scrollback** (`pty_pane/tests.rs`). Environment/profile
  isolation plus `scroll_up`/`scroll_down` enter and clamp scrollback. These
  spawn a tiny real PTY running `seq`; this is the one place we let a child
  process in because it is deterministic and sub-second.

## What we deliberately don't test

- **The interactive event loop.** `tui::run_tui` opens `/dev/tty`, toggles raw
  mode, pushes kitty flags, spawns the selected agent PTY, and runs the panel loop. We
  test the *pure* logic it calls (`handle_key`, `App::*`, `focus_*`,
  `panel_borders`, `key_to_bytes`, the render helpers); we don't drive a real
  terminal or a real Claude/Codex process.
- **Ratatui frame output.** We assert on the `Line`s we build, not on
  which cell ratatui painted them into.
- **`std::process::Command` / system `open` / `osascript`.** Spawning
  Finder, the editor tab, or the agent CLI is not a unit. We test the pure builders
  (`finder_target`, `edit_shell_command`, `iterm_new_tab_applescript`,
  `build_llm_command`), not the spawn.
- **Real agent-provider behavior.** The Rust suite executes the exact installed
  SessionStart and Stop commands against temporary roots and SQLite databases,
  but it does not launch a real Claude or Codex provider session. Provider event
  emission remains the frontend's documented contract, not behavior Brain can
  manufacture or verify in isolation.
- **Tautological defaults / getters.** `Bucket::Projects.label()` returns
  `"Projects"` — we keep one stability check, not a battery of getter
  tests.
- **"Does it compile" smoke tests.** `cargo build` covers that.

## Test layout

| Location | Scope |
| --- | --- |
| `src/<module>.rs` → `#[cfg(test)] mod tests` | Pure-function unit tests for that module's branches (paths, settings, config, open_target, picker, menu, confirm, render, session, entry). |
| `tests/entry_collect.rs` | `entry::collect` against real temp directory trees. |
| `tests/root_resolution.rs` | `parse_config_root` + `expand_tilde_with_home` composed the way `brain_root` relies on. |
| `tests/workspace_cli.rs` | Compiled-binary workspace registry behavior with isolated `HOME`, `XDG_CONFIG_HOME`, current directory, and roots: manifest-aware create/attach, persistence failures, record-preserving mutations, selector/validation errors, deterministic `NO_COLOR` list output, and non-destructive removal. |
| `tests/workspace_readiness.rs` | Exhaustive bootstrap policy, strict manifest validation, interactive/headless readiness, repair, and first-create-to-next-command flow. |
| `tests/workspace_registry_migration.rs` | Legacy flat-env conversion, exact backups, matching first manifest, idempotence, valid-v2 portable-policy upgrade, and persistence-failure preservation. |
| `tests/workspace_access_policy.rs` + `tests/access_boundary.rs` + `tests/agent_access_adapter.rs` + `tui::app_brain::tests` | Portable mode ownership/defaults, strict and atomic persistence, exact advisory contract, real App launch-context parity, adapter mechanisms, option-terminated prompt argv, selected cwd, honest typed status, naive warning limits, and minimal environment. |
| `tests/workspace_runtime_isolation.rs` + `tests/workspace_runtime_isolation/` | Two-workspace portable-store, env-identity, default-change, state, lock, response, and sync-runtime isolation, split by concern with shared fixture support. |
| `tests/sync_workspace_paths.rs` | Direct UUID separation for sync paths, concurrent cross-workspace locks, same-workspace serialization, journal reads, and current-state reads. |
| `tests/sync_workspace_identity.rs` | Pure and compiled-binary remote manifest identity decisions, fail-closed mutation ordering across sync/repair/check, two-record cross-adoption refusal, and gated real-rclone setup publication/read-back. |
| `tests/sync_local.rs` | Gated real-rclone transport plus local CSV merge coverage; the transport invokes rclone with the production UUID-derived bisync workdir and reporter paths. |
| `tests/workspace_docs.rs` | Stable clap-to-doc workspace commands, selector spellings, storage locations, obsolete root-write rejection, and honest access-language invariants. |
| `tests/phase2_acceptance.rs` | Hermetic composed acceptance fixtures for one portable person selected from two independent machine registries and authenticated inbound identity flowing through `ActorContext` into a real task-script assignment. |
| `tests/todo_script_mutators.rs` | Brain-owned task scripts, including selected-root `BRAIN_ROOT` propagation and isolated actor/workspace environment for every subprocess. |
| `tests/task_schema_migration.rs` + `tasks::schema::transaction_tests` | Temp-only inactive migration fixtures: workspace/kind-scoped deterministic UUIDv5, explicit last-legacy-sync and pre-existing durable-backup-base preconditions, exact durable portable backups, canonical/lexical backup-path separation, strict current-schema detection, row/display-ID preservation, byte-idempotent reruns, injected deep-directory and backup-file parent open/sync failures, immediate journal-temporary cleanup, and crash/failure recovery before and throughout replacement. |
| `tests/task_id_collision_merge.rs` + `sync::csv_merge`/`csv_sync`/`counters` tests | Temp-only and pure fixtures for UUID merge identity, name-aligned headers, deterministic display-ID collision winners/allocation, mirror-order and repeat convergence, pipe/comma `blocked_by`, production-format free-text `see_also` rewrites with URL and substring preservation, deleted-target fallback without marker leaks, project metadata reverse links, retry and local/remote error classification, strict/forward-compatible whole-operation schema policy, no-write refusal, and task/habit next-counter floors. |
| `tests/triage_habits_config.rs` + `tasks::triage_habits` tests | Temp-only managed-definition reconciliation, rename-stable marker identity, CLI/TUI/web mutation guards, strict malformed-config refusal, managed-only and unmanaged-carrier display-reference purge, authenticated workspace-bound journal recovery, interprocess ownership, transaction module-size guards, live-file continuity, fresh re-enable, startup/reindex/repair restoration, suppressed-alert post-sync refresh, palette-enable/startup-refresh interleaving, and injected crashes at internal publication and cleanup boundaries. |
| `skills/todo/scripts/tests/test_workspace_context.py` | Standalone Python subprocess coverage for selected-root-only writes, effective-actor assignment, explicit portable-membership validation, legacy and absent assignment-header migration, empty-CSV schema initialization, missing-context failure, UUIDv4 creation, UUID-preserving edits, fresh habit-occurrence identity with assignment/system-key retention, feature-gated managed triage completion, stale-snapshot refusal for CSV and project metadata, shared-owner serialization, protected removal, concurrent counter allocation, and managed-history garbage collection. |
| `tests/verbose_cli.rs` | End-to-end `--verbose` contract for the compiled binary: stdout mirroring, `/tmp` log-file creation, command/action breadcrumbs, and task CSV load/write logging. |

### Phase 2 migration and convergence matrix

Every row below uses temporary roots, registries, caches, databases, and CSVs.
No test reads or writes a real user workspace.

| Scenario | Evidence |
| --- | --- |
| The same portable person is selected on two simulated machines | `two_machine_registries_select_the_same_portable_person` attaches the same portable workspace from two isolated machine registries and selects the same portable user ID on both. No device-specific identity is generated. |
| Authenticated inbound identity drives default assignment | `authenticated_inbound_actor_drives_default_task_assignment` maps generic email and phone senders to a second workspace member, proves that identity overrides the local user, exports the immutable actor context, and creates real task rows assigned to the inbound actor. |
| Two machines independently create the same display ID | `tests/task_id_collision_merge.rs` gives distinct UUID rows the same display ID, swaps local/remote order, and repeats the merge to prove deterministic convergence. |
| Relationships survive display-ID reconciliation | `tests/task_id_collision_merge.rs` covers composite `blocked_by`, bounded free-text `see_also`, deleted-target fallback, and project metadata reverse links. |
| Legacy rows receive stable migration identity | `tests/task_schema_migration.rs` derives UUIDv5 from workspace UUID, CSV kind, and legacy display ID, then proves byte-idempotent fixture migration with exact backups. The migration interface remains inactive. |
| Disable purges managed history without false-positive loss | `tests/triage_habits_config.rs` removes managed definitions, open rows, completed history, and derived references while preserving same-named unmarked rows and unrelated transcripts. `tasks::triage_habits::purge` limits JSON edits to top-level `tasks[]`, preserves unrelated JSON/text bytes and ambiguous display references, and aborts on malformed JSON, invalid UTF-8, or traversal failures. |
| Re-enable starts fresh | `disabling_purges_every_managed_row_and_derived_reference_then_reenables_fresh` proves exactly two new open managed rows, new UUIDs, and no restored history. |

The suite does not claim a filesystem sandbox, a general prompt-injection
detector, coordinated task migration activation against a real workspace, or
functional OpenCode behavior. Shared HTTP receiver routing and exact TUI
job-forwarding are covered by the active Phase 4 integration suites.

`tests/*.rs` reach into the crate via `brain::module::Symbol` because
`src/lib.rs` re-exports the modules. A binary-only crate has no library to
link integration tests against, so `lib.rs` exists purely for that.

## Mocking strategy

We introduce a seam **only** when the real implementation crosses a
process / shell / terminal boundary, and even then we prefer pushing the
logic to a pure function over mocking:

- **No mock for the filesystem.** `entry::collect` runs against real temp
  dirs (`tempfile`). Real walkdir behavior (hidden filter, depth) is the
  spec.
- **No mock for the terminal.** Instead of mocking crossterm, the
  navigation/matching logic is pure (`handle_key`, `App`), and only the
  thin `run()` shell touches `/dev/tty`.
- **No mock for the config store.** `settings` schema resolution runs against
  an explicit in-memory map, never the real `<brain-root>/.config/config.json`.
  That's a value seam, not a mock.

**Production modules don't get test-only methods.** Setup helpers live in
the `#[cfg(test)]` block (unit) or inside the integration test file.

## Commands

```sh
# everything (fast — well under a second)
( cd path/to/brain && cargo test --release )

# one module's unit tests
cargo test --release picker::

# one integration file
cargo test --release --test entry_collect

# persistent receiver intent, exact-record isolation, and shared transition
cargo test --release --test receiver_enablement

# selected-record provider setup, users, URLs, and ingress stability
cargo test --release --test receiver_setup_workspace

# workspace documentation contract
cargo test --release --test workspace_docs

# lint clean (pedantic + nursery are on)
cargo clippy --release --all-targets

# verbose
cargo test --release -- --nocapture
```

## Adding a test the right way

1. RED first. Add the assertion, build, confirm the failure matches what
   you expect.
2. Implement to GREEN with the minimum code.
3. No new mock unless you cross a process / shell / terminal boundary —
   push the logic to a pure function and test that instead.
4. Setup helpers go under `#[cfg(test)]` or in the test file, never as
   `pub fn make_test_*` on a production module.
5. If the behavior is user-visible (a new key, menu item, or config
   variable), update the relevant `docs/` file in the same change.

Receiver enablement tests under `command/server/receiver/enablement/tests.rs`
keep persistent intent separate from live process state. They cover the shared
pure transition, exact selected-record mutation, stale identity rejection with
byte preservation, authoritative control refresh, reduced clap grammar, and
dynamic labels in both palette models. Lifecycle tests use injected instants or
bounded polling; they never depend on fixed sleeps.

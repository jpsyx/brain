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
  the raw `--workspace/-w` selector, including after delegated task positionals,
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
  detached expected-UUID propagation and bootstrap mismatch refusal,
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
- **Composed multi-workspace acceptance.**
  `tests/multi_workspace_acceptance.rs` is one orchestration-focused scenario
  over two temporary workspaces. It reuses the real schema-v2 registry,
  portable users, UUID caches, TUI and sync locks, shared-server leases,
  authenticated SMS routing, task script, UUID CSV merge, triage flag, access
  policy, capability resolution, and `AgentController` launch boundary. The
  only doubles are the provider request and agent transport at external
  process edges. Claude, Codex, and OpenCode produce selected-root launch specs;
  ratatui, PTYs, and live agent providers never start. Lifecycle waits use
  bounded polling or channels, not fixed sleeps.
- **Workspace documentation contract.** `tests/workspace_docs.rs` runs the
  compiled binary's root, workspace, and nested alias help, then checks only
  stable command names and selector spellings against the current README/docs.
  It also pins the registry, portable manifest, and UUID-cache locations;
  rejects command-like instructions that write structural `root` through
  `brain env`; and requires the prompt-based/non-sandbox disclaimer plus the
  invariant that changing the default workspace never changes access mode. It
  deliberately avoids snapshots and punctuation-heavy prose.
- **Lifecycle integration.** `tests/hook_integration.rs` plus its focused
  `hook_integration/{atomic,installer}.rs` modules run the real Python
  generic session-start bridge against a temporary SQLite DB and the real shell installer
  against temporary homes/roots. They cover the typed workspace/actor
  identity plus session attribution contract, selected-root argument and
  `BRAIN_ROOT` precedence, project-relative commands, actor-scoped Claude,
  Codex, and OpenCode rotation, atomic target-claim serialization, rollback and retry after
  an injected mutation failure, equal opaque IDs with conflicting immutable
  attribution, schema-v2 row preservation, and malformed/ambient no-op
  behavior. The hook-installer
  unit tests live in `src/command/server/receiver/hooks/tests.rs`; they pin the
  exact installed Codex JSON command schema, execute the
  actual configured start and completion commands as one attributed lifecycle, and
  proves stale deployed scripts are refreshed. A regression test runs the real
  compatibility Claude completion entry point on a payload with **no** `last_assistant_message` but a
  `transcript_path` present and proves it still publishes the response artifact
  by recovering the final assistant text from the transcript — delivery must
  not hinge on that one optional field. Further unit tests prove locked
  concurrent JSON mutations retain both workspace registrations and unrelated
  settings, always leave parseable JSON, and preserve original bytes when an
  atomic replacement fails. TUI setup tests prove a held workspace singleton
  prevents hook refresh.
  `tests/stop_hook_actor.rs` proves the stable response ID and actor/channel
  completion contract for a Codex-style `thread_id` payload. It also pauses a
  real turn-complete bridge after payload parsing, rotates the same live Claude lineage
  through the real session-start bridge, and proves the stale completion is
  rejected after serialization. A deterministic publication-failure fixture
  proves every registered frontend retains `active` state and leaves no staged file.
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
  round-trip, `completed` to `active` reactivation for every frontend, exact
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
- **OpenCode compatibility and acceptance boundaries.** `tests/opencode_smoke.rs`
  covers `--open-code`,
  normalized `-oc`, mutually exclusive selection, adapter command generation,
  trusted named-agent configuration, semantic input translation, session
  identity, and controller delegation. `tests/opencode_acceptance.rs` drives a
  real deterministic fake executable through `AgentController`, including
  exact argv/environment, fresh and validated-resume launch, literal text,
  immediate submit, native busy-turn follow-up, `/new`, and idempotent shutdown.
  `tests/opencode_plugin.rs` runs the actual plugin under a Bun/Node SDK harness,
  covering root/child/resumed event shapes, message selection, minimal
  environment, failure logging, repeated-idle deduplication, and the real
  Python bridges plus SQLite DB. `tests/opencode_compatibility_script.rs` and
  probe unit tests cover required CLI surfaces, generated config schema,
  plugin loading, timeout/output bounds, caching, and disposable HOME/XDG
  isolation. Installer and doctor tests cover idempotence, rollback, and exact
  stale plugin/bridge detection. The standalone-installer suite also starts
  from stale mixed Codex hooks, runs repair twice, and proves canonical generic
  hooks are installed exactly once while unrelated settings survive.
- **Portable advisory access policy.** `tests/workspace_access_policy.rs`
  proves first/later create and attach defaults, valid-v2 upgrade seeding,
  strict typed status, trusted config mutation, and default-switch byte
  preservation. Access-store unit tests prove malformed-byte preservation and
  live-file continuity, temporary cleanup, and successful retry across an
  injected pre-replace interruption.
  `tests/access_boundary.rs` pins the exact non-sandbox prompt fragments,
  unrestricted absence, immutable inbound separation, all actor/session/triage
  contexts, honest themed status, and the deliberately bypassable literal-path
  warning. `tests/agent_access_adapter.rs` proves Claude system-prompt, Codex
  developer-instruction, and OpenCode Brain-agent installation, selected cwd, the explicit minimal
  environment, and real shell argv termination for option-looking prompts.
  App-level controller tests capture the actual fresh/resumed main-panel,
  authenticated SMS/email, and triage launch specs for all frontends, including
  exact trusted policy, cwd, separate prompt, actor, and channel. A nested-process PTY test proves unrelated inherited workspace
  secrets do not reach the child after `env_clear`; a temporary-HOME profile
  regression proves the non-profile shell cannot recreate a filtered secret.
  Adapter environment unit tests prove only OpenCode receives ambient
  `OPENCODE_*` variables and that Brain's merged `OPENCODE_CONFIG_CONTENT`
  remains authoritative.
- **Workspace capabilities.** `tests/workspace_capabilities.rs` separates
  portable logical selection from selected-record machine material, pins the
  missing-versus-empty skill defaults, normalized/invalid logical names,
  malformed transport data, unavailable credentials, and skill sources. It
  verifies Claude's owner-only strict MCP JSON and conservative direct-command
  evidence, Codex's secret-free documented per-call overrides against the
  installed parser, OpenCode's inherited-config-preserving Brain layer and
  secret-free generated MCP schema, collision-free stdio secret remapping, honest enforcement
  reports, exact symlink-free actor/root-local skill rendering without
  global-registry mutation, canonical machine-source containment, parent-link
  retarget rejection, lifecycle cleanup, safe symlink unlinking, cache-root and
  actor-ancestor sentinel preservation, and redacted status/Debug output.
  Setup-seam tests prove unrestricted startup does not parse unused malformed
  capability lists for every frontend while mode/live fields and all
  workspace-only capability fields stay strict. App-level tests prove
  unrestricted launch assembly does not parse unused malformed capability data
  and both workspace-only main and triage requests attach the same plan.
  Controller unit tests exercise the complete access-mode/capability-plan
  matrix and prove only unrestricted-without-plan and matching
  workspace-only-with-plan reach frontend or transport work. App launch tests
  also prove malformed capability configuration leaves a free resumable
  session unclaimed and clears the attempted response identity.
- **Facade source boundary.** `tests/agent_registry_boundary.rs` rejects public
  concrete frontend modules, adapter traits, or adapter operation exports and
  guards shared call sites against direct frontend branching. Black-box
  integration tests launch all three frontends through `AgentController` and a
  recording transport.
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
  injected work uses the selected adapter's semantic busy-turn sequence,
  `Ctrl-N` targets the effective main or skill-session tab, shutdown fires once, and
  agent exit closes only the panel. It also proves half-page scroll targets the
  visible skill-session controller and whole-shell teardown explicitly shuts down
  every controller. The actual `App::open_skill_session` path uses
  the selected adapter, includes only ephemeral hook metadata, and creates no
  session row. Prelaunch validation tests prove capability and response
  identity errors happen before a resumable claim and clear the attempted
  response identity. Fallback completion captures the transport snapshot with
  the controller's initiating actor/channel before teardown.
- **Skill sessions** (`skill_session/`, `tui/app_skill_session.rs`,
  `tui/app_brain/tests/skill_session.rs`). The pure half is unit-tested directly:
  what the workspace offers (`available`: the builtin daily triage only while the
  check is enabled, then the parsed `skill_sessions` entries), what a running
  session withdraws (`runnable`), how a malformed definition degrades without
  renumbering its siblings' keys, the tab-strip / `Alt+<digit>` slot resolution
  (`resolve_active_tab`, `tab_order`, `tab_for_slot`), the interactive editor's
  routing and list arithmetic, and the signal's parse + close gate (including
  rejecting a token that isn't safe as a file name, since it arrives in a request
  body). Recording-transport App tests then prove the wiring: a configured session
  launches its own prompt (plus the appended completion protocol) under its own
  tab title, two sessions run as separate tabs and complete independently (only
  the tab whose token arrives closes, and its start row returns), a declared
  required output holds a tab open until it exists, and neither a stale signal from
  a dead shell nor a signal for another tab closes a freshly-opened one.
- **Receiver dispatch state.** `tui/receiver_state.rs` proves that an idle
  open panel switches to queued receiver work, an active submitted turn waits,
  a same-channel warm panel is reused, a different channel replaces it, and a
  warm receiver lease never hides interactive bridge completion. Failed
  launches retain their message and retry backoff deadlines are honored.
  `sync/freshness.rs` tests the strict two-hour message threshold;
  `sync/journal.rs` proves push-only/aborted rows do not refresh it.
  `server/delivery.rs` verifies that provider delivery is dispatched off the
  TUI thread. The app receiver tests mutate `users.json` and the machine
  registry after queue acceptance, then prove Claude, Codex, and OpenCode launch
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
  write, and every registry-declared lifecycle directory, hook-settings lock,
  and artifact write compare an exact recursive before/after tree. The snapshot
  includes bytes, modes, symlinks, directories, and pre-existing lock files,
  proving exact restoration, peer preservation, rollback-error aggregation,
  and no live reload after failure. Static-artifact regressions separately
  prove leaf and parent symlinks cannot escape the selected workspace, an
  external dangling chain creates no target, and an in-workspace symlink
  remains intact across idempotent installation. The standalone installer also
  rejects a symlinked plugin parent whose referent is outside the selected root.
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
  lease-capability local habits/session endpoints, including query stripping
  and rejection of global, malformed, or
  extra-component paths. `tests/server_workspace_routing.rs` injects lease
  instants to prove only a live enabled lease resolves to its revalidated
  workspace context, while disabled, unknown, and known-without-live-TUI routes
  remain distinct. `tests/habits_workspace_routing.rs` drives the
  compiled shared process with two fake live TUIs and distinct manifests. It
  proves each ingress renders and mutates only its own habits, skill-session completion
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
  `brain server status`, selected `brain receiver status`, `brain sync status`,
  `brain workspace list`, and `brain tasks doctor` commands. It
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
  pruning. Dedicated WAL fixtures also prove that sync status and tasks doctor
  do not checkpoint or otherwise mutate an existing SQLite database.
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
  CSV integration regression verifies an unchanged second pass performs no remote write.
  `tests/sync_trigger_workspace.rs` drives the injected detached-child and
  production lock boundaries: exact canonical argv/UUID environment,
  fail-closed compiled-binary bootstrap, concurrent different-workspace entry,
  and same-workspace coalescing/following use bounded channels without fixed
  sleeps. `sync/trigger.rs` verifies completed detached children are reaped.
  Injected receiver clocks/readers/runners deterministically prove the 250ms
  status poll, journal-advance gate, five-second retry grace, and three-attempt
  fallback. The clock-driven watcher-loop test proves stopping one workspace's
  watcher leaves its peer live; `tests/watch_local.rs` waits on callback
  channels rather than fixed sleeps.
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
  terminal or a live Claude/Codex/OpenCode provider process.
- **Ratatui frame output.** We assert on the `Line`s we build, not on
  which cell ratatui painted them into.
- **`std::process::Command` / system `open` / `osascript`.** Spawning
  Finder, the editor tab, or the agent CLI is not a unit. We test the pure builders
  (`finder_target`, `edit_shell_command`, `iterm_new_tab_applescript`,
  `build_llm_command`), not the spawn.
- **Real agent-provider behavior.** The Rust suite executes the exact installed
  lifecycle commands against temporary roots and SQLite databases, drives a
  deterministic fake OpenCode process, and loads the real OpenCode plugin in a
  fake SDK harness. It does not send a prompt to a live Claude, Codex, or
  OpenCode provider. Provider event emission remains the frontend's documented
  contract, not behavior Brain can manufacture or verify in isolation.
- **Tautological defaults / getters.** `Bucket::Projects.label()` returns
  `"Projects"` — we keep one stability check, not a battery of getter
  tests.
- **"Does it compile" smoke tests.** `cargo build` covers that.
- **Personal data in bundled skills.** Keeping identity, private paths, and
  private URLs out of `skills/` is a **review obligation**, not a test. A
  substring-matching guard would have to commit the very personal data it
  protects into this public repo — see "Why there is no automated personal-data
  guard test" in [decisions.md](decisions.md). Read the diff instead; if you
  want automation, keep it outside the repo (a local pre-commit hook or a
  private CI list).

## Test layout

| Location | Scope |
| --- | --- |
| `src/<module>.rs` → `#[cfg(test)] mod tests` | Pure-function unit tests for that module's branches (paths, settings, config, open_target, picker, menu, confirm, render, session, entry). |
| `tests/entry_collect.rs` | `entry::collect` against real temp directory trees. |
| `tests/root_resolution.rs` | `parse_config_root` + `expand_tilde_with_home` composed the way `brain_root` relies on. |
| `tests/receiver_url_cli.rs` + `command::server::receiver::url::tests` | Compiled-binary webhook-URL reporting with no server ever started: both channels by default, `--sms`/`--email` narrowing (`--sms --email` means all, not a conflict), `-w` selecting that workspace's own origin and ingress, a missing `brain_receiver_public_url` naming both ways to set it instead of printing a headless URL, and `receiver status` reporting the same rows. Pure tests cover channel selection, row rendering, and the trailing-slash normalization that would otherwise break provider signature verification. |
| `tests/root_creation.rs` | First-use workspace setup: a workspace registered on another machine has its root created, its manifest written from the registry UUID **when no sync is configured** (a configured remote's manifest is adopted instead — see `sync::identity::adopt`), and PARA + task/habit CSVs + counters seeded; a root under a *missing parent* is reported rather than invented; re-running rewrites nothing and leaves user content alone. The pure halves (`root_setup`, `startup_sync_direction`, `performs_setup_sync`) are unit-tested in `workspace::initialize`. |
| `tests/persona_cli.rs` + `personalization::{personas,store,command,onboarding}::tests` | Compiled-binary per-user personas: writing one member never disturbs another, `show`/`get`/`list` address the right person and mark the local one, a member with no entry still appears as `(unset)`, an unknown user ID is rejected by name, a schema-1 file migrates onto the reading machine's local user and rewrites keyed, and a headless command reports a missing persona on stderr without failing or skipping its own work. The pure halves (keyed parse/migration, roster block, prompt gate, notice wording) are unit tests. |
| `tests/env_cli.rs` + `env::breakdown::tests` + `env::render::tests` | Recursive dotted get/set, secret-free confirmations, `default_agent_frontend` canonicalization (`Open-Code` → `opencode`) plus rejection of an unknown frontend without disturbing the stored value, and the `brain env` breakdown: machine-global rows are exactly the non-`workspaces` top-level keys, every registered workspace gets a block resolved against its own root (never a peer's value or default), an undeclared global still lists, a non-selected workspace's secret renders `(set)`, and the pure renderer's section order, default/selected labels, shared name column, `(unset)`/`(empty)` states, and legend footnote. |
| `tests/workspace_cli.rs` | Compiled-binary workspace registry behavior with isolated `HOME`, `XDG_CONFIG_HOME`, current directory, and roots: manifest-aware create/attach, persistence failures, record-preserving mutations, selector/validation errors, deterministic `NO_COLOR` list output, and non-destructive removal. `workspace list` reports feature health for every registered workspace, `-w` narrows it to one, and a workspace still needing setup renders a note instead of failing the listing. |
| `tests/workspace_readiness.rs` | Exhaustive bootstrap policy, strict manifest validation, interactive/headless readiness, repair, and first-create-to-next-command flow. |
| `tests/workspace_requirements.rs` + `tests/workspace_requirements/` | Central required/optional matrix, fail-closed malformed sync/receiver fields, portable receiver mappings, advisory capability health, redaction, exact selected-record isolation, and focused shared fixtures. |
| `tests/status_read_only.rs` | Filesystem snapshots proving workspace list, sync status, receiver status, tasks doctor, and server status do not create or mutate machine/workspace state, including symlink and live-process cases. |
| `tests/workspace_registry_migration.rs` | Legacy flat-env conversion, exact backups, matching first manifest, idempotence, valid-registry portable-policy upgrade, and persistence-failure preservation. Plus the **schema v2 → v3 upgrade**: an ordinary command hoists `markdown_to_pdf_path` into the machine-global map and strips every record copy, several configured paths collapse onto the first canonical workspace, a machine that never set one gains no global map, the exact previous bytes are backed up once and a rerun is a no-op, every workspace then resolves the one value, and a read-only `workspace list` reads an old schema without rewriting it. The pure rewrite is unit-tested in `workspace::registry::upgrade`. |
| `tests/workspace_access_policy.rs` + `tests/access_boundary.rs` + `tests/agent_access_adapter.rs` + `tui::app_brain::tests` | Portable mode ownership/defaults, strict and atomic persistence, exact advisory contract, real App launch-context parity, adapter mechanisms, option-terminated prompt argv, selected cwd, honest typed status, naive warning limits, and minimal environment. |
| `tests/opencode_smoke.rs` + `tests/opencode_acceptance.rs` | Selector, command, session, semantic-input, facade, and deterministic fake-process acceptance for OpenCode without a live provider call. |
| `tests/opencode_plugin.rs` + `tests/fixtures/opencode/plugin_harness.js` | The real thin plugin under Bun/Node with fake SDK events, root-session filtering, completion extraction, failure logging, repeated-idle deduplication, and generic-bridge publication. |
| `tests/opencode_compatibility_script.rs` + `agent::opencode::probe` tests | Supported-feature probes, generated config and plugin loading, isolated HOME/XDG state, bounded execution, cache behavior, and the opt-in compatibility script. |
| `tests/workspace_runtime_isolation.rs` + `tests/workspace_runtime_isolation/` | Two-workspace portable-store, env-identity, default-change, state, lock, response, and sync-runtime isolation, split by concern with shared fixture support. |
| `tests/sync_workspace_paths.rs` | Direct UUID separation for sync paths, concurrent cross-workspace locks, same-workspace serialization, journal reads, and current-state reads. |
| `tests/sync_workspace_identity.rs` | Pure and compiled-binary remote manifest identity decisions, fail-closed mutation ordering across sync/repair/check, two-record cross-adoption refusal, absence of UUID workdir creation before identity, active-migration refusal before rclone, and gated real-rclone setup claim/publication/read-back. Unit barriers additionally prove two-phase late-claim election remains safe with a non-atomic canonical-copy fake. |
| `tests/sync_trigger_workspace.rs` | Exact detached canonical argv plus expected workspace UUID, compiled bootstrap mismatch refusal, injected child launch, concurrent cross-workspace lock entry, and same-workspace coalescing/following with bounded channels. |
| `tests/sync_local.rs` + `tests/sync_local/` | Gated real-rclone harness with focused transport, CSV merge, conflict, schema-transition, and multi-workspace modules; two concurrent local remotes use distinct production UUID-derived workdirs and CSV baselines, a mismatched remote manifest refuses before bisync, a wrong-typed present task schema refuses without publication, and a configured second legacy machine joins an already-current remote through the real coordinator, floors stale task/habit counters, allocates non-colliding IDs through the real mutators, then converges both machines and remote byte-for-byte. |
| `tests/watch_local.rs` | Real watcher callbacks over temporary personal and family roots; dropping one joined worker leaves the peer live, with channel deadlines instead of sleeps. |
| `tests/multi_workspace_acceptance.rs` + `tests/multi_workspace_acceptance/` | One hermetic personal-plus-family scenario covering selector/default policy, UUID caches and locks, one shared server, authenticated wife assignment through the real task script, deterministic display-ID reconciliation, disabled family triage, registered-frontend advisory capability parity, family unavailability, personal continuity, and final server shutdown. |
| `tests/workspace_docs.rs` | Stable clap-to-doc workspace commands, selector spellings, storage locations, obsolete root-write rejection, and honest access-language invariants. |
| `agent::codex::sessions` + `agent::codex::frontend_tests` | Codex resume validation against a temporary rollout tree: an exact trailing-segment id match, refusal of prefix collisions and of ids not following a `-`, unrelated filenames, a missing sessions directory, day-tree search including older days, and no descent past the day level. The frontend half proves the interactive and response-channel predicates agree, that no resolvable home means nothing is resumable, and that a validated id becomes `codex resume '<id>'`. |
| `tui::receiver_state` channel parity | Every dispatch decision (wait for an active turn, wait for a remote job, close an idle panel, start next) is asserted identical for SMS and email, and warm-panel reuse/replacement is asserted symmetric in both directions — the wait/reuse/start rules had previously been proven only for SMS. |
| `workspace::templates` tests | The seeded `AGENTS.md` / `README.md`: written when absent, never overwriting an edited copy, idempotent, cross-referencing each other, carrying nothing instance-specific (no `~/brain`, absolute paths, private skills directory, or personal names), and naming only skills Brain actually bundles — checked in passages that discuss skills, so an example project slug is not mistaken for one. |
| `tests/empty_workspace_initialization.rs` | First-run scaffolding through the compiled binary: an empty workspace gets PARA + CSVs + counters + a schema document declaring v2/`task_uuid`; a sync subcommand seeds the document it needs (sync dispatches before the workspace gate); and **a first sync that fails still leaves the whole task store in place**, since seeding it afterwards left a joining machine merging as `Legacy` against a `Current` remote with an empty `tasks/`. |
| `sync::csv_merge::remote_csvs` + `sync::csv_sync` part_06 + `sync::setup` schema_preflight | Remote task-schema generation decided by CSV *content*: absent/current/legacy classification (header-only current CSVs are current, whitespace proves nothing, one legacy CSV makes the remote legacy), a remote missing only its schema document is healed by publishing it, a failed publication is surfaced rather than assumed healed, and genuinely legacy remote rows still refuse while naming `brain workspace migrate`. Setup's guard refuses on legacy rows instead of on mere CSV existence. |
| `sync::identity::adopt` tests + `tests/sync_local/manifest_adoption.rs` | A machine joining an already-synced workspace: the remote manifest is adopted with its `receiver_ingress_id` preserved (minting would fork it, and bisync never reconciles the manifest), a remote owned by another workspace UUID is refused without writing, malformed remote bytes are refused, an existing local manifest is never replaced, and an empty remote leaves the registry UUID as the fallback. The unit half uses a fake remote with B2's blank-read semantics; the `sync_local` half drives real `rclone` against a local-path remote. |
| `tasks::schema::seed` tests | The embedded canonical `tasks/SCHEMA.json`: it declares schema version 2 / `task_uuid` merge key / mutable `task_id` display identity, its documented columns equal the headers Brain seeds and are all known current columns (the drift that left the pre-existing document declaring `assignee`), it carries no personal data, and seeding is write-only-when-absent, idempotent, a no-op without a `tasks/` directory, and leaves a freshly seeded workspace reading as schema-current. |
| `tests/workspace_requirements/task_schema.rs` | The `TaskSchema` health row: `incomplete` without `tasks/SCHEMA.json`, `ready` with it, so `sync status` cannot report a workspace ready when its next sync must fail. |
| `tests/workspace_suggestion_selector.rs` | Source guard: every backticked `brain <command>` a production message suggests for a workspace-scoped family (`sync`, `config`, `persona`, `tasks`, `habits`, `todo`, `reindex`, `user`) must carry `-w`/`--workspace`, so it goes through `workspace::suggest` rather than a hand-rolled literal. Machine-local and registry-level families (`env`, `skills`, `server`, `workspace`), `--help` examples under `src/cli/`, inline test modules, and the two rename-notice literals that *name* a command are excluded on purpose. |
| `tests/phase2_acceptance.rs` | Hermetic composed acceptance fixtures for one portable person selected from two independent machine registries and authenticated inbound identity flowing through `ActorContext` into a real task-script assignment. |
| `tests/todo_script_mutators.rs` | Brain-owned task scripts, including selected-root `BRAIN_ROOT` propagation and isolated actor/workspace environment for every subprocess. |
| `tests/task_schema_migration.rs` + `tasks::schema::transaction_tests` | Temp-only coordinator-owned migration primitives: workspace/kind-scoped deterministic UUIDv5, duplicate pre-existing UUID repair across task and habit CSVs, explicit last-legacy-sync and pre-existing durable-backup-base preconditions, exact durable portable backups, canonical/lexical backup-path separation, strict current-schema detection, row/display-ID preservation, byte-idempotent reruns, injected deep-directory and backup-file parent open/sync failures, descriptor-relative backup publication after parent replacement, immediate journal-temporary cleanup, and crash/failure recovery before and throughout replacement. Ordinary startup and sync never call this activation path. |
| `tests/multi_workspace_migration.rs` + `tests/multi_workspace_migration/` | Exact rollout ordering and state classification, activation locking before discovery and journal creation, UUID-scoped journal resumption, replayable local-only legacy-to-current joining with task/habit counter max/floors under stale, malformed, and missing inputs, duplicate-UUID repair during resumed verification with current CSV/baseline republish, schema-last CSV/baseline/metadata publication with injected transport failure, resume-only recovery across ambiguous publication, post-final-sync config/user/assignment reload, injected atomic journal/backup failures, privacy-limited backup inventory with nested-symlink refusal, canonical `assigned_to` sender/assignment gates with shipped headless remediation, and a compiled-binary local rollout proving preflight no-write refusal, complete cutover, retained backup, and byte-idempotent rerun. |
| `tests/task_id_collision_merge.rs` + `sync::csv_merge`/`csv_sync`/`counters` tests | Temp-only and pure fixtures for UUID merge identity, full canonical known-field headers, deterministic forward-compatible fields, remote schema absence/newer/malformed/mismatch refusal, deterministic display-ID collision winners/allocation, mirror-order and repeat convergence, pipe/comma `blocked_by`, production-format free-text `see_also` rewrites with URL and substring preservation, deleted-target fallback without marker leaks, project metadata reverse links, retry and local/remote error classification, strict/forward-compatible whole-operation schema policy, no-write refusal, and task/habit next-counter floors. |
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
| Legacy rows receive stable migration identity | `tests/task_schema_migration.rs` derives UUIDv5 from workspace UUID, CSV kind, and legacy display ID. `tests/multi_workspace_migration/` then drives the compiled explicit command through a complete local cutover and byte-idempotent rerun with one retained backup. |
| Concurrent empty-remote setup elects one owner | `sync::identity::tests::late_competing_claims_stage_then_only_one_retry_publishes_with_non_atomic_copy` uses barriers and a deliberately non-atomic canonical-copy fake, with no sleep, to prove that newly staged claims never publish and only the durable lowest UUID wins on retry. |
| Schema transition survives the legacy/current boundary | `tests/multi_workspace_migration/schema_transition.rs` proves remote compatibility preflight, CSVs, durable baselines, then schema ordering and retryable failure. `tests/sync_local/schema_transition.rs` uses real rclone for final legacy sync, migration, immediate current sync, a second legacy-machine join, and an independently current unconfigured machine establishing an empty remote before a second current machine converges. |
| Disable purges managed history without false-positive loss | `tests/triage_habits_config.rs` removes managed definitions, open rows, completed history, and derived references while preserving same-named unmarked rows and unrelated transcripts. `tasks::triage_habits::purge` limits JSON edits to top-level `tasks[]`, preserves unrelated JSON/text bytes and ambiguous display references, and aborts on malformed JSON, invalid UTF-8, or traversal failures. |
| Re-enable starts fresh | `disabling_purges_every_managed_row_and_derived_reference_then_reenables_fresh` proves exactly two new open managed rows, new UUIDs, and no restored history. |

The suite does not claim a filesystem sandbox, a general prompt-injection
detector, live cloud migration against a production remote, or live provider
compatibility beyond the probed OpenCode feature contract. Shared HTTP receiver routing and exact TUI
job-forwarding are covered by the active Phase 4 integration suites.

### Phase 5 composed acceptance matrix

The Phase 5 scenario uses one temporary machine registry and two roots. The
omitted selector resolves personal while `-b fam` resolves family; the roots
have different UUID caches, TUI locks, sync locks, and live leases in one
shared process. A signed family SMS resolves the portable wife identity and a
fake agent transport invokes the real Brain-owned task script with that actor,
producing a wife-assigned row. The same run proves deterministic display-ID
collision repair, no managed triage state when family triage is disabled,
equivalent registered-frontend advisory launches without personal capability
material, family unavailability after its fake TUI closes, personal receiver
continuity, and shared-process exit after the final close.

`tests/sync_local.rs` supplies the gated transport complement when `rclone` is
installed: both workspaces resync concurrently through their production
`WorkspacePaths`, maintain separate workdirs and CSV baselines, and reject a
remote UUID mismatch before any bisync workdir or remote content mutation.

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

# strict lint (pedantic + nursery are on)
cargo clippy --release --all-targets -- -D warnings

# release/privacy/skill gates
cargo test --release bundled_skills_carry_no_personal_data
python3 -m unittest discover -s skills/todo/scripts/tests

# Phase 5 composed and gated transport/lifecycle coverage
cargo test --release --test multi_workspace_acceptance -- --nocapture
cargo test --release --test multi_workspace_migration -- --nocapture
cargo test --release --test sync_local -- --nocapture
cargo test --release --test watch_local -- --nocapture

# source-format and patch hygiene
cargo fmt --check
git diff --check

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

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
- **Command-to-runtime ownership.** `TaskViewOptions::from(&Cli)` is tested
  after mutating and dropping the source clap DTO, proving task-view filters
  and display state are owned runtime data. `tests/tui_construction_boundary.rs`
  rejects an `App` lifetime, a retained task clap DTO, the obsolete receiver
  launch parameter, or any `run_tui` shape other than one owned `TuiLaunch`.
  It also requires startup acquisition and assembly to live in
  `runtime/builder.rs`, leaving `runtime/mod.rs` focused on execution and
  teardown, and rejects a TUI-root `PanelSide` re-export.
- **TUI dependency ownership.** `tests/tui_dependencies_architecture.rs`
  scans every production TUI source while identifying inline and external test
  modules. It rejects root `crate::tui::*` imports, production sibling
  `use super::*` imports, and
  wildcard child re-exports from `tui/mod.rs`. The token-aware fixtures cover
  direct and grouped use trees, arbitrary `pub(...)` visibility, whitespace,
  and nested groups. They distinguish `App<'a>` lifetimes from character
  literals. The suite also pins the lifetime-free App, sole overlay and
  receiver owners, and one-request `run_tui` boundary, and proves an external
  `#[cfg(test)] mod tests;` is classified as test code rather than production.
- **Terminal lifecycle ownership.** `tui::runtime::terminal` drives the real
  acquisition and restoration state machine through a headless recording
  operations seam. Failure injection covers rollback at every fallible setup
  boundary, including a possibly partial alternate-screen/mouse write. The
  tests pin normal cleanup order, optional keyboard-pop omission, cursor
  restoration, and idempotent repeated restoration. Scripted fail-once cleanup
  failures prove restoration continues through every armed step, returns the
  first required error, clears successes, and retries only failed capabilities.
  They also prove optional keyboard-pop failure stays best-effort and cannot
  replace an event-loop error, required cleanup retains its prior precedence
  when both paths fail, and `Drop` completes without panicking. None opens
  `/dev/tty`.
- **TUI process lifecycle and recurring order.** `tui::runtime::shutdown`
  tests a pure acquisition/shutdown state model. It pins singleton, receiver,
  server, terminal, App, and background-service acquisition; idempotent
  teardown; server-before-agent shutdown; periodic-puller and watcher drop;
  session-lock release; terminal restoration; and singleton ownership through
  final runtime drop. `tui::runtime::tick` pins the production-used order for
  exited-panel task refresh, heartbeat health, skill sessions, receiver, sync
  status with conditional task refresh, and the triage gate with conditional
  task refresh. Its refresh-stage test pins logical-day advancement, task
  reload, conditional rollover triage, and reporting. The event-update
  classifier verifies input filtering. Whole-shell agent tests also prove that
  every controller is attempted once and shutdown errors are returned to the
  runtime for logging. These tests use no sleeps or terminal.
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
  `BRAIN_ROOT` precedence, working-directory-independent commands for every
  frontend, actor-scoped Claude,
  Codex, and OpenCode rotation, atomic target-claim serialization, rollback and retry after
  an injected mutation failure, equal opaque IDs with conflicting immutable
  attribution, schema-v2 row preservation, and malformed/ambient no-op
  behavior. The hook-installer
  unit tests live in `src/command/server/receiver/hooks/tests.rs`; they pin the
  exact workspace-local Claude and Codex JSON command schema, execute the
  actual configured start and completion commands as one attributed lifecycle, and
  prove stale deployed scripts are refreshed. A regression test runs the real
  session-stop bridge on a payload with **no** `last_assistant_message` but a
  `transcript_path` present and proves it still publishes the response artifact
  by recovering the final assistant text from the transcript — delivery must
  not hinge on that one optional field. Further unit tests prove locked
  concurrent JSON mutations retain both workspace registrations and unrelated
  settings, always leave parseable JSON, and preserve original bytes when an
  atomic replacement fails. TUI setup tests prove a held workspace singleton
  prevents hook refresh.
  `tests/startup_migration.rs` runs the compiled binary to prove an ordinary
  command removes only Brain-owned global lifecycle entries across Claude,
  Codex, and OpenCode; installs all three workspace integrations into every
  existing configured root; proves every legacy workspace hook path forwards
  to the new start or stop hook for already-running frontends; self-heals a
  deleted managed hook; reconciles the receiver schema in every registered
  workspace that already has a state DB while leaving absent DBs for first
  `Db::open`; removes only schema-v7 launch retry origin on downgrade to v6;
  removes that receiver schema through the older down migration; leaves
  help and version byte-idle; and restores the previous lifecycle layout
  through the down migration.
  `tests/install_script.rs` drives the real installer around fake versioned
  binaries to pin upgrade-after-replace and downgrade-before-replace ordering.
  `tests/stop_hook_actor.rs` proves the stable response ID and actor/channel
  completion contract for a Codex-style `thread_id` payload. It also pauses a
  real session-stop bridge after payload parsing, rotates the same live Claude lineage
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
- **Palette state and menu navigation.** The generic `CommandPalette<A>` is a
  pure state machine tested for the search palette's case-insensitive
  word-atom/number filtering, the task palette's case-insensitive contiguous
  substring filtering, empty-query restoration and empty results, selection
  clamping, wrapping versus saturating movement, each surface's established
  Ctrl/Alt handling, Enter confirmation, and Esc/Ctrl-c cancellation. Catalog
  guards prove shared task/search application rows wrap the same `GlobalAction`
  and preserve their exact shared or contextual label/shortcut metadata. Task
  palette tests also pin globally scoped habits and agenda rows to
  `GlobalAction`, preventing them from bypassing the one global executor. A
  direct-shortcut architecture guard requires Close brain, Show tasks, Message
  brain, and Open agenda to enter `App::execute_global_action`, while preserving
  the active skill-session close route.
  Search structural guards keep the layout toggle last and ensure every
  `SearchAction` appears exactly once when applicable (including `CreatePdf`
  when a markdown target is present). The two contextual rows: "Create PDF"
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
- **The session store** (`state/`, in-memory SQLite). Scoped resume
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
  recording transport. The adapter contract table also drives identical
  normalized observation requests through Claude, Codex, and OpenCode, proving
  ordered missed-boundary recovery and explicit current/prior/placeholder
  session rotation behavior. Focused parser tests cover the exact ten fields,
  duplicate/missing/unknown fields, identifier and revision bounds,
  phase/timestamp consistency, equal and regressed cursors, identity/session
  mismatches, missing files, exact 4096-byte acceptance, one-byte-over-limit
  rejection, exact 256-byte identifiers, permissions, non-regular files,
  nonblocking FIFO rejection, symlinked cache ancestors, metadata-to-open
  replacement, stable-length short reads, trailing JSON, and redacted
  diagnostics. Cross-poll tests pin timestamp immutability, phase preservation,
  lifecycle order, and a nondecreasing emitted stream. A deterministic
  post-read seam rotates session ownership on another thread and proves the
  controller's fresh post-delegation check returns no facts. A structural
  receiver scan covers both coordination trees plus ephemeral receiver-tab
  ownership and rejects provider enum branches and literals, concrete adapter
  or parser ownership, transcript/rollout/event grammar, direct normalized-reader
  access, and observation-path access outside launch/controller ownership.
  Composed App tests then drive that neutral facade through Claude, Codex, and
  OpenCode. They prove an unobserved launch remains `launched`, current-session
  rotation persists exact acceptance, a newer full snapshot atomically catches
  up accepted plus progressing, and a cursor rebuilt from durable evidence does
  not emit an earlier phase. Deterministic cases cover missing, malformed,
  unrelated, equal-revision, and completion-only evidence; owner loss between
  read and transaction; artifact plus lifecycle completion in one tick; local
  cleanup after terminal evidence; and FIFO release to the next durable job.
  Existing exit and shutdown cases prove that no terminal evidence means local
  cleanup without replay or durable regression. No fixed sleep is used.
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
  response identity. Main-panel teardown refreshes the frontend-rotated native
  session binding before releasing the exact interactive session owner.
- **Focused TUI state owners** (`tui/state/`). `AppContext` tests pin immutable
  workspace, path, config, and frontend identity, including whole-snapshot
  config replacement. `BrainPanelState` tests pin main-controller and
  skill-session ownership, controller-derived completion actor identity,
  actor/turn transitions, controller shutdown, monotonic tab identity, and
  checked exhaustion with unchanged state. A focused App launch-boundary test
  forces the exhausted state through a test-only owner fixture, drives the real
  `open_skill_session` transaction, and proves the rejected controller and
  completion signal are cleaned up while the selected tab and tab collection
  stay unchanged and the error is reported.
  The same owner tests now interleave skill and receiver tabs to prove one
  never-reused ID stream, stable rendered/navigation order, failure-safe
  allocation cleanup, distinct receiver metadata and controller lookup, and
  whole-shell receiver shutdown. Composed App tests hold the main view, panel
  visibility, effective tab, and focus fixed across background receiver insert,
  exit observation, removal, and rejected allocation; they also prove receiver
  teardown is independent from main and skill controllers. A renderer test
  prevents receiver tabs from advertising the skill-only close shortcut.
  `AppServices` tests use focused runner, sync, and receiver-attachment
  coordinator seams to pin semantic effects, and `StatusState` tests separate
  transient and persistent messages while covering triage-gate, live-toggle,
  and sync-poll transitions. Standalone
  `TasksState` tests
  cover construction, view source changes, assignment and query filtering,
  logical-day rematerialization, selection, notes expansion, body rebuilding,
  wrapped-row layout, and scroll
  clamping. Standalone `ShellState` tests cover construction, main-view and
  focus transitions, panel side and hit-test layout, embedded search, logs, and
  active-tab selection against the set of open session identities. Call-site
  tests use semantic transitions, owner-side policy decisions, typed effect
  plans, or the iterator-backed task panel model, never aggregate field
  representation.
  `tests/tui_state_aggregates_architecture.rs` extracts every focused owner
  struct body, pins the exact private owner-field types, and requires App to be
  exactly the eight-field composition of context, tasks, brain, shell, overlay,
  services, status, and receiver. It requires one private declaration for every
  owned field and rejects duplicates or former flat declarations across
  visibility forms. Its positive API allowlist, exact tokenized projection
  shapes, and focused consumer signatures pin the permitted aggregate surface. A tokenized
  dataflow scan rejects direct or aliased representation access, propagates
  aggregate taint through intermediate lets, resolves local raw/reference
  return aliases, and rejects raw `App` forwarding through multiline typed or
  parenthesized bodies. A separate structural scan rejects immutable App
  methods whose entire body merely forwards a context or brain query to one
  owner. It derives taint only from the returned expression and its transitively
  referenced transparent local bindings, resolves shadowed names from the
  latest lexical binding, and ignores dead bindings. Explicit cross-owner
  mediation remains allowed. Synthetic renamed forwarders, typed and
  parenthesized alias chains, shadowing, dead bindings, and cross-owner fixtures
  pressure-test each guard branch.
- **Skill sessions** (`skill_session/`, `tui/app_skill_session/`,
  `tui/app_brain/tests/skill_session.rs`). The pure half is unit-tested directly:
  what the workspace offers (`available`: the builtin daily triage only while the
  check is enabled, then the parsed `skill_sessions` entries), what a running
  session withdraws (`runnable`), how a malformed definition degrades without
  renumbering its siblings' keys, ShellState-owned tab-strip / `Alt+<digit>`
  slot resolution, the interactive editor's
  routing and list arithmetic, and the signal's parse + close gate (including
  rejecting a token that isn't safe as a file name, since it arrives in a request
  body). Recording-transport App tests then prove the wiring: a configured session
  launches its own prompt (plus the appended completion protocol) under its own
  tab title, two sessions run as separate tabs and complete independently (only
  the tab whose token arrives closes, and its start row returns), a declared
  required output holds a tab open until it exists, and neither a stale signal from
  a dead shell nor a signal for another tab closes a freshly-opened one.
- **Durable receiver state.** Focused tests under `state/receiver/tests/`
  exercise stable SMS and verified/fresh email identity, frontend-specific
  native resume versus transcript fallback, durable acceptance and reopen,
  provider deduplication scope, FIFO selection, owner-checked transitions and
  renewals, every persisted lifecycle state, retry overflow, millisecond
  timestamps, foreign keys, transcript/binding replacement, and schema
  reconciliation. Recovery tests prove every nonterminal expired lease is
  reclaimable, terminal rows are not, expired launching atomically cleans exact
  stale lifecycle lineage and becomes a due bounded Spawn retry, exhaustion
  fails before a later tick selects the next FIFO row, Accepted and later
  progressed reclaim preserves state and retry evidence, and a due delivery
  retry stays retrying until its new live owner resumes delivery. The state test
  wrapper and store are split along identity, acceptance, claim, conversation,
  recovery, and schema seams rather than collected in one oversized file.
- **Receiver dispatch state.** Composed `tui::app_brain::tests` drive the one
  production durable tick with an injected clock and event barriers. They prove
  that a busy main panel is untouched; equal timestamps use job-ID FIFO order;
  later arrivals stay durable and unclaimed during an active run, then launch on
  the next tick; Claude, Codex, and OpenCode each receive a new isolated
  controller and PTY; and launch, close, and the next launch preserve view, tab,
  visibility, and focus. Exact-completion tests cover crossed artifacts, claim
  renewal, expiry and replacement after terminal validation, ownership loss,
  progressed stale-state refusal, child exit, spawn failure, orderly active and
  claimed shutdown, reply delivery, task reload, and sync push. Attachment tests
  prove shared Resend/download cancellation authority, cancellation during
  process publication, process-group kill and reap, `.part` rename, unread-result
  directory cleanup, and idempotent worker shutdown without fixed sleeps.
  Deterministic teardown observers advance the injected clock during worker
  shutdown and only after an owned directory Drop, proving each retry timestamp
  is sampled after all cleanup. A
  focused tab test proves
  that a second simultaneous receiver controller is rejected and shut down
  without changing the user's selected tab, view, visibility, or focus.
  `tests/tui_receiver_runtime_architecture.rs` rejects
  the former receiver field bag on `App`, direct representation access outside
  `tui/receiver/`, and cross-feature refresher/sync adapters or IO inside the
  runtime. It also pins the intent refresher and nonblocking attachment
  coordinator behind semantic `AppServices` operations; its runtime scan
  includes the receiver facade as well as `runtime.rs` and its children.
  `sync/freshness.rs` tests the strict two-hour message threshold;
  `sync/journal.rs` proves push-only/aborted rows do not refresh it.
  `server/delivery.rs` verifies that provider delivery is dispatched off the
  TUI thread. `server/reply/` proves that an SMS body loses every markdown
  marker before its length is measured, that near-markup (`2 * 3`,
  `snake_case_name`, an unpaired `**`, a non-address `<…>`) survives verbatim,
  and that code content is delivered as written; the `plain_text/block.rs` and
  `plain_text/inline.rs` suites cover their own edge cases (fence verbatimness,
  a `#` with no space, a dash rule versus a dash bullet, unpaired spans). The app receiver tests mutate `users.json` and the machine
  registry after queue acceptance, then prove Claude, Codex, and OpenCode launch
  through `AgentController` with the captured actor, channel, response email,
  and allowed participant recipients.
- **Receiver admission and workspace isolation.**
  `tests/receiver_workspace_isolation.rs` uses focused fixture and model
  support modules with deadline-bounded polling and no fixed sleeps. It drives
  both real-process fixture types through one shared permit so full-suite load
  cannot stampede optimized server startup. A test that replaces a fixture
  first drops its orderly-shutdown predecessor. Each directly spawned child is
  owned by an unwind-safe guard that kills and reaps it if startup polling or
  later fixture construction fails. The suite drives
  signed SMS through the real shared process into the exact UUID-scoped
  durable queue and proves HTTP success cannot precede the committed row. A
  second real-process fixture registers two live workspaces with distinct
  numbers, credentials, and actors for the same normalized sender, posts every
  request to the one machine-wide `/sms` URL, rejects a request signed with a
  peer workspace's credential (in both directions: a peer's number and a peer's
  token), then proves each request enters only its exact workspace DB.
  The suite also rejects an unknown sender and cross-workspace frames, verifies
  the 1 MiB body cap, and proves disabled or missing targets return one
  channel-specific unavailable response while another workspace keeps the
  process alive. Composed HTTP cases drop the client response after a complete
  write, observe the committed row, retry the same provider ID, and require one
  original job/conversation. A second case kills the shared child abruptly,
  observes production heartbeat/election recovery and re-registration, then
  proves the retry still resolves to that row. A 64-row durable queued-capacity
  case maps the rejected sixty-fifth SMS to the existing unavailable response
  without inserting a job or conversation. Absent-process coverage proves no
  responder is invented, while durable admission tests prove response loss,
  provider retry, process crash, and restart retain one exact database row. A
  signed Resend event submitted while its exact TUI is unavailable is replayed
  after re-registration; the replay must remain outside the durable store and
  must not reach the Receiving API.
  Pure tests under `server/receiver/dispatch/tests/` pin the synchronized late
  revocation boundary separately from provider-ID coordination: in-flight
  duplicates are unavailable, every later successful SMS/Email retry re-enters
  durable acceptance, and only verified-unavailable Email is retained in the
  bounded workspace/channel discard set. A barrier test verifies that an
  unavailable duplicate defers its discard, returns retry instead of success,
  preserves the in-flight reservation, and becomes remembered when the pending
  acceptance resolves. Store tests prove durable provider
  deduplication returns the original row before queued-capacity rejection.
  Barrier-driven dispatch tests disable
  exact live authority after actor resolution and prove revalidation prevents
  any durable admission. Typed provider tests preserve ignored-event 202 and
  upstream 502 outcomes. Injected Resend fetch tests cap both provider
  responses, and a counting reader proves only one proof byte beyond the limit
  is consumed. Inbound email identity is pinned at the boundary that produces
  `AuthenticatedInbound`: a realistic `Display Name <addr>` `From` header
  authenticates, thread participants reduce to bare addresses so the reply
  allowlist can match them, and an unparseable participant is dropped rather
  than carried forward. `users::normalize_mailbox` is tested directly for
  display names, quoted display names containing a comma, non-ASCII display
  names (which must not be split mid-character), and values with no usable
  address; a companion test pins that `normalize_email` still refuses to guess,
  so configuration validation is unchanged. `delivery::allowed_thread_recipients`
  proves a display-name `resend_from_email` is still excluded from its own
  reply. `server/receiver/http/email/body/tests.rs` covers prompt shaping as
  pure functions: HTML becomes readable text with `script`/`style` bodies
  dropped, `<br>` and entities preserved, an in-budget message passed through
  untouched, and an oversized one truncated on a character boundary with a
  notice the agent can see.
- **Workspace-specific receiver setup.**
  `tests/receiver_setup_workspace.rs` runs provider-free, local CLI fixtures
  against two selected workspaces. It proves distinct Twilio and Resend secrets
  remain in their exact machine records, numeric-looking secrets remain strings,
  output contains the machine-wide `/sms` and `/email` URLs (and no ingress)
  without secret values, and setup writes portable user mappings without rotating either
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
  Process-launching scenarios share a two-scenario RAII permit so concurrent
  optimized server startups remain bounded under full-suite load.
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
  transitions reject the pre-revocation ticket. Maximum-revision tests and
  staged durable-admission races cover disable, unregister, and disable-enable
  ABA after final revalidation but before commit; every losing admission
  returns unavailable and creates no durable row. Failed
  enablement update or receiver-changing registration replay leaves the
  whole lease table unchanged and cannot revive or extend authority.
  A synchronized test hook on the real `SharedReceiverPipeline` revokes
  authority after production final revalidation and authorization but before
  durable admission commits. Disable, unregister, and disable-enable ABA each
  cancel the admission and create no row. Mutation coverage
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
  expiry, and proves durable state remains empty. A third real-pipeline
  race holds the control mutex across that IO boundary, advances to exact
  expiry while commit waits for control, and proves the clock is sampled only
  after lock acquisition. Its commit probe requires both the COMMITTED state
  and the still-held mutex, making pre-lock clock and post-unlock CAS mutations
  fail.
  `tests/tui_receiver_dispatch_architecture.rs` builds production ownership
  from the Rust module graph rooted at `lib.rs`, `main.rs`, and binary roots.
  It retains declared logical module identities, including `#[path]` modules.
  Exact `cfg(test)` module, included-file, and item scopes are excluded; an
  undeclared Rust file defaults to production regardless of a misleading
  `tests`, `_tests`, or `test_support` name. Every such undeclared production
  file is an audited orphan root independent of its path; declared ordinary
  modules become receiver-reachable only through exact calls from a receiver
  root. The global AST symbol graph resolves exact cross-module aliases, UFCS
  owners, fields, and returned values without a same-basename fallback. Its
  declaration and export graph follows direct and nested public glob imports,
  terminates cyclic globs, honors local declarations, and fails closed on an
  unknown or colliding glob-owned type instead of guessing an identity.
  Function bodies add ordered lexical frames for block imports, type
  declarations, and variables: nested scopes shadow outer imports and unwind
  before siblings, while function, method, impl, alias, and struct generics
  resolve as local types before any named or glob import. Block-local type
  aliases retain their target, full ordered lifetime/type/const parameter
  sequence, type defaults, and generic substitutions at the definition scope.
  Arguments bind by the declared parameter kind, so a bare path parsed by
  `syn` as a type still occupies a const position and cannot shift a later type
  default. Explicit use-site type arguments override defaults; omitted defaults
  expand in parameter order and may refer to earlier parameters. An omitted
  parameter without a default remains opaque. Each active lexical alias frame
  combines the exact declaration with resolved supplied type facts. Finite
  same-alias arguments resolve before that frame guards default and target
  recursion, while a matching non-progress frame terminates true self or
  mutual cycles. Nearest declarations stop outer alias lookup, and unknown or
  ambiguous targets retain the fail-closed glob fact.
  Generic struct and tuple-struct field facts align lifetime, type, and const
  arguments against the declaration, resolve explicit types in the use-site
  module, and expand omitted or chained type defaults in the definition
  module. Field projections substitute those facts before deciding whether an
  exact controller, channel, or queue role is present.
  Qualified-self calls retain both type and trait identity in call targets,
  implementation nodes, and return facts, so two traits sharing one method
  name cannot overwrite one another. Non-test `cfg` alternatives that produce
  the same exact function or method node union every possible production call
  edge, violation, and return-type fact independent of source order. Return
  alternatives stay separate, so a forbidden canonical role in any possible
  branch is retained without combining unrelated facts into an invented type;
  exact `cfg(test)` items stay excluded. Ordinary method dispatch resolves a
  trait only when its exact canonical identity is visible through the local
  module, a named import, or the nearest finite module or block glob export
  graph.
  Local declarations shadow glob traits, and colliding glob traits do not
  create a guessed implementation edge. Typed function and closure patterns
  bind only the matching tuple, nested tuple, tuple-struct, struct, slice,
  reference, or or-pattern component fact. Tuple and tuple-struct suffixes
  after `..` project from the end; a slice `tail @ ..` retains the remaining
  sequence shape. Borrowing on an outer aggregate, a reference pattern, `ref`,
  `ref mut`, or match ergonomics reaches each projected field or component, so
  only owned queue iteration is consumption. Sequential `let` shadowing
  replaces the earlier fact after the initializer is analyzed, while
  alternatives merge only across `or` patterns and production `cfg` branches.
  Nested, `move`, and returned closures retain their own lexical variable
  facts, and an inner closure shadows outer bindings. The receiver-reachable
  graph rejects main-panel
  operations, activity sampling, and Unix socket accepts and reads, including
  reads before job decoding. A separate semantic pass rejects
  canonical typed `InboundJob` channel or queue consumption in every declared
  or orphan production module, independent of receiver-like module names or
  call reachability. Receiver `iter`, `try_iter`, `into_iter`, and `for`
  iteration consume messages. Owned `VecDeque` `into_iter` and `for` iteration
  consume the queue, while borrowed `VecDeque` iteration and inspection remain
  harmless; unrelated channel and socket operations remain outside that global
  rule. The guard also counts exactly one production receiver tick call.
  Mutation fixtures cover unconditional test-looking modules, true test scopes,
  non-test platform alternatives and return facts in both source orders,
  neutral-name orphans, indirect interactive helpers, typed destructuring,
  nested and returned closures, cross-module aliases, qualified-self UFCS,
  same-named trait implementations in both source orders, module and block glob
  trait visibility, controller fields and return chains, Unix sockets,
  channels, queues, and consuming iteration. Negative fixtures prove a
  qualified safe trait call cannot inherit its peer's forbidden edge, local or
  ambiguous glob traits cannot select a forbidden peer, unrelated channel and
  socket types stay harmless, and unrelated same-name methods and ordinary
  interactive controller APIs remain disconnected.
  Lifetime-only `JobSocket` binding, ownership, and drop remain allowed.
  Only the exact typed
  `ServerClient::refresh_enabled_generation(ServerGeneration, WorkspaceId)`
  target is treated as an outbound control capability; calls from every other
  `ServerClient` method remain in the receiver-reachable graph.
  The composed durable tests use
  the real state store and App coordinator together rather than relying only on
  store unit tests. They explicitly cover freshness-first renewal, exact
  registration and frontend-specific binding evidence, durable rollback, FIFO
  tie-breaks, active-run exclusion, disable-during-pending and
  disable-during-active management, terminal correlation, atomic native-binding
  plus terminal commit, retry after both a binding mismatch and a deterministic
  SQLite write error, durable `/new` and `/restart` atomic conversation
  rollover and backlog isolation, a
  barrier-driven restart scan/claim race, disabled pending-control rollover,
  next-message resume, and no-focus behavior without fixed sleeps.
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
  not enter durable state. A real SQLite lock test stages schema reconciliation,
  holds a competing writer, and proves receiver open installs the handoff busy
  budget before configuration or migration. Legacy Unix-stream step-clock
  characterization advances the same
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
- **Read-only status after startup reconciliation.** `tests/status_read_only.rs`
  first applies the current automatic migration, then runs the compiled
  `brain server status`, selected `brain receiver status`, bare
  `brain receiver`, `brain receiver email` / `brain receiver phone`,
  `brain sync status`,
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
  redundant migration rewrites, config initialization, users transaction locks, skill rendering,
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
  `AppServices`-owned injected receiver sync clocks/readers/runners
  deterministically prove the 250ms status poll, journal-advance gate, five-second retry grace,
  three-attempt fallback, and completion push. A runtime unit test proves the
  gate transitions only from supplied observations. The clock-driven watcher-loop test proves stopping one workspace's
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

- **The agenda sync** (`tasks/agenda/tests/`, `tests/agenda_sync_cli.rs`). The
  decision is pure — `sync_markdown` over agenda text, CSV rows, and a fixed
  date — so the section-preservation guarantee is asserted as whole-document
  equality, not substring probes. The filesystem shell is tested against a
  `Targets` struct built from a `tempfile::tempdir()`, including PDF regen
  through a stub `markdown-to-pdf` that copies its input to `--out` so the test
  can read exactly what the renderer was fed. `tests/agenda_sync_cli.rs` drives
  the real binary end-to-end.

  **Unit tests are protected structurally**: under `cfg(test)` the
  `agenda_markdown_dir` fallback is a path that cannot exist, so a unit test can
  never resolve the machine's real `/tmp` and rewrite the developer's own agenda
  for today. `agenda::tests::defaults` guards that.

  **Integration tests must isolate it themselves**, because they link the
  library without `cfg(test)` and may spawn the binary. `HOME` and
  `XDG_CONFIG_HOME` do not redirect `/tmp`. Two patterns:
  `tests/verbose_cli.rs::isolate_agenda_dir` runs `brain env set
  agenda_markdown_dir=<tempdir>` once the workspace is ready, and
  `tests/habits_workspace_routing/support.rs::agenda_env` writes the value
  straight into the fixture's registry records.
- **The bundled skills ship no executable code** (`skills::embed`). A skill
  carrying a script would be a second implementation nobody can test from here,
  so the bundle asserts no `.py`/`.sh`/`.js`/`.rb` file reaches it.

- **The bundled skills' command references** (`tests/bundled_skill_commands.rs`).
  Every `brain …` command named in any bundled skill is extracted — from code
  spans and fenced blocks only, so prose like "the brain is a directory" is not
  mistaken for one — and `--help` is run against each. A skill is an instruction
  an agent follows literally, so a renamed command has to fail here rather than
  in someone's session.
- **The ported task commands** (`tasks/mutate/tests/`, `tasks/backlog/tests.rs`,
  `tasks/scan/tests.rs`, `tasks/rules/tests.rs`, `tasks/habits/tests.rs`,
  `contacts/tests.rs`). Each is a pure decision over CSV rows and a fixed date,
  so the tests are tables in and values out. The boundaries are what they
  actually assert: which defers carry a penalty, which chunk cascades, what the
  lint fixes versus only flags, when a parked task counts as superseded.

## What we deliberately don't test

- **The interactive event loop.** `TuiRuntime` opens `/dev/tty`, toggles raw
  mode, pushes kitty flags, spawns the selected agent PTY, and runs the panel
  loop. We test its lifecycle and recurring order through pure stage models. An
  injected post-registration application-setup failure uses the production
  partial-start owner and proves server-lease teardown precedes job-socket
  removal without a terminal or sleep. We test the terminal lifecycle through
  injected headless operations, and the pure
  application logic it calls (`handle_key`, `App::*`, `focus_*`,
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
- **Personal data in repository changes.** Keeping identity, private paths,
  private URLs, and user-specific context out of committed skills, source,
  docs, scripts, tests, fixtures, and templates is a **review obligation**, not
  a test. A
  substring-matching guard would have to commit the very personal data it
  protects into this public repo — see "Why there is no automated personal-data
  guard test" in [decisions.md](decisions.md). For the exact reviewed branch
  base, inspect the complete name-status list and every added line in the full
  diff. Check names and identifiers, email and phone values, organizations and
  customers, domains and internal hosts, private URLs, absolute home paths,
  private product or project names, and user-specific prose. Confirm expected
  examples are neutral fixtures. If automation is desired, keep the private
  denylist outside the repo in a local hook or private CI secret.

## Test layout

| Location | Scope |
| --- | --- |
| `src/<module>.rs` → `#[cfg(test)] mod tests` | Pure-function unit tests for that module's branches (paths, settings, config, open_target, picker, menu, confirm, render, session, entry). |
| `tests/module_structure.rs` | Directory-wide architecture guard: every tracked Rust test location under `src/` and `tests/` must use behavior-owned section filenames, never `part_<digits>.rs`; failures enumerate every offending path. Large suites retain shared lexical fixture scope through a parent `include!` list and a sibling `*_sections/` directory. |
| `tests/tui_construction_boundary.rs` | Command-to-runtime seam: owned `TuiLaunch`, a lifetime-free `App`, no retained task clap command, no obsolete receiver launch argument, a focused startup builder module, and no TUI-root `PanelSide` re-export. |
| `tests/tui_dependencies_architecture.rs` | Directory-wide TUI dependency seam: production imports name their owner path explicitly, production modules cannot obtain sibling APIs through `use super::*`, and `tui/mod.rs` has no wildcard child re-exports. It also pins the lifetime-free App, sole overlay and receiver ownership, and one-request `run_tui`; token-aware self-fixtures cover direct and grouped use trees, arbitrary `pub(...)` visibility, lifetimes versus character literals, each forbidden spelling, and external test-module classification. |
| `tests/tui_state_aggregates_architecture.rs` | Focused-state seam: exact owner-body extraction pins private Context/Tasks/Brain/Shell/Services/Status representation, including the six semantic AppServices effects, and App's exact eight-field composition. It rejects duplicate or flat App declarations across visibility forms. Outside `tui/state/`, direct or aliased representation access and single-owner App forwarding through transitively referenced transparent local bindings are forbidden; focused handlers/renderers and semantic aggregate surfaces are required. Synthetic fixtures cover alternate visibility, typed/parenthesized alias chains, lexical shadowing, dead bindings, and forwarding evasions without rejecting cross-owner mediation. |
| `tests/tui_receiver_runtime_architecture.rs` | Receiver ownership seam: `App` owns one `ReceiverRuntime`, none of the former receiver-local fields, and no TUI module outside `tui/receiver/` accesses the representation directly. `ReceiverRuntime` retains only intent, freshness gating, the durable run, and a narrowly allowed legacy endpoint lifetime owner for BR-18; the guard rejects activity, input, queue, and socket-consumer behavior. `AppServices` retains the cross-feature sync effect adapter and nonblocking receiver-attachment coordinator behind semantic operations; the guard rejects those effects, workspace paths, journal/current-state reads, filesystem/process APIs, and detached sync launch from the receiver runtime. |
| `tests/tui_receiver_dispatch_architecture.rs` | Durable dispatch seam: an exact `cfg(test)`-aware Rust module graph defaults undeclared files to production and audits each as an orphan root while preserving declared module identities. A global symbol graph completes imports and aliases across every production source before collecting definitions, then follows exact receiver-owned call edges through neutral helpers, cross-module aliases, qualified-self UFCS, fields, and controller-return chains without same-basename guessing. Ordinary and qualified-self calls, trait implementation nodes, and method return facts canonicalize a generic aliased self type to its underlying identity while retaining the exact trait; direct and inherent methods remain distinct. Ordinary trait visibility follows exact named imports plus finite module and lexical glob exports, honors local shadowing, and refuses to guess across colliding traits. Type roles resolve modules, named imports, direct and nested public glob imports and re-exports, aliases, and generic arguments to exact production symbol identities. Function bodies add lexical frames for block imports, local type declarations and aliases, ordered variable bindings, and function/method/impl generics. Block aliases retain definition-scope targets, substitutions, defaults, and the full lifetime/type/const parameter sequence. Arguments bind by declared kind, including ambiguous bare const paths; explicit types override defaults; omitted defaults may refer to earlier parameters; omitted no-default parameters remain opaque. Generic struct fields align declaration parameters with explicit use-site types and definition-scope defaults before projection. Active alias frames combine the exact declaration with resolved supplied type facts. Finite same-alias arguments resolve before default and target recursion is guarded, while matching non-progress frames terminate self and mutual cycles. Nearest declarations shadow outer aliases, sibling blocks do not inherit one another, and local or generic types resolve before imports and globs. Sequential bindings replace same-scope predecessors only after initializer analysis; `or` patterns alone merge lexical alternatives. Tuple and tuple-struct rest suffixes map from the end, slice rest bindings retain sequence shape, and outer/ref-pattern borrowing propagates through projections. Alias, default, and export graphs terminate cycles, and unknown or ambiguous glob-owned type operations fail closed, so receiver-local same-named `AgentController`, `App`, `BrainPanelState`, and `InboundJob` types remain harmless while canonical aliased, qualified, module-globbed, and block-globbed Brain types stay guarded. Only the exact typed `ServerClient::refresh_enabled_generation` target is a control-capability boundary; every other `ServerClient` method stays receiver-reachable. The call graph rejects reachable main-panel input, activity sampling, Unix socket acceptance or reads before or after job decoding, and broad dead-code masking. Independently, a global semantic pass rejects canonical typed `InboundJob` channel or queue consumption in declared and orphan production modules while allowing unrelated channel/socket code, safe-trait peers, unrelated same-name methods, ordinary interactive APIs, and lifetime-only `JobSocket` ownership. Mutation fixtures cover both same-method implementation orders, aliased ordinary dispatch, aliased qualified-self return propagation, exact-symbol positive and negative controls, module and block glob imports for types and traits, local and ambiguous trait controls, block-local alias targets, generic chains and defaults, partial and explicit overrides, lifetime/const kind alignment, bare/braced/literal const arguments, generic struct fields, aggregate borrowing, exact rest mapping, sequential shadow replacement, finite same-alias nesting, true self/mutual cycles, ambiguous and unknown targets, nested shadowing, local-type and generic controls, declared-neutral direct and indirect typed consumers, dangerous `ServerClient` delegation, and their harmless peers; durable controls compile in the live coordinator, and production source contains exactly one receiver-consumer tick. |
| `state::receiver::tests` + `state::database::configuration_tests` | Durable job/conversation identity, ordered collision-safe v8/v9 token reconciliation, bounded generator exhaustion with full transaction rollback, lifecycle evidence, non-destructive FIFO claim bundles, queued-restart claim exclusion, one live workspace claim, exact-owner launch CAS, generic-transition rejection of progressed retry origins, bounded pre-acceptance retry, atomic expired-launch registration cleanup and due retry/exhaustion, conservative Accepted-and-later recovery, complete receiver-registration tuple attribution, crossed-placeholder and crossed-conversation rejection, Claude equal-ID proof versus rotated Codex/OpenCode binding, provider-first deduplication, atomic queued capacity, concurrent final-slot admission, transcript preservation, scope checks, numeric safety, foreign keys, reopen persistence, and receiver-specific pre-migration SQLite lock budgeting. |
| `tui::receiver::planning_tests` + `tui::app_brain::tests::receiver_durable_attachment_prompt` + `agent::adapter_tests::contract` | Table-driven Claude/Codex/OpenCode rendering from an already-authorized Fresh/Resume choice: empty transcript, UTF-8-safe newest-context truncation, complete localized path records with honest omission, composed post-staging prompt and shell-command bounds, and both fresh/resume command translations with a non-blank initial prompt. |
| `tui::receiver::{session_tests,failure_tests}` | Unique remote instances distinct from the main TUI, exact fresh/resume session ownership, explicit fallible registration cleanup with best-effort Drop fallback, main-lineage preservation, concrete shutdown diagnostics, and controller/session/durable retry rollback for every pre-acceptance failure class. |
| `tests/startup_migration.rs` | Compiled ordinary-startup reconciliation plus explicit downgrade for lifecycle integrations and receiver schemas v6/v7/v8/v9 across every registered workspace that already has a state DB, including damaged-state repair; absent DBs remain absent until first `Db::open`, and help/version remain side-effect free. |
| `tests/entry_collect.rs` | `entry::collect` against real temp directory trees. |
| `tests/root_resolution.rs` | `parse_config_root` + `expand_tilde_with_home` composed the way `brain_root` relies on. |
| `tests/receiver_url_cli.rs` + `command::server::receiver::url::tests` | Compiled-binary webhook-URL reporting with no server ever started: both channels by default, `--sms`/`--email` narrowing (`--sms --email` means all, not a conflict), **every `-w` printing the same machine-wide URL** with no ingress in it, a machine-global write under `-w` saying so and then being visible everywhere, a missing `brain_receiver_public_url` naming both ways to set it instead of printing a headless URL, and `receiver status` reporting the same rows. Pure tests cover channel selection, row rendering, the routing rule the block explains, and the trailing-slash normalization that would otherwise break provider signature verification. |
| `tests/config_portable_identity.rs` + `settings::portable::tests` + `settings::vars::tests` | The three config variables the portable roster answers. End-to-end: a real `brain receiver setup` (which writes only `users.json`) makes `config list`/`config get` report the phone and email it authorized rather than `(unset)`, the table names `users.json` and `brain user`, an unconfigured workspace still reads `(unset)`, and `config set` on one of the three is refused with the commands that work. Pure tests cover roster collection (inbound-allowed only, deduped, roster order), blank/absent response addresses, the roster outranking a stale legacy value, a legacy value still answering an empty roster, an unreadable roster never hiding the rest of the table, and the refusal landing before any write. |
| `tests/root_creation.rs` | First-use workspace setup: a workspace registered on another machine has its root created, its manifest written from the registry UUID **when no sync is configured** (a configured remote's manifest is adopted instead — see `sync::identity::adopt`), and PARA + task/habit CSVs + counters seeded; a root under a *missing parent* is reported rather than invented; re-running rewrites nothing and leaves user content alone. The pure halves (`root_setup`, `startup_sync_direction`, `performs_setup_sync`) are unit-tested in `workspace::initialize`. |
| `tests/persona_cli.rs` + `personalization::{personas,store,command,onboarding}::tests` | Compiled-binary per-user personas: writing one member never disturbs another, `show`/`get`/`list` address the right person and mark the local one, a member with no entry still appears as `(unset)`, an unknown user ID is rejected by name, a schema-1 file migrates onto the reading machine's local user and rewrites keyed, and a headless command reports a missing persona on stderr without failing or skipping its own work. The pure halves (keyed parse/migration, roster block, prompt gate, notice wording) are unit tests. |
| `tests/env_cli.rs` + `env::breakdown::tests` + `env::render::tests` | Recursive dotted get/set, secret-free confirmations, `default_agent_frontend` canonicalization (`Open-Code` → `opencode`) plus rejection of an unknown frontend without disturbing the stored value, and the `brain env` breakdown: machine-global rows are exactly the non-`workspaces` top-level keys, every registered workspace gets a block resolved against its own root (never a peer's value or default), an undeclared global still lists, a non-selected workspace's secret renders `(set)`, and the pure renderer's section order, default/selected labels, shared name column, `(unset)`/`(empty)` states, and legend footnote. |
| `tests/workspace_cli.rs` | Compiled-binary workspace registry behavior with isolated `HOME`, `XDG_CONFIG_HOME`, current directory, and roots: manifest-aware create/attach, persistence failures, record-preserving mutations, selector/validation errors, deterministic `NO_COLOR` list output, and non-destructive removal. `workspace list` reports feature health for every registered workspace, `-w` narrows it to one, and a workspace still needing setup renders a note instead of failing the listing. |
| `tests/workspace_readiness.rs` | Exhaustive bootstrap policy, strict manifest validation, interactive/headless readiness, repair, and first-create-to-next-command flow. |
| `tests/workspace_requirements.rs` + `tests/workspace_requirements/` | Central required/optional matrix, fail-closed malformed sync/receiver fields, portable receiver mappings, advisory capability health, redaction, exact selected-record isolation, and focused shared fixtures. |
| `tests/status_read_only.rs` | After priming the common automatic migration, filesystem snapshots prove workspace list, sync status, receiver status, tasks doctor, and server status do not create or mutate further machine/workspace state, including symlink and live-process cases. |
| `tests/workspace_registry_migration.rs` | Legacy flat-env conversion, exact backups, matching first manifest, idempotence, valid-registry portable-policy upgrade, and persistence-failure preservation. Plus the **machine-global hoist upgrades (v2 → v3 → v4)**: an ordinary command hoists `markdown_to_pdf_path` and then `brain_receiver_public_url` into the machine-global map and strips every record copy, several configured paths collapse onto the first canonical workspace, a machine that never set one gains no global map, the exact previous bytes are backed up once and a rerun is a no-op, every workspace then resolves the one value, and a read-only `workspace list` reads an old schema without rewriting it. The `receiver_origin_upgrade` section proves the **v3 → v4 receiver-origin hoist**: a machine that already configured a receiver has its per-workspace origin hoisted by the next ordinary command, credentials and each workspace's routing number stay in their records, the exact previous bytes are backed up once, and afterwards every `-w` prints the same machine-wide `/sms` URL with no doubled slash and no ingress. The pure rewrite is unit-tested in `workspace::registry::upgrade`. |
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
| `tui::app_brain::tests::{receiver_durable_launch,receiver_durable_lifecycle,receiver_durable_binding_completion,receiver_durable_resume_boundaries,receiver_durable_resume_completion,receiver_durable_shutdown,receiver_durable_attachment_worker}` | Composed production-tick coverage for disabled/busy gating, freshness-first exact renewal, timestamp/job-ID FIFO, an arrival waiting unclaimed behind an active run, isolated all-frontend controller launch, exact artifact correlation, atomic native-binding plus terminal commit, expiry and owner replacement after validation, resumed Codex and OpenCode second-run completion when the durable binding already equals the lifecycle-native session, post-validation owner renewal for missing all-frontend history and OpenCode probe errors, post-registration owner renewal for a rejected exact resume claim, live-owner Fresh controls for each fallback, retained retry after binding mismatch or SQLite error, barrier-driven lifecycle rotation after validation that preserves the old artifact and active run, lost ownership without job/retry mutation, progressed stale-state refusal, spawn and child-exit retry, receiver-first claimed/active shutdown, attachment process cancellation and owning cleanup, reply/task/sync terminal effects, and unchanged view, tab, visibility, and focus at launch, close, and next launch. Tests use injected clocks and event barriers rather than fixed sleeps. |
| `workspace::templates` tests | The seeded `AGENTS.md` / `README.md`: written when absent, never overwriting an edited copy, idempotent, cross-referencing each other, carrying nothing instance-specific (no `~/brain`, absolute paths, private skills directory, or personal names), and naming only skills Brain actually bundles — checked in passages that discuss skills, so an example project slug is not mistaken for one. |
| `tests/empty_workspace_initialization.rs` | First-run scaffolding through the compiled binary: an empty workspace still counts as empty after automatic lifecycle installation, then gets PARA + CSVs + counters + a schema document declaring v2/`task_uuid`; a sync subcommand seeds the document it needs (sync dispatches before the workspace gate); and **a first sync that fails still leaves the whole task store in place**, since seeding it afterwards left a joining machine merging as `Legacy` against a `Current` remote with an empty `tasks/`. |
| `sync::csv_merge::remote_csvs` + `sync::csv_sync::tests_sections::remote_schema_publication` + `sync::setup` schema_preflight | Remote task-schema generation decided by CSV *content*: absent/current/legacy classification (header-only current CSVs are current, whitespace proves nothing, one legacy CSV makes the remote legacy), a remote missing only its schema document is healed by publishing it, a failed publication is surfaced rather than assumed healed, and genuinely legacy remote rows still refuse while naming `brain workspace migrate`. Setup's guard refuses on legacy rows instead of on mere CSV existence. |
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
compatibility beyond the probed OpenCode feature contract. Shared HTTP receiver routing and exact durable
TUI consumption are covered by the active Phase 4 integration suites.

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
- **One focused seam for terminal lifecycle.** A recording operations
  implementation verifies Brain's acquisition and restoration decisions. It
  does not emulate crossterm or a terminal. Navigation and matching remain pure
  (`handle_key`, `App`), and only `TerminalSession::acquire` touches
  `/dev/tty`.
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

# skill-script gate
python3 -m unittest discover -s skills/todo/scripts/tests

# manual privacy gate (replace the value with the exact reviewed branch base)
privacy_review_base=REVIEWED_BRANCH_BASE_SHA
git diff --name-status "${privacy_review_base}..HEAD"
git diff --unified=0 --no-ext-diff "${privacy_review_base}..HEAD"

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

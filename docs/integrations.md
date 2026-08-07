# Integrations

`brain` is a single binary with a persistent TUI and short-lived command
families. It has no shell-mutating one-shot commands, so there is no plan
protocol and no zsh wrapper. The TUI owns interactive file opening, Finder
reveals, PDF conversion, trash, and agent launches by spawning processes
itself. This doc covers how the binary is run and each of those handoffs, plus
the frontend SessionStart/Stop hooks, UUID-scoped state DB, shared TUI-lifetime
server, and workspace-scoped sync boundary.

## How brain is run (`run.sh`)

`run.sh` is the entry point. It rebuilds `target/release/brain` when
`Cargo.toml` or any `src/**/*.rs` is newer than the binary (build chatter to
stderr), then `exec`s the binary, forwarding every argument. It does **not**
capture stdout, parse a plan, or apply any parent-shell effect — the binary
handles its own effects.

The intentional stdout families are `config/env/version`, `workspace list`,
explicit plain-task output, and help. `--verbose` mirrors logs to stdout for
non-TUI commands. Clap errors and diagnostics go to stderr. The TUI renders to
`/dev/tty`, so nothing an interactive session paints reaches stdout. Default
progress narration also goes to stderr. Long-running one-shot commands print
concise phase plans before they probe the filesystem, start daemons, spawn
external tools, touch the network, or write install trees. Every TUI run writes
a timestamped `/tmp` log file;
the command palette's receiver and brain log rows switch the main panel to a
scrollable view of the relevant log
directory and the log file via `open`. Verbose logs are intentionally more
detailed than the default progress trace: they include the selected command
action, non-secret argv/path details, task CSV load/write paths, rclone raw
stderr, CSV merge notes, server state decisions, doctor probe results, and skill
install counts.

## The tasks view (in-process, no handoff)

The selected workspace's task CSVs (`<brain-root>/tasks/{tasks,habits}.csv`) are read directly by
`brain`'s tasks main view (`crate::tasks`), and `brain tasks …` launches the
merged shell (or runs a tasks utility) in-process. The tasks-view command
helpers and shell-outs live in the tasks modules:

- **`brain tasks complete <id>`** — native task/habit completion in the
  binary. The CLI, TUI palette action, and `/habits/done` route all share this
  Rust path, so status, `completed_date`, `last_touched`, habit recurrence, and
  chunked-task MIT migration stay consistent without a Python completion script.
  Verbose runs log the resolved brain root, normalized id (when applicable),
  CSV files read/written, and completion result.
  Managed triage rows are rejected at each of those user-facing entry points;
  the sanctioned path that mutates them is
  `brain habits complete-managed-triage daily|weekly` (native) or the equivalent
  `apply_sync_rules.py --complete-managed-triage daily|weekly`, either of which
  becomes a no-op when the portable feature is disabled.
  All Rust task mutations and bundled Python CSV/counter writers acquire the
  same SQLite immediate transaction at
  `<workspace-cache>/tasks.transaction.lock`. Portable config read-modify-write
  operations, the habits web completion route, and bundled Python project
  metadata writers use that owner too. Python CSV and JSON writers reject a
  changed read snapshot and use a synced same-directory atomic replacement.
  The protected `remove_task.py` boundary rejects enabled managed-row deletion.
- **`brain tasks doctor`**: prints a progress plan before checking the selected
  UUID-scoped state DB schema, both Claude and Codex SessionStart hook settings,
  `rclone version`, and the centralized selected-workspace requirements. It
  opens SQLite through an immutable read-only URI so a WAL database cannot be
  checkpointed by observation, passes an explicit no-config path to the rclone
  probe, and does not create cache, config, lock, journal, or rendered-skill state.
  OpenCode remains outside the active controller health check.
- **`agenda` zsh function** — `Ctrl+A` runs it via the injected `ShellRunner`.
- **`brain habits` / palette "Open habits in browser"**: connect to the
  already-running shared brain process and open its
  `/local/<exact-live-lease>/w/<selected-ingress>/habits` page via the system
  `open`. These short-lived paths never participate in process election;
  if no TUI owns a live process, they ask the user to open a brain TUI first.
  The process itself carries no
  selected `--brain`; each habits request instead carries an opaque ingress ID.
  The process requires its live lease before reloading the exact registry record
  and matching portable manifest. Missing, malformed, unknown, no-live-TUI,
  unavailable, or identity-mismatched routes never fall back to the default.
- **`brain habits revive|fix <name>`** — repair a lapsed recurring habit (all
  occurrences `done`, none pending) by fuzzy name, without touching the server.
  Dispatched after workspace bootstrap by `command/server/habits.rs`; the
  logic lives in `tasks::revive`, which reuses `tasks::complete`'s
  `spawn_next_occurrence` so revived and completion-spawned occurrences share
  one anchor-to-due code path. See [features.md](features.md).
- **`brain habits skip <id|fuzzy> [--until YYYY-MM-DD]`** — cadence-aware
  "not today" for a habit (daily → mark done + respawn; non-daily → defer one
  day; `--until` → defer to a date). Dispatched by the same focused habits
  command module; the logic lives in `tasks::skip`,
  which reuses `tasks::complete`'s `locate` (id/fuzzy resolution, rejecting task
  ids) and `spawn_next_occurrence`. Native port of the retired `skip_habit.py`.
  See [features.md](features.md).
- **`brain habits complete-managed-triage <daily|weekly>`** — deterministically
  complete Brain's *protected* managed triage occurrence (which the ordinary
  complete/skip CLIs refuse while enabled): mark today's occurrence done + spawn
  the next, keyed on the stable `system_key`. The logic lives in
  `tasks::triage_habits::complete_managed` (reusing `tasks::complete`'s
  `read_csv`/`write_csv`/`spawn_next_occurrence`); the same
  `complete_managed_triage` entry point backs the daily-triage nudge's **Skip**
  button (`App::skip_triage`), so the button and the CLI share one Rust path. A
  no-op that mutates nothing when `enable_triage_habits` is off. The native
  equivalent of `apply_sync_rules.py --complete-managed-triage`.
- **Receiver server** - the machine-wide TUI-lifetime process accepts
  `POST /w/<selected-ingress>/sms` and
  `POST /w/<selected-ingress>/email` only for an enabled workspace with a live
  lease. Ingress resolution precedes all workspace provider and user reads.
  Twilio requests must pass the exact URL/form HMAC and SMS sender allowlist.
  Resend requests must pass the official `v1,<signature>` Svix verification, a
  five-minute timestamp window, and the email sender allowlist. Successful
  Resend deliveries receive HTTP 200, and the Receiving Email plus Receiving
  Attachments APIs supply the full body and signed download URLs. The process
  stops when its final live TUI lease is removed or expires.
- **`<agent_cmd> …` with cwd set to `<root>`**: the brain panel's PTY,
  shared by both main views (see below).

This is the "central dispatch" design: `brain` is the single terminal command,
and each capability is either an in-process main view (tasks, brain-directory
search) or a spawned process it drives (Claude or Codex for conversational work,
Finder/editor for files, `markdown-to-pdf` for conversions).

## The Brain Panel: Claude, Codex, Or OpenCode

The persistent shell's brain panel spawns the selected agent frontend itself,
inside a PTY (`pty_pane.rs`). Claude is the default; pass `brain --codex`,
`brain -cx`, `brain tasks --codex`, or `brain tasks -cx` to run Codex instead.
`brain --open-code` and `brain -oc` select the OpenCode adapter. The adapter
launches `opencode` with the named Brain agent, translates semantic input to
OpenCode control sequences, and supplies the trusted Brain policy through
`OPENCODE_CONFIG_CONTENT`. The installed `.opencode/plugins/brain.js` bridge
maps OpenCode session-created and session-idle events into Brain's existing
lifecycle hooks. Selecting both non-default frontends exits with
`🔴 Choose one agent frontend: --codex or --open-code.`

| Frontend | Command source | Resume/fresh command shape |
| --- | --- | --- |
| Claude | `claude_cmd` in brain env, default `claude --dangerously-skip-permissions` | `<claude_cmd> [--mcp-config <cache-json> --strict-mcp-config] --resume <id>` or `--session-id <id>` |
| Codex | `codex_cmd` in brain env, default `codex` | Current panels launch fresh as `<codex_cmd> [-c <capability-override>...]`; the adapter retains `resume <id>` for a future validated resume source |
| OpenCode adapter | `opencode_cmd` in brain env, default `opencode` | Launches with the Brain agent, separate initial prompt, and optional `--session`; lifecycle uses the workspace Brain plugin. |

`agent::ClaudeFrontend` and `agent::CodexFrontend` own these command shapes and
splice the configured base command in verbatim so it may carry its own flags.
`agent::OpenCodeFrontend` owns only stable identity and typed unsupported
results. All frontend operations are fallible so the controller can reject the
adapter availability and setup before a transport side effect.
Before the main panel claims a free resumable session, it resolves the selected
workspace's capability plan and asks the adapter for the candidate's stable
response identity. A validation or identity error therefore cannot strand a
free session as claimed. If the later transport launch fails, Brain releases
the instance claim and clears the response identity for the attempted
interactive or receiver launch.
The TUI owns an `AgentController` for each live main or triage panel and calls
semantic submit, queue, new-session, snapshot, and shutdown operations. The
crate-level `session::build_llm_command` remains a compatibility wrapper for
pure callers. `PtyPane` implements the frontend-neutral transport and
applies a complete launch spec; its working directory is set to the
already-selected `WorkspaceContext::root()` before the child starts, so the
agent begins in that workspace from the first instant without consulting the
default workspace. The transport evaluates the configured command with fixed
`/bin/sh -c`; it does not load a login or interactive profile and never depends
on a shell alias.

Trusted `HookMetadata` lives on `LaunchRequest` and is merged into the explicit
child environment by each functional adapter. The separate hook slot retained
on `LaunchSpec` is reserved and empty; the PTY transport does not interpret it.
When the shell exits, Brain explicitly shuts down both live controllers before
releasing the session lock, so teardown does not depend on `PtyPane::drop`.

Every `LaunchRequest` also carries an immutable access-policy snapshot built
from the selected workspace, resolved actor, and portable config before any
user or inbound prompt is considered. In `workspace_only` mode, Claude receives
that advisory through `--append-system-prompt`; Codex receives it through the
`developer_instructions` config override. The ordinary user prompt remains a
separate argument after the frontend's `--` option terminator, so prompt text
that begins with `-` cannot become a Claude flag or Codex config override.
Fresh, resumed, interactive, SMS, email, and daily-triage
requests use the same policy construction. Unrestricted mode adds no policy
instruction.

`workspace_only` is advisory prompt enforcement plus best-effort capability
filtering, easy to bypass, and not tenant isolation. The selected cwd,
environment filtering, and literal-path warning reduce accidental leakage;
they do not create an OS, VM, container, or filesystem security boundary.

Capability selection is separate from the boundary prompt. Portable config
requests logical MCP and skill names; only the selected workspace registry
record supplies commands, URLs, paths, and credentials. Claude writes selected
available MCPs atomically to
`~/.cache/brain/workspaces/<uuid>/capabilities/claude-mcp.json`, keeps the
directory owner-only and the file mode `0600`, and launches with
`--mcp-config` plus `--strict-mcp-config`. It does not use `--bare`, so the
user's shared Claude authentication remains in effect. Strict selection is
claimed only when the configured command can be parsed as a direct `claude`
invocation without shell operators, comments, option terminators, or
Brain-owned MCP/session/prompt flags. Other configured commands still receive
the generated flags but are honestly reported as advisory because Brain cannot
prove that the shell executes them. The installed Claude CLI has no verified
select-some skill flag, so skills remain advisory.

Codex receives only documented per-call `-c mcp_servers.*` overrides. Server
keys use the full workspace UUID plus a byte-hex logical name, avoiding both
global-name collisions and punctuation collisions. Credential values travel in
the child environment and the overrides name their environment variables, so
secrets do not appear in argv. For stdio servers, each selected server receives
collision-free generated environment names through an owner-only wrapper; the
wrapper remaps them to the server's requested child names immediately before
`exec`. Frontend/auth/lifecycle names such as `HOME`, `PATH`, `CODEX_HOME`,
provider API keys, and `BRAIN_*` are rejected as MCP credential targets.
Because dotted overrides merge with Codex's base
configuration and do not prove exclusion of unrelated global servers, Codex
MCP and skill enforcement remains advisory. Brain does not set `CODEX_HOME` or
select a separate profile.

The selected skill view is rendered per workspace UUID and actor below the
capability cache. It incorporates that root's extensions and contains only
available selected sources. Machine sources are loaded from exactly their
configured absolute directory. The root-owned first path component is the
explicit trust anchor (so a platform alias such as `/var` can be canonicalized);
the resolved directory must remain below that canonical anchor, and every
component beneath the anchor, the skill root, and every descendant entry must
be symlink-free. The canonical path is retained in the launch plan, so a
retargeted parent link cannot redirect a later render. Brain never searches a
sibling directory by logical name. It creates no links into the shared skill
registry,
so switching workspaces cannot prune or rewrite global skill state. Run
`brain skills status` for requested, available, and frontend enforcement rows;
the formatter never receives connection values or credentials.

Capability artifacts are frontend-lifecycle state. A Claude launch removes
stale Codex wrappers and abandoned Claude temporary files; a Codex launch
removes stale Claude JSON and temporary files. Unrestricted launches remove the
whole workspace capability cache. Cleanup treats symlinks as links rather than
following them. Before any recursive removal, Brain validates the trusted
workspace cache root and each existing component down through
`capabilities/actors/<actor>/skills`; a symlinked ancestor fails the launch
closed. Successful atomic publications sync their parent directory.
Debug formatting exposes names and enforcement metadata only; connection
material, credential values, prompts, hook values, commands, and launch
environment values are redacted.

The PTY clears inherited environment before launch. The explicit replacement
contains only a narrow set of frontend runtime necessities (`HOME`, `PATH`,
`SHELL`, user/locale/temp values, and `SSH_AUTH_SOCK` when present), the selected
workspace and actor's `BRAIN_*` identity, frontend kind, and trusted hook
metadata. It does not forward provider API keys, another workspace's secrets,
or registry JSON. Using a non-profile shell also prevents startup files from
rehydrating variables removed by the environment filter. This filtering and
the trusted prompt reduce accidents and naive leakage among trusted users.
They remain easy to bypass, are unsuitable for adversarial users or sensitive
isolation, and do not replace an external OS, VM, machine, or container
boundary.

When brain injects a prompt into an already-open panel, the controller sends
the text first and owns the final semantic queue action a couple of event-loop
ticks later so the frontend
doesn't treat the submit key as part of a paste. Claude receives `Enter`.
Codex receives `Tab`, because Codex uses `Tab` to queue a message behind active
work and treats `Enter` as immediate steering.

The TUI separately tracks whether a prompt has actually been submitted.
Opening the panel is therefore not itself considered active work. This lets an
inbound SMS or email replace an idle startup panel immediately, even if the
daily-triage modal is still covering it, while a real local Claude or Codex
turn still finishes before receiver work switches sessions. The Stop response
file clears that active-turn state even while a receiver lease is warm. A
failed receiver-session launch leaves the message in the queue for a backoff
retry.

### The daily-triage tab and its completion signal

Answering **Yes** to the startup daily-triage nudge spawns a *second*,
ephemeral agent session as a brain-panel tab (`App::triage_brain`,
`app_triage_tab.rs`) rather than typing `/triage` into the main session. It is
launched through an `AgentController` and a fresh `LaunchRequest` seeded with
`/triage`, but with two deliberate differences from the main panel:

- **It is never tracked.** Its `HookMetadata` contains only
  `BRAIN_TRIAGE_DONE_URL` and `BRAIN_TRIAGE_TOKEN`. The selected adapter adds
  the common workspace identity and `BRAIN_AGENT_KIND`, while
  `BRAIN_INSTANCE_ID`, `BRAIN_STATE_DB`, and `BRAIN_RESPONSE_ID` remain absent.
  The SessionStart hook requires those tracking variables in addition to
  workspace identity, so the triage session is never written to
  `brain_sessions` and is never a resume candidate.
- **Completion is signalled, not inferred.** A triage pass can involve
  back-and-forth with the user, so "the agent went idle" is not a reliable done
  signal. brain connects to the shared process already attached to the TUI and
  passes its capability-protected local triage-completion URL plus a one-time
  token into the session. When the `/triage` skill finishes (the habit marked
  and every output the run declared it must produce on disk) it POSTs
  `{"token": "<token>", "require": ["<path>", …]}` to that URL. The process and
  the TUI are separate processes, so the signal crosses on disk: the
  `routes::triage` handler records it to
  `<workspace-cache>/triage-done.json` via
  `crate::triage_signal::record_done`, and the TUI's per-tick
  `App::tick_triage_done` reads it (`triage_signal::read_signal`) and auto-closes
  the tab only when the token matches the tab it opened **and** every path in
  `require` exists (`triage_signal::ready_to_close`). The token guard means a
  stale signal from an earlier run can't close a freshly-opened tab; the
  `require` gate means a *premature* signal can't close the tab before the run's
  declared outputs are written (the signal is held, re-checked each tick, until
  they exist). **Core knows nothing about what those outputs are** — `require`
  is empty unless an extension rendered into the skill declared a path at the
  `triage:daily-required-outputs` hook, and an empty list closes immediately, so
  the generic core (and any fork) behaves exactly as before. If the triage child
  exits on its own, the same tick closes the tab regardless.

`brain server`'s route table therefore includes
`POST /local/<lease>/w/<ingress>/triage/done` (see `server/router.rs` plus
`server/routes/triage/`), an unauthenticated localhost-only endpoint consistent
with the ingress-scoped habits completion route.

## Shared-server process lifecycle

One machine-wide process stores its infrastructure below
`~/.cache/brain/server/`: `process.json`, `control.sock`, `election.lock`, and
`server.log`. `process.json` is generation-tagged and contains only PID, port,
generation UUID, and start time. It carries no selected workspace or portable
payload. A starter must atomically own the election lock, and the hidden
`brain server run --generation <uuid> --port <port>` loop validates that token
before binding. An advisory lock on the shared server directory serializes
exact observed-owner reaping and parent-to-child token adoption; the parent
releases the mutex only through an explicit handoff that leaves its generation
token for the child while retaining exact cleanup responsibility. A successful
child adoption changes the token owner, making parent cleanup a no-op; child
loss before adoption leaves the token unchanged, so the parent removes it when
the bounded publication wait ends. During that wait the parent retains the
elected `Child` and uses `try_wait`, rather than PID liveness, so a zombie is an
observed failed starter and election retries within the original deadline.
Immediately after publication, before the injected observation seam or any
fallible parent handoff cleanup, a lifetime waiter owns and reaps that child so
later SIGKILL cannot wedge heartbeat recovery behind a zombie token. Cleanup
failures still propagate to the caller. If
another startup contender briefly holds
the advisory mutex at that boundary, explicit cleanup retries at a fixed
interval for at most two seconds while the exact parent token remains instead
of abandoning it. Adoption or replacement changes the record and ends the
retry without touching the new owner; acquisition and timeout failures return
to the caller. Both the initial token inspection and the exact recheck under
the mutex distinguish a missing token from filesystem or malformed-JSON
failures. Those failures propagate, and cleanup borrows its handoff capability
so the same value can retry after repair. Losing TUI contenders use bounded
polling for the published winner.

The process is not an independently managed daemon. Public `brain server`
actions are read-only `status` and `logs`; there is no start, kill, or restart
surface. Only live TUI startup and heartbeat recovery may call the electing
client. Habits and triage callers attach without electing. The final orderly
lease removal returns `ShutdownNow` immediately; an injected-clock watchdog
expires crashed leases, preserves final-expiry shutdown across rejected late
control transitions, and stops an elected process that receives no first
registration within two seconds. Drop and SIGINT/SIGTERM cleanup remove the
process record and socket only when their generation still owns them. The
cleanup owner and safe `signal-hook` flags are installed before state
publication; the process loop observes flags outside the handler and performs
ordinary Rust cleanup.

The status probe is stricter than ordinary command bootstrap. `brain server
status` and `brain receiver status -b <workspace>` skip run-log creation and
all workspace mutation seams, including registry migration, access-mode or user
repair, users transaction recovery, installed-skill rendering, and render-stamp
writes. They inspect only existing process/control state and existing selected
workspace bytes. They never acquire election ownership or notify a process.
Receiver status reads the published process generation once and obtains live
process plus exact-workspace facts from one generation-bound control request.
A live-process transport, protocol, or replacement-generation failure is
reported. Process and workspace status use immutable lease views; watchdog
ticks provide periodic pruning and guarantee final crashed-lease shutdown when
no traffic arrives. Ordinary registration, heartbeat, enablement, unregister,
ingress lookup, routing, and availability transitions also opportunistically
discard expired leases. Status probes never prune or advance lifecycle state.

The externally observable lifetime is exact. Personal and family TUIs can hold
two leases in the same generation and receive one message through their own
job sockets. Closing family unregisters it; if personal remains, a family
request receives one unavailable response and is discarded while personal
stays routable. Closing personal then removes the final lease and the process
exits immediately. If the final TUI crashes, its heartbeat expires at TTL and
the watchdog makes the same shutdown decision. With no live TUI and no process,
an inbound text receives no Brain response.

The control socket exchanges one newline-delimited JSON request and response
per connection, with a 16 KiB frame cap and one two-second absolute deadline
covering connect, write, flush, and read. The codec checks that same deadline
before every attempt, including attempts that keep making progress. Each reader
consumes through EOF and rejects trailing frames, so a slow byte stream cannot
extend the budget one syscall at a time. Connect uses a safe nonblocking Unix
socket plus bounded readiness polling; it creates no connector worker that can
outlive the caller's deadline.
Register, heartbeat, receiver-enable refresh, and unregister requests carry the
target process generation; stale generations are rejected before lease state
can change. The read-only workspace-ingress lookup is also generation-bound and
returns a value only for the exact requested live workspace lease. Snapshot is
read-only and returns only generation plus live-lease count. Registration supplies the TUI-resolved root only for an ephemeral,
normalized comparison. The process reloads the machine registry, requires the
exact canonical name and workspace UUID, reopens that record's portable
manifest, and verifies its workspace and ingress UUIDs. It derives the expected
job socket from its own machine paths plus the validated UUID, then requires a
matching live TUI singleton PID and probes the job listener within the same
control-request deadline. Neither the root nor the client-supplied socket
selects stored state. Receiver intent comes from the registry record rather
than the TUI. If an accepted response is lost, retrying the exact same
generation, lease, workspace identity, PID, and derived endpoint is accepted
idempotently and renews the lease deadline. A competing lease or changed
identity is still rejected.

`brain receiver start`, `brain receiver stop`, `--with-receiver`, and the two
TUI command palettes persist intent through the same pure transition. The
transaction reloads the selected canonical record and verifies its immutable
UUID before changing only `receiver_enabled`. A running shared process receives
a generation-bound workspace UUID notification and reloads that record before
changing live routing authority. Missing processes and missing live leases are
valid: persistent intent governs the next registration, and the short-lived
caller never elects or hosts ingress. Startup applies `--with-receiver` before
the selected TUI binds its job socket and registers its lease.
Persistence is the commit point for these mutations. If the optional live
refresh cannot be delivered afterward, the CLI or palette reports a warning
while retaining and displaying the committed intent instead of claiming that
the mutation failed.
The route loader also requires that already-selected exact registry record to
remain enabled before credentials, users, prompts, or sockets are opened. This
closes the persistence-to-control-refresh race without changing ingress-first
routing.

For every HTTP request, the pure router first parses an exact typed provider
`/w/<ingress>/{sms,email}` route or local
`/local/<lease>/w/<ingress>/...` capability route. The shared process then captures a generation-bound
ticket for the exact accepting lease. Only that ticket permits registry, root,
manifest, or workspace-runtime selection. Those filesystem checks occur
without holding the control-state mutex, and the process revalidates the same
live authority incarnation after loading before returning a context. Ordinary
heartbeat renewal preserves the incarnation. Registration and receiver
enablement changes advance it; removal or expiry leaves no accepting
authority, and any later registration advances the remembered incarnation. A
disable/re-enable or same-fields unregister/re-register ABA transition always
invalidates the old ticket. The next revision is checked before expiry,
enablement, registration, or revision state changes, so overflow cannot partly
apply a transition.
Immediately before the job-socket handoff, dispatch also reloads the exact
canonical registry record, verifies the selected workspace UUID, and requires
its persisted receiver intent to remain enabled before revalidating the live
generation and authority revision. A persisted disable that races after route
loading therefore cannot enqueue even when its best-effort refresh notification
was lost. At admission commit, that filesystem reload remains outside the
control mutex. One combined operation then acquires control, samples the
monotonic instant inside the lock, revalidates exact route and admission
identity, and performs the admission CAS before unlocking.
Unknown ingress returns 404. Known ingress that is receiver-disabled or has no
live TUI returns 503 before local route behavior or receiver dispatch; it is
never acknowledged as accepted work.

The shared listener uses four fixed process-lifetime accept workers and no
application request queue. Each connection carries one request, request heads
and local action bodies are capped at 16 KiB, and parsing starts with a single
absolute two-second monotonic deadline established before the request head.
Successful bytes do not renew it. Local actions retain that deadline through
their response. Receiver bodies and local signature/event parsing also stay
inside it; only after they succeed does the request enter a separate fixed
30-second provider/handoff/response phase. An expired parse phase cannot be
revived at that transition. HTTP framing accepts at most one
`Content-Length` or the one supported `chunked` transfer coding, never both,
and rejects repeated or unsupported codings, invalid field-name syntax,
malformed chunk sizes, forbidden framing trailers, and over-limit chunks or
trailers. Field values remove only `SP` and `HTAB` optional whitespace and
reject forbidden controls and Unicode whitespace before framing decisions.
Chunk extensions are rejected by the intentional extension-free safe subset.
Workers cannot accept until all four spawns succeed, and a partial
start is aborted before any body read. Each worker finishes routing and
post-load live-lease revalidation before reading a POST body. This keeps
stalled or oversized body IO out of the lifecycle/control loop and keeps the
thread set fixed under incomplete headers. Final process exit signals workers
without joining a worker held in client IO. A TUI retains the ingress accepted
with its registration for habits and triage URLs; a short-lived habits command
asks the current generation for that selected workspace's live accepted
ingress. Neither path reselects an ingress from a later manifest read.

TUI startup orders ownership as workspace readiness, UUID singleton, UUID-local
`jobs.sock`, bounded connect/elect/register handshake, heartbeat worker, then
agent/event loop. If the selected generation exits between discovery and
registration, the handshake re-enters election and registers with the winner;
an authoritative workspace rejection returns immediately.
The worker sends one heartbeat per second. Missing transport, a stale
generation, or a lost lease triggers bounded election/reuse and re-registration;
concurrent TUIs use the same election path so only one replacement wins.
Orderly exit stops the worker and unregisters before removing `jobs.sock`.

The nudge's **Skip** button takes a different route entirely. Skipping is
deterministic — it only marks today's protected Morning Triage occurrence done
and spawns the next — so `App::skip_triage` calls
`tasks::triage_habits::complete_managed_triage(Daily)` **in-process** and then
`reload_tasks()`, with **no** brain-panel launch, prompt, or completion signal.
There is no agent in the loop and therefore nothing to signal: unlike the Yes
tab, the mutation is complete and observable the moment the button is pressed.
It respects `enable_triage_habits` (a disabled feature is a `Disabled` no-op
that still dismisses the nudge). This is why only the Yes path needs the
tab/token/`require` machinery above.

## Agent sessions: SessionStart/Stop hooks and state DB

Which session to run is decided by the **lock + recency** model in
`state.rs` (DB at `<workspace-cache>/state.db`, WAL):

1. At ordinary command bootstrap brain resolves the local actor once. TUI
   startup first acquires the workspace singleton, then refreshes the selected
   workspace's Claude and Codex hooks before opening or migrating the state DB,
   reaps locks held by dead
   PIDs, then walks `sessions_by_recency()` within the exact
   frontend/workspace/actor/channel scope
   and resumes the first whose **transcript actually exists** on disk —
   `~/.claude/projects/<mangled selected-root>/<id>.jsonl`
   (the Claude adapter's project-dir rule plus a fallback scan). A session opened but
   never chatted in leaves a DB row with **no** transcript, which `claude
   --resume` can't find (the "couldn't find session with ID …" error); brain
   skips those. If it claims a valid candidate it `--resume`s it; otherwise
   it starts a fresh `--session-id` (registered, locked to this PID) and, if
   it skipped a missing-transcript candidate, shows a status-line alert:
   *"couldn't find a session to resume; starting a new brain chat"*.
2. brain passes the selected workspace's `BRAIN_WORKSPACE_ID`,
   `BRAIN_WORKSPACE`, `BRAIN_ROOT`, `BRAIN_ACTOR_ID`, `BRAIN_CHANNEL`, and
   `BRAIN_AGENT_KIND` plus
   `BRAIN_INSTANCE_ID` / `BRAIN_PID` / `BRAIN_STATE_DB` /
   `BRAIN_RESPONSE_DIR` / `BRAIN_RESPONSE_ID` into the child environment.
   Live panels carry these as `LaunchRequest::HookMetadata`; the selected
   adapter combines them with trusted workspace identity. `session::env_for`
   remains a compatibility helper for pure callers and tests. Local work uses
   the resolved `local_user_id`.
   Receiver work first authenticates the provider request, then resolves an
   enabled portable sender; the queued workspace UUID and actor override the
   machine default for that complete request lineage. The accepting pipeline
   also captures the initiating user's normalized `response_email` and only
   allowlisted participants from that authenticated thread. Claude and Codex
   receive the same immutable actor/channel through `AgentController`, and
   later registry or `users.json` changes cannot substitute another response
   identity while the turn is running.
   Multiple machines may select the same portable person ID. That ID represents
   one person, not one device, owner, creator, or audit principal.
   Bundled task mutators resolve their selected root and actor only from this
   contract. A missing `BRAIN_ROOT` or `BRAIN_ACTOR_ID` fails directly; scripts
   never fall back to a home-directory brain. New rows use `BRAIN_ACTOR_ID` for
   `assigned_to`, while explicit assignment reads the selected root's portable
   `users.json` before writing.
3. A **SessionStart hook** —
   `scripts/claude_session_start_hook.py`, wired into
   `<brain-root>/.claude/settings.json` under `hooks.SessionStart` — fires on
   every session start / resume / `/clear` / compact. Reading those env
   vars, it accepts only an exact registered frontend/workspace/session/actor/
   channel tuple or a new frontend ID rotating an already registered active
   shell lineage. Unregistered events are ignored. An accepted event records
   the actual frontend session ID plus immutable attribution (locked to
   `BRAIN_PID`), resets completion status to `active`, and frees
   the instance's other sessions, so a `/new` mid-run becomes the session
   brain resumes next time and the prior conversation stays resumable. With
   any common workspace identity or required session attribution variable
   absent, the hook is a no-op. Authorization reads, target ownership checks,
   the accepted upsert, and prior-session release run inside one
   `BEGIN IMMEDIATE` transaction. Concurrent rotations therefore serialize
   before authorization; rejected or failed attempts roll back without
   changing either lineage, and SQLite's busy timeout lets a contender retry
   the decision after the current writer commits.
4. A **Stop hook** (`scripts/claude_stop_hook.py`) records the turn's final
   assistant message under
   `<workspace-cache>/responses/<response-id>.json` only while the exact
   frontend/workspace/session/actor/channel/instance tuple is still locked in
   the session store; an unregistered, released, or rotated completion is
   ignored. It resolves that message defensively: it prefers the payload's
   `last_assistant_message` convenience field and, when a Claude Code build
   omits it, falls back to parsing the last assistant text message out of the
   Stop payload's `transcript_path` JSONL. Delivery therefore never hinges on a
   single optional field — a turn with no recoverable final text is the only
   no-op. The hook stages a unique, synced file, starts `BEGIN IMMEDIATE`,
   rechecks that locked tuple, and updates the same predicate only when exactly
   one row matches. It publishes and syncs the artifact before committing the
   `completed` state. A publication or commit failure rolls back the database
   and removes or restores only the file owned by that attempt. A concurrent
   SessionStart rotation serializes at the transaction boundary, so a stale
   Stop event cannot complete the prior lineage. The stable response ID is
   independent of the frontend session ID, which gives fresh Codex turns the
   same completion path as Claude. The artifact includes frontend, workspace,
   session, response, actor, channel, and completion status. The
   TUI discards it unless both match the launched session context.
   For an interactive turn, the TUI consumes it as the completion signal that
   allows queued receiver work to switch sessions. For an active SMS/email
   job, it sends the channel-specific final response, marks remote work idle,
   and renews the three-minute channel lease. The PTY stays visible and can be
   reused by another message on the same channel. A different channel or local
   input switches only after active work has finished.
   If the agent process exits during remote work before the artifact is
   consumed, `App::close_brain` captures the transport snapshot plus the
   controller's immutable initiating actor/channel before shutdown and hands
   that captured value to the fallback delivery path. Mutable lease or local
   actor state cannot retarget the completion.
5. When the panel closes (the agent exits) or the shell quits, brain `release`s
   its lock, floating that session to the top of the resume queue — so
   "Message brain" (`Ctrl-M`) re-opens it, and a fresh startup resumes it.

Codex panels use the same hook scripts and state DB. Every TUI startup and
receiver setup writes equivalent `SessionStart` and `Stop` entries to
`~/.codex/hooks.json`. Shared Codex hook updates take an adjacent machine-wide
SQLite transaction lock, reload current bytes under that lock, and publish
synced JSON through a same-directory atomic rename. Concurrent workspace TUIs
therefore preserve one another's registrations and unrelated settings; a
failed replacement preserves the prior bytes. Current
Codex CLI versions may ask you to trust those hooks once in the Codex UI.
Claude and Codex remain separate scopes in one session store, and both report
session starts and completed receiver turns through the same brain response
protocol.
Brain verifies the exact installed `hooks.json` command shape and executes both
scripts against Codex-style `thread_id` payloads in tests. Whether Codex emits
those documented lifecycle events is frontend-owned behavior and is not
simulated as an external Codex process test.

**One hook namespace, one DB per workspace.** Before the merge, `brain` and `tasks`
each ran their own SessionStart hook keyed on separate env-var namespaces
(`BRAIN_*` vs `TASKS_*`) writing separate DBs, so the two shells never adopted
each other's sessions. The merged shell has a single app-level brain panel, so
there is now exactly **one** hook (`scripts/claude_session_start_hook.py`, keyed
on `BRAIN_*`), one DB per workspace UUID (`<workspace-cache>/state.db`, table
`brain_sessions`), and
one namespace. Both installers deploy the two scripts into
`<brain-root>/.claude/brain-hooks/` and register them in that workspace's
`.claude/settings.json`.

**Hook commands are project-relative, not root-specific.** The registered
command is `python3 .claude/brain-hooks/<script>.py`. Claude runs project hooks
from the selected workspace, so the command remains valid when a synced
workspace lives at a different home path or uses a different root name. The
Rust installer and `install_hook.sh` emit the same command.

Claude and Codex use different command scopes. Claude runs project hooks from
the selected workspace, so its project-relative command remains portable.
Codex reads a global `~/.codex/hooks.json` and may execute it from any working
directory, so Brain emits `python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/<script>.py"`
for Codex. `BRAIN_ROOT` selects a workspace explicitly; `$HOME/brain` is the
portable default. Brain does not depend on jpsyx-configs.

`scripts/install_hook.sh` deploys + installs both the SessionStart and the brain
`claude_stop_hook.py` Stop hook (stripping any stale entries — old absolute /
wrong-home / legacy `rc/` paths — matched by script basename). Every TUI
startup does the same automatically before state migration or agent launch;
`brain receiver setup` also refreshes both frontends. The standalone
`./scripts/install_hook.sh [brain-root]` remains a repair path for users who
change Claude settings manually. Its root precedence is the explicit argument,
then `BRAIN_ROOT`, with `$HOME/brain` retained only as a documented legacy
single-workspace fallback. The Stop hook is
required for receiver jobs: it records the completed assistant response so the
TUI can deliver it over SMS or email without exposing the full thinking trace.

Receiver setup stores provider credentials in the selected workspace's record
in the machine-local brain env store. Enter the public base URL only, for
example `https://brain.example.com`; the Twilio portal receives
`https://brain.example.com/w/<selected-ingress>/sms` and the Resend portal
receives `https://brain.example.com/w/<selected-ingress>/email`. Twilio signs the exact SMS URL, so the
receiver derives that path before verification. Ordinary provider resolution
uses only that selected record; Brain does not treat process-level `TWILIO_*`,
`RESEND_*`, or `BRAIN_RECEIVER_PUBLIC_URL` values as runtime overrides. Secret
values are redacted by `brain env list` and `brain env get`.
The same pre-write validator serves guided and headless setup. It accepts only
an HTTPS origin without a path, query, fragment, or embedded credentials,
normalizes the Twilio sender to E.164 and the Resend sender as an email, and
rejects blank selected-channel values with diagnostics that never include the
submitted provider or address value.

The guided setup maps each configured address to an existing portable person
or creates a named person in the selected workspace's `.config/users.json`.
SMS setup requires only a phone identity; email setup requires only an email
identity. Complete headless flags carry the user ID, optional new-user display
name, address, and explicit inbound-allowed state. Provider values and public
base URL are committed to only the exact canonical-name plus workspace-UUID
record. A successful setup or `receiver set` sends an existing-process-only
reload notification for that workspace UUID. It neither elects nor restarts a
shared process, and a failed notification leaves the saved configuration as
the commit point with a warning.

Before setup writes, it snapshots the selected provider values, exact
portable-user bytes, and selected Claude/Codex hook artifacts, excluding their
transaction lock pathnames. Provider, user, and hook writes are ordered under
one persistent workspace-local advisory lock that remains held through
rollback. Acquisition checks its fixed monotonic deadline before each lock
attempt and after a successful attempt before ownership can escape, then
reports the exact receiver setup lock that timed out. A failure conditionally restores only values and
files still equal to this attempt's after-image, preserving concurrent success,
peer records, and live lock inodes. Manifest identity and URL validation complete before the
first write, and live reload happens only after the whole transaction succeeds.

Run logging applies a central argv redactor before the timestamped file or
`--verbose` mirror sees a line. Receiver provider fields and portable
phone/email values are replaced for `--flag value`, `--flag=value`, and
`receiver set name=value` forms. Env assignments use `env::is_sensitive`, so
whole `agent_capabilities` documents and nested
`agent_capabilities.mcps.*.credentials.*` values use the same authoritative
policy after the same uppercase/dash canonicalization as `brain env set`. Each run log is created exclusively with mode
`0600` as defense in depth.

The receiver handoff endpoint is the selected workspace's mode-`0600`
`<workspace-cache>/jobs.sock`. One bounded serialized `InboundJob` carries the
workspace UUID, immutable actor and channel, normalized sender, prompt, stable
provider email/attachment IDs, provider ID, and acceptance-time response
metadata. The TUI stages the decoded job, returns `prepared`, and adds it to its
64-entry memory queue only after the server's exact-revision admission commits.
If the final `accepted` write fails, it removes that just-appended job before
the event-loop poll releases its exclusive queue borrow.
The shared HTTP listener uses four
blocking workers, a 1 MiB body limit, constant-time HMAC verification, and a
1024-entry recent-delivery cache keyed by workspace, channel, and provider ID.
Only acknowledged SMS jobs enter the cache. A failed or in-flight SMS handoff
returns unavailable and schedules no retry; a later provider retry may attempt
a fresh handoff. A signature-verified unavailable Resend ID is retained as a
permanent discard in the same bounded cache. Known unavailable ingress is
resolved before selecting that exact workspace's signing credential, and no
root, user, prompt, or job socket is opened for this verification. A later live
replay is rejected before Receiving API access. Persisted disable uses this
same exact-workspace path even when live refresh is blocked or fails. Dispatch retains the original
route ticket, reserves five seconds for
the response, derives one handoff deadline capped at two seconds and at that
response cutoff, then revalidates the exact generation, authority incarnation,
enabled state, and live lease immediately before socket IO. The safe
nonblocking connector, complete frame write, and acknowledgment read all use
that same absolute handoff deadline; continuous progress cannot renew it.
Resend's received-email and attachment-metadata calls each have a ten-second
maximum and a 1 MiB response cap; an oversized stream is stopped after one
proof byte beyond the cap, before JSON parsing. Verified unavailable, ignored,
and permanent discarded Resend events return provider success and remain
outside the queue; invalid signatures remain authentication failures, and
internal 500/502 outcomes remain failures rather than provider success. Delayed email
dispatch refreshes signed attachment access from stable provider IDs, and
processing plus final replies preserve the original subject and message
lineage without widening recipients. Receiving-API rejection or malformed
provider JSON returns 502.
Provider
credentials, message bodies, and signed media URLs are passed to `curl` through
standard input rather than process arguments. Provider output is captured so it
cannot corrupt the TUI. Outbound Twilio/Resend calls are serialized through a
bounded background delivery worker, preserving reply order without blocking
keyboard input or shell shutdown.

## System `open` and the editor

The search view opens files from inside the running TUI, via `open_target`'s
impure spawners; the brain panel never closes. A text file →
`open_in_editor_tab`, which runs `osascript` to open a **new iTerm2 tab**
(`iterm_new_tab_applescript` over `edit_shell_command` = `cd <dir> &&
${VISUAL:-${EDITOR:-nvim}} <file>`); a blob or directory → `open_with_system`
(`open <path>`); a Finder reveal (`Ctrl-Enter`) resolves to the parent dir
(`open_target::finder_target`) and calls `open` on it. Whether a file is text
or a blob is decided by `open_target::is_textlike`. On a non-iTerm2 terminal
the editor path falls back to `open <file>`. Nothing is emitted to stdout; the
shell stays up throughout.

Verbose TUI log viewing reuses the same system handoff: the brain log action calls
`open <parent-dir>` so Finder shows the log directory, then calls `open <log>`
for the timestamped file itself.

## Handoff: `markdown-to-pdf` (the "Create PDF" command)

The "Create PDF" command (palette row / `Ctrl-G` on a `.md` file) converts the
highlighted markdown to a colocated same-name PDF and opens it. It reuses the
user's existing converter rather than reimplementing PDF generation.

`markdown-to-pdf` is a hard prerequisite. Its path is the brain-env variable
`markdown_to_pdf_path` (machine-local, set via `brain env`), auto-discovered on
first run and validated at startup (see [config.md](config.md), `src/env/`, and
the gate in `settings/`); a missing/invalid path fails
fast with a red error. `open_target::create_pdf` spawns that command directly
(`<file.md> --out <file.pdf>`) — invoking the command, not any shell-function
wrapper, since a child process can't call a shell function. The output path is
`open_target::pdf_output_path` (same directory, same stem, `.pdf`).

- **Same-name guarantee.** The converter's non-interactive mode does *not*
  overwrite an existing PDF — it writes a `-vN` variant. To keep the output
  name identical to the source, `create_pdf` removes any pre-existing PDF at
  the target path first, so the converter always writes the exact name.
- **Opening the result.** The conversion runs in place and the PDF is handed
  to `open_target::open_with_system` (`open <pdf>`) — the brain shell stays up.
- **Best-effort.** A converter failure is swallowed (like a failed file-open)
  so a broken toolchain can't tear the shell down.

## Handoff: `osascript` → Finder trash (the "Delete" command)

The "Delete" command (palette row / `Ctrl-D` on any entry) moves the
highlighted file or directory to the **Trash** rather than unlinking it, so a
mistaken delete is recoverable (`Put Back`). It's a **user-style** delete: no
new mechanism, just the same Trash the user empties by hand.

`open_target::move_to_trash` shells out to `osascript` with the line
`open_target::trash_applescript` builds — `tell application "Finder" to delete
POSIX file "<path>"` (the path escaped for the AppleScript literal). Finder's
`delete` handles both files and directories and lands them in the Trash.

- **Confirmed first.** Both entry points route through the red `confirm.rs`
  modal (default **No**); the trash only runs on `Accept`.
- **Refresh after.** The search view re-walks its scope (`App::refresh`) and
  drops the trashed path (`picker::App::drop_path`), so the entry disappears
  from the list.
- **Best-effort.** A failed `osascript` is swallowed (like the PDF path) so a
  denied automation permission can't tear the shell down.

## Handoff: `rclone` + Backblaze B2 (`brain sync`)

`brain sync` (`src/sync/`) manually syncs the brain root to a private B2
bucket by shelling out to `rclone bisync`. It's a handoff like
`markdown-to-pdf`: brain doesn't reimplement transfer or conflict resolution,
it drives an existing tool and manages the surrounding safety and
bookkeeping.

The selected `WorkspacePaths` value is the only source for machine-local sync
locations. Production lock, journal, current-state, follow, rclone-workdir,
temporary-file, and CSV-baseline paths are either taken from it directly or
passed as an explicit derivation. No sync handoff consults HOME or a global
brain-root lookup.

- **Credentials never touch argv or rclone config.** `src/sync/remote.rs`
  (`build_remote`) turns the brain-env `sync` block (`b2_bucket`, `b2_path`,
  `b2_key_id`, `b2_app_key`) into `RCLONE_CONFIG_BRAIN_*` environment
  variables (`_TYPE`/`_ACCOUNT`/`_KEY`) passed to the rclone child process,
  plus a `BRAIN:<bucket>[/<path>]` remote argument that carries no secret.
  If `sync.crypt_password` is set, the same builder appends a second
  env-defined remote, `BRAINCRYPT`, with
  `RCLONE_CONFIG_BRAINCRYPT_TYPE=crypt`,
  `RCLONE_CONFIG_BRAINCRYPT_REMOTE=<BRAIN arg>`, and the optional crypt
  password/salt/filename settings from the `sync` block, then returns
  `BRAINCRYPT:` as the argv target. There is no persisted `rclone.conf`
  anywhere: remotes are reconstructed from brain env on every invocation, and
  because credentials ride in the child's environment rather than its argv,
  they never show up in `ps` output.
- **`rclone crypt` is optional and password escrow is external.** Crypt is off
  when `sync.crypt_password` is empty. To enable it, store rclone-obscured
  values in the machine-local `sync` block (`rclone obscure <passphrase>` for
  `crypt_password`, and optionally a different obscured salt for
  `crypt_password2`). `crypt_filename_encryption` can override rclone's default
  filename mode, and `crypt_directory_name_encryption=false` leaves directory
  names readable. brain does not generate, remember, recover, or sync the
  original passphrases; losing them means existing encrypted remote data cannot
  be decrypted.
- **Progress is narrated as work happens.** `command::format_sync_plan` prints
  only the mode, local root, and remote target. `sync_once` then prints a phase
  line describing the comparison and direction before marker repair and the
  rclone process, followed by a task/habit CSV merge phase. If a normal sync
  receives a check-access marker failure, it announces and runs the equivalent
  narrow `brain sync repair` flow automatically. These are default user-facing
  progress lines, separate from `--verbose` debug logging.
  A clean explicit repair also reapplies the selected workspace's managed
  triage invariant. Failed, aborted, coalesced, and ordinary sync runs do not.
- **rclone is an external prerequisite.** Brain checks that the executable can
  start before touching the remote. When it is missing, sync stops with an
  install guide with two explicit choices: Homebrew users can run `brew install
  rclone`, or everyone else can use rclone's official installer command. Brain
  does not bundle rclone, keeping its release, signing, and architecture
  updates independent from the transport's upstream releases.
- **The bisync argv is built once** by `src/sync/args.rs`
  (`bisync_args`): direction (`brain sync` / `--push` / `--pull` / `brain
  sync repair`) maps to rclone's `--conflict-resolve` (`newer` / `path1` /
  `path2`), plus `--conflict-loser pathname` + `--conflict-suffix
  __brainconflict__` (the keep-both mechanics — see [features.md](features.md)
  and [data-model.md](data-model.md)), `--max-delete <percent>`, `-v` (so
  rclone emits the `Transferred:`/`Deleted:`/`Errors:` summary block the parser
  reads — at default verbosity rclone prints no summary and every count parses
  as 0), `--stats 10s --stats-one-line` (periodic one-line progress instead of
  rclone's default one-shot summary), `--resilient --recover` (so an
  interrupted run can resume on the next invocation without forcing a full
  `--resync`), `--check-access --check-filename RCLONE_TEST`, and default
  excludes (`.git/**`, `.DS_Store`, `.cache/**`, the remote identity manifest
  `.config/workspace.json`, setup claims `.config/workspace-claims/**`, task
  schema metadata `tasks/SCHEMA.json`, friendly conflict copies `*(conflict *)*`, and raw
  markers `*.__brainconflict__*`) plus any
  user-configured `sync.exclude` patterns and an optional `sync.max_size` cap
  (`--max-size`, omitted when unset).
  `src/sync/run.rs` (`run_rclone`) spawns `rclone` with that argv and the
  env-var remote, and parses its captured stderr into transferred / deleted /
  error counts plus an abort reason.
- **Remote ownership is proved before remote data work.**
  `src/sync/identity/mod.rs` strictly loads the selected root's existing
  `.config/workspace.json`, requires its UUID to equal the selected
  `WorkspaceId`, and probes the same path under the `build_remote` target with
  `rclone cat`. Matching compatible bytes produce a private `VerifiedRemote`
  capability required by the check-access, bisync, semantic CSV, and counter
  lanes. Mismatch, malformed JSON, incompatible schema, and a missing manifest
  on a nonempty remote all refuse before those lanes can mutate anything.
  Setup alone may initialize a demonstrably empty remote. Before canonical
  publication, `src/sync/identity/claim.rs` writes the exact manifest as an
  append-only `.config/workspace-claims/<uuid>.json` object with immutable-copy
  defense, verifies it by `cat`, strictly enumerates and validates the claims,
  and elects the lowest UUID. Setup then re-probes `.config/workspace.json`;
  only the elected claimant may publish and read back the canonical bytes.
  Claims are excluded from ordinary transfer, and claim-only targets remain
  retryable. An unreachable probe never guesses that the remote is empty.
  Existing local manifest bytes are validation input, never rewritten by setup
  or transfer, and cross-workspace adoption is not implicit. Bisync workdir
  creation and stale-lock reaping happen only after this identity gate.
- **Progress streams live instead of blocking silently.** `run_rclone`
  inherits its own stdout for the child (`Stdio::inherit()`) and pipes only
  stderr (`Stdio::piped()`) — rclone writes its logs/stats to stderr. That
  pipe is read line-by-line on the main thread: each line is echoed to
  brain's stderr as it arrives *and* appended to a capture buffer, so the
  user watching the terminal sees rclone's live output while brain still gets
  a full transcript to parse into a `RunOutcome` once the child exits. No
  extra thread and no deadlock risk: there's exactly one pipe, and it's
  drained continuously rather than buffered up front. The periodic one-liner
  that makes this worth watching (`--stats 10s --stats-one-line`, e.g.
  `Transferred: 12.3G / 144G, 9%, 5.2 MByte/s, ETA 6h`) comes from
  `args::bisync_args`, alongside `--resilient --recover` (below).
- **`--max-delete` and `--check-access` are both active guards.**
  `max_delete_percent` (from `sync.max_delete_percent` in the brain-env `sync`
  block, default 50) aborts a run that would delete more than that share of
  files, without propagating the deletes. rclone's own safety abort ("too many
  deletes") is mapped by `src/sync/verify.rs` to an `Aborted` outcome pointing
  at `brain sync repair` if the deletes were intentional. `--check-access
  --check-filename RCLONE_TEST` is the path
  symmetry guard: rclone aborts unless both sync roots contain the marker.
  `src/sync/check_access.rs` owns that lifecycle. `brain sync setup` and
  `brain sync repair` write `<brain-root>/RCLONE_TEST`, copy it to the remote
  root with `rclone copyto`, and then run the resync. Normal `brain sync`,
  `--push`, and `--pull` do not silently repair missing markers; if the guard
  fails, `src/sync/run.rs` classifies it as `AbortKind::CheckAccess` and
  `verify.rs` tells the user to run `brain sync repair`.
- **rclone's own empty-directory guard.** Independently of brain's
  `--max-delete` guard, `rclone bisync` refuses to run at all when one side's
  prior listing has gone fully empty ("cannot find prior Path1 or Path2
  listings" / "must run --resync to recover") — its own protection against
  treating a wiped or never-initialized side as "delete everything on the
  other side." `src/sync/run.rs` recognizes this wording as
  `AbortKind::PriorListingMissing`. Historically that meant surfacing a
  pointer at **`brain sync repair`** for the human to re-run with `--resync`;
  as of the progress/resume work, `command::sync_once` handles the common
  case (an interrupted or killed `--resync`) itself: `should_auto_resync`
  (pure) says yes whenever the abort is `PriorListingMissing` **and** the run
  that just aborted wasn't already a resync (so it retries exactly once,
  never loops), `sync_once` re-runs bisync as `Direction::Resync`, and the
  journal note records "auto-resumed after interrupted baseline". `brain
  sync repair` still exists for restoring the guard marker and baseline on an
  already configured machine, but you no longer have to reach for it after a
  Ctrl-C mid-sync — the next plain `brain sync` resumes on its own.
- **Never journal `clean` for an interrupted or errored run.** This is what
  makes auto-resume safe rather than merely convenient: `verify::classify`
  only ever returns `Clean` on a full, zero-error rclone exit, so an
  interrupted run (even one that transferred most of its files before dying)
  always comes back `NeedsAttention`/`Aborted` and gets auto-resumed (or
  surfaced) on the next invocation — brain never tells you a sync finished
  when it didn't, so nothing in scope is silently left un-synced.
- **Deletions propagate bidirectionally, by design.** `rclone bisync`
  mirrors deletes exactly like edits: removing a file on one machine deletes
  it from the B2 bucket on that machine's next sync, and deletes it from
  every other machine on *that* machine's next sync — there is no
  local-only delete. The only brake on this is the `--max-delete` guard
  above; short of tripping it, a delete is real and bidirectional. B2 itself
  keeps prior file versions after a delete (its own object versioning)
  unless a bucket lifecycle rule is configured to prune them, so a delete
  synced by brain is not necessarily unrecoverable at the B2 layer — but
  brain does not manage or rely on that; treat `--max-delete` as the only
  safety net brain provides.
- **The setup flow.** `brain sync setup` (`src/sync/setup.rs`) checks
  `rclone` is on `PATH`, then acts as a guided walkthrough: it asks whether you
  already have a bucket (`ask_has_bucket` / pure `parse_yes_no`), and if not
  prints `bucket_walkthrough()` — the step-by-step Backblaze bucket + app-key
  guide (private, Default Encryption on, Object Lock off) whose coverage of the
  critical settings is unit-tested — and pauses. It clearly says this enables
  cloud sync on this machine, then prompts on `/dev/tty`
  for the bucket + B2 key id + application key (pre-filled with any existing
  values), validates them, validates the local manifest, and probes the remote
  identity before persistence. It prints the local canonical name and UUID,
  configured target, observed status, and a compatible remote manifest's UUID.
  A matching identity proceeds. Setup publishes and reads back the exact local
  manifest when the reachable remote has no files. For a nonempty manifestless
  target, setup requires either an explicit interactive confirmation or the
  exact selected UUID through `--adopt-workspace-id`; a generic `--yes` is not
  accepted. Mismatched, malformed, incompatible, or present-but-unreadable
  manifests remain hard refusals. A newly published ownership claim stages the
  attempt and returns before canonical publication; a retry elects from the
  durable claim set. The UUID-scoped sync lock covers remote claim election,
  manifest publication/read-back, any safe empty-remote task-schema transition,
  marker bootstrap, and the complete initial baseline. Setup runs one
  `Direction::Resync` sync, requires its result to be `Clean`, and only then
  writes the `sync` block into brain env (`crate::env::set_raw`, **not** brain
  config, see [config.md](config.md)). `NeedsAttention`, `Aborted`, and
  transport errors leave the candidate credentials unsaved. An active workspace
  migration journal refuses setup before remote identity work. It never creates a bucket
  or treats an unreachable probe as a new bucket. If the existing `sync`
  block contains crypt fields, setup preserves them when refreshing bucket
  credentials. `brain sync repair` reruns just that last step (check-access marker
  bootstrap + resync), so it is the recovery path for the empty-directory guard
  above. It requires the `sync`
  block to already exist; if the user runs it first, brain explains that repair
  only repairs an existing setup and ends with `brain sync setup`.
- **The journal.** Every run (whichever direction, including `setup`'s
  initial baseline) is classified — `Clean` / `NeedsAttention` / `Aborted` —
  by `src/sync/verify.rs` and recorded by `src/sync/journal.rs` into a SQLite
  journal at **`<workspace-cache>/sync/journal.db`** (table `sync_runs`, WAL like
  the state DB). It's machine-local and **never synced** (it lives outside
  the brain root, like the rest of brain env), so each machine's sync history
  stays its own. `brain sync status` reads the most recent row plus the
  configured trigger flags and the open-conflict count; see
  [data-model.md](data-model.md) for the row schema.
  Status opens the journal through an immutable read-only URI, preventing an
  observational read from checkpointing WAL state. It first renders cloud-sync
  and watcher requirement health from the raw
  selected record, so a partial or malformed block is `incomplete`, not
  silently `off`. That local status does not cache or claim a remote identity;
  setup and every data-moving sync/repair/check operation still probe the
  remote manifest and require the selected workspace UUID immediately before
  transport.
- **Conflicts, on disk.** rclone leaves the losing side of a same-file
  conflict named `<original>.__brainconflict__<N>` (literal dot + suffix +
  trailing integer, e.g. `one.md.__brainconflict__1`), on both sides; a
  post-pass (`src/sync/conflicts.rs`, `rename_markers`, matching via
  `is_marker`) renames it to the friendly `name (conflict <host> <date>).ext`
  right after the rclone run. Both the friendly (`*(conflict *)*`) and raw
  (`*.__brainconflict__*`) patterns are default excludes above, so neither gets
  synced around on a later run. Because the rename leaves zero leftover markers,
  `sync_once` also feeds the *count of copies renamed* into `verify::classify`
  so the run is still reported `NeedsAttention` (journalled `conflicts=N`) — a
  real conflict is never masked as clean. `brain sync conflicts` lists what's
  still open; resolving a group is the agent-driven flow described next.
- **The conflict-resolution contract for agents (C5).** `brain sync conflicts`
  re-derives `ConflictGroup`/`ParsedCopy` from the on-disk friendly names via
  `conflicts::parse_conflict_name` + `conflicts::group_conflicts`, so the
  themed human list and `--json` agree on which files are real conflict copies.
  `--json` is the structured enumerator the `/second-brain resolve-conflicts`
  skill (and any other agent) consumes: `command::conflicts_json` renders each
  group as `{ "original", "original_exists", "copies": [{ "path", "host",
  "date", "modified", "bytes" }] }` (paths relative to the brain root;
  `modified`/`bytes` are `null` when the file's metadata can't be read).
  `brain sync resolve <original> [...]` is the matching brain-side deleter: it
  looks up that original's copies via `conflicts::copies_for_original` and
  deletes them (never the canonical file itself), refusing outright
  (`ResolveDecision::CanonicalMissing` in `src/sync/command/resolve.rs`) if the
  canonical original doesn't exist on disk — the skill must merge into it
  first. `resolve` never invokes `rclone` or the journal; it's a pure local
  filesystem delete, so the skill runs one ordinary `brain sync` afterward to
  push the resolution out.
- **The two task CSVs and their schema marker skip bisync entirely.**
  `tasks/tasks.csv` and `tasks/habits.csv` are added to `args::bisync_args`'s
  default excludes alongside `tasks/SCHEMA.json` (`src/sync/args.rs`), so the
  generic lane cannot publish merge semantics ahead of data and baselines. Lane-A bisync never touches the CSVs;
  line-based bisync would happily let one machine's edit clobber another's on
  structured, id-keyed data. Instead `command::sync_once` runs a dedicated
  step (`crate::sync::csv_sync::sync_csvs`) once bisync itself hasn't aborted.
  It holds the workspace task-store owner across CSV publication and dependent
  counter reconciliation. For each CSV it reads the cached baseline
  (`csv_sync::baseline_path`,
  `<workspace-cache>/sync/baselines/{tasks.csv,habits.csv}`, machine-local and
  never synced), the local file, and the remote copy (fetched with `rclone
  copyto <remote> <tmp>`, over the same env-var `BRAIN:` remote bisync uses);
  fetches and preflights both local and remote `tasks/SCHEMA.json` plus the base, local, and remote generations
  of both CSVs, then merges with the pure 3-way merge in
  `crate::sync::csv_merge`. Any preflight failure aborts this whole lane before
  CSVs, baselines, project metadata, remote objects, or counters change.
  Nonempty legacy input must contain and remains keyed by `task_id`, even when
  compatibility writers have added `task_uuid` and populated it for new rows;
  only matching active `tasks/SCHEMA.json` schema v2 metadata makes input name-aligned and
  keyed by immutable `task_uuid`. Only an absent remote marker is legacy.
  Every present marker must parse as a complete supported protocol declaration
  with an integer version and `task_uuid` merge key; malformed, incomplete,
  wrong-typed, incompatible, newer, and legacy/current mismatch states fail
  before either remote CSV is read or any publication occurs. Current output
  orders all known fields canonically and declared forward-compatible fields
  lexically. The
  inactive task-schema helper is never called by sync; see
  [data-model.md](data-model.md) for the rules. Distinct UUIDs that claim one
  display ID are renumbered deterministically, side-specific `blocked_by` and
  bounded task references in free-text `see_also` are resolved through UUIDs;
  URL spans and non-reference text remain byte-preserved. Final project reverse links are
  staged from CSV `project` fields before repo-relative `.METADATA.json` paths
  are copied to the configured remote. Every authoritative metadata file is
  republished, even when its local bytes were already current, so retry heals
  a previous partial remote publication. Local metadata write failures surface
  as local-write errors, while callback failures identify remote publication.
  The operation writes the merged CSV
  back to the local file, pushes it to the remote with another `rclone
  copyto`, then overwrites the baseline with the same merged text. A missing
  baseline (first run on a machine) means every row reads as newly added, so
  the first CSV sync is a safe union of both sides rather than a guess. The
  bundled task/habit writers stamp `last_touched` on every row mutation, so
  same-field CSV conflicts normally resolve by row recency on both tables. The
  merge outcome (added/merged/deleted/soft-conflict counts) is folded into the
  sync journal's `note` column as a `csv: +A ~M -D` segment (see
  [data-model.md](data-model.md)); a typed CSV-lane failure stops sync and
  prevents counter reconciliation. The step is skipped entirely when the
  bisync run aborted. See [decisions.md](decisions.md) for why this file pair
  gets a semantic merge instead of keep-both.
- **The two id counters are max-merged and floored out-of-band, right after the CSVs.**
  `tasks/.tasks_next_id` and `tasks/.habits_next_id` hold the next integer id to
  hand out. They're excluded from bisync too, because bisync's newer-mtime rule
  would let a machine with a *lower* counter that wrote more recently win, and it
  would then re-hand-out ids the other machine already assigned. Instead
  `command::sync_once` calls `crate::sync::counters::sync_counters`: for each
  counter it fetches the remote value (same `rclone copyto` transport), reads
  the local value, and applies the corresponding floor returned by the CSV
  operation. It does not fetch the remote CSVs a second time. The resulting
  value is `max(local, remote, reconciled_max + 1)`. Push-only sync also writes
  the reconciled floor locally. The floor prevents a
  normal writer from reissuing a display label created by collision
  reconciliation. Missing or garbage counter values are treated as absent.
- **`brain check` has a read-only CSV lane too.** Since those two CSVs are
  excluded from dry-run bisync, `src/sync/check.rs` reads the same cached
  baselines, reads the local CSVs, fetches each remote CSV with `rclone copyto`
  into a temp file, and reports row-level `+A ~C -D` push/pull deltas. It never
  writes local files, remotes, or baselines; if a remote CSV cannot be fetched,
  the local row diff is still shown and the remote side is reported as
  unchecked. When the cached baseline is missing, the preview avoids
  double-counting: identical local/remote CSVs are clean, and when both sides
  are non-empty and differ, the remote CSV is used as a provisional snapshot
  for local row deltas. The report explicitly says CSV rows are baseline diffs,
  not provenance, and that `brain sync` will merge by immutable identity after
  schema migration. The lane resolves `tasks/SCHEMA.json` once, uses `task_id`
  while migration is inactive and `task_uuid` only for active schema v2, then
  parses baseline, local, and remote generations through one fallible boundary.
  Invalid metadata, malformed records, and duplicate active identities render a
  warning naming the generation and relative CSV; they never panic or emit a
  false clean result.
- **Task-schema activation is an explicit coordinated operation.**
  `brain workspace migrate` owns the last legacy semantic sync, remote identity
  and all-machines gates, portable backups, resumable journal, task UUID
  activation, derived rebuild, and final verification. Ordinary startup,
  readiness, and sync never activate it. The UUID-scoped journal is
  `<workspace-cache>/migrations/multi-workspace-v1.json`; its original retained
  backup stays below `<workspace-cache>/migration-backups/`. When sync is
  configured, the first journaled step uses the existing legacy semantic sync
  before task UUID identity becomes authoritative. When another machine has
  already published schema v2, that step runs generic rclone without ordinary
  task reconciliation, then `migration::legacy_join` fetches both remote CSVs
  with `rclone copyto` and performs a local-only, `task_id`-keyed bridge. It
  preserves remote UUIDs for matching rows, then fetches both remote counters
  and atomically floors the local task and habit counters to
  `max(local, remote, joined_max + 1)`. Missing or malformed counters are
  absent inputs, so the joined tables still establish a safe floor. The bridge
  is safe to replay before local activation and has no remote publication
  path. The coordinator immediately
  reloads portable config, users, and both assignment CSVs after that sync;
  newly pulled sender mappings and managed-triage policy are therefore
  preflighted before backup or mutation. The coordinator takes the UUID sync
  lock before rollout discovery, planning, or journal creation and retains it
  through verification. The transition publishes both
  current CSVs, durably writes the exact local baselines, then publishes
  `tasks/SCHEMA.json` last. Every step is atomically recorded, so rerun validates
  the workspace/plan and resumes the same backup. Ordinary sync and setup
  refuse while that journal remains active.
  Failure reports the exact resume command and is resume-only at every
  journaled step, including ambiguous remote publication before its journal
  record. Success removes the active journal but keeps
  the backup.
  Shared-server control, TUI lease recovery, public opaque-ingress routing,
  authenticated actor resolution, exact TUI job forwarding, and response
  delivery are now active.
- **rclone is a soft prerequisite, not a startup gate.** Unlike
  `markdown-to-pdf`, a missing `rclone` never blocks `brain` from starting —
  `brain sync` itself just fails when it tries to spawn `rclone` and can't.
  `brain tasks doctor` (`src/tasks/doctor.rs`) reports rclone/sync health as
  one informational line (`rclone ✓ <version> · sync configured` or `rclone ✗
  not installed · sync off`), which never affects the doctor's overall
  pass/fail.

## Auto-sync triggers (startup pull, change push, receiver freshness)

The auto-sync layer (`src/sync/{lock,watch,trigger,freshness,current,follow}.rs`,
wired into `src/tui/event_loop/setup.rs` and `src/tui/app_sync.rs`) drives the
rclone handoff automatically. Every automatic trigger runs the sync in a
**detached background process**, never on a thread inside the shell, so a sync
can neither write over the TUI nor be killed when the shell quits. Its own
outside-world touchpoints:

- **The workspace sync lock** (`src/sync/lock.rs`) is a generation-tagged owner file at
  `<workspace-cache>/sync/sync.lock` (beside the sync journal, machine-local
  cache). Brain writes and syncs the complete PID plus random generation in a
  same-directory pending file, takes an advisory file lock, and atomically
  hard-links that inode into visibility. Only one sync runs at a time across every trigger
  for that UUID (the extras skip rather than queue), while different
  workspaces may sync concurrently. `Guard` owns a heartbeat thread that refreshes the lockfile mtime
  while the sync is still running. A later acquire advisory-locks the exact
  observed inode before reaping it when either
  the owner PID is dead (the same `server::lifecycle::pid_alive` `kill -0`
  probe the server uses) or the heartbeat mtime is older than the stale cap;
  that closes the SIGKILL + PID-recycle wedge. `Guard` stops the heartbeat and
  removes the file on drop, but only if it still holds **our generation**, so a Guard
  whose lock was reaped out from under it (a crash-recovery race) never deletes
  the new owner's lock. A missing or garbage lockfile reads as stale/reapable.
  The manual `run_sync` in `command/sync.rs` takes this lock too, closing a pre-existing
  concurrent-`brain sync` race.
- **The detached sync spawn** (`trigger::spawn_detached_sync(workspace, dir)`) is the one
  entry point the startup, watcher, and receiver-freshness triggers use. It
  spawns the current exe as
  `brain --brain <canonical-name> sync [--pull|--push] --if-idle` fully
  detached, with `BRAIN_WORKSPACE_ID=<selected UUID>` as a defense-in-depth
  expectation. Bootstrap compares that environment value to the registry UUID
  selected by `--brain` and refuses the command on malformed or mismatched
  input. `detached_sync_request` is the pure argv/environment builder and
  `DetachedSyncRunner` is the injected launch boundary used by concurrency
  tests. The real runner uses `process_group(0)` (its own process group, so it outlives the
  parent and survives terminal close) plus stdin/stdout/stderr all set to
  `Stdio::null()`, mirroring how `src/server/lifecycle.rs` spawns the server
  daemon, and needing no `unsafe`. Each child acquires the sync lock itself;
  `--if-idle` makes it exit silently when a sync is already running (coalesce),
  as opposed to a user-run `brain sync`, which *follows* the in-flight one.
  The owning TUI moves the `Child` into a waiter thread; `wait()` reaps it when
  complete, preventing the defunct-process accumulation seen with dropped
  child handles.
- **The in-flight state files** (`src/sync/current.rs`) let a detached sync stay
  observable without printing to any terminal. A running sync's `Reporter`
  appends every progress line to `<workspace-cache>/sync/current.log` (and echoes
  to its own stderr — the terminal for a foreground run, `/dev/null` for a
  detached one) and writes a `current.json` record (pid + direction + start)
  that it removes on drop. `brain sync status` reads that record (validated
  against `server::lifecycle::pid_alive`) to show `syncing now …`; a user-run
  `brain sync` that finds the lock held calls `follow::follow_until_done`, which
  tails `current.log` to the terminal until the run ends (`src/sync/follow.rs`).
- **The brain-owned bisync workdir** (`run::bisync_workdir` →
  `<workspace-cache>/sync/bisync`, passed as `--workdir` by `args::bisync_args`)
  fixes rclone's bisync state location so it is deterministic and its lock files
  are reapable. Because brain's own lock already serializes all syncs,
  `run::reap_stale_bisync_locks` removes any `*.lck` there before each run — it
  is necessarily from a dead, interrupted run (`.lst` baselines are preserved).
  An interrupted run that left the baseline unusable is detected by
  `parse_outcome` (the `--resync`/"cannot find prior"/critical-error family →
  `AbortKind::PriorListingMissing`) and self-healed by the existing one-time
  auto-resync in `command::sync_once`.
- **The gated two-workspace transport check** (`tests/sync_local.rs`) runs two
  local remotes concurrently with separate workspace UUIDs and production
  `WorkspacePaths`. It verifies distinct rclone workdirs and semantic CSV
  baselines, then presents one workspace with the other's remote manifest and
  proves refusal occurs before a bisync workdir or remote content write. It
  never reads a configured production remote.
- **The watcher's exclude set** (`watch::is_watch_relevant`, a pure path
  predicate) mirrors the bisync filter (see `args::bisync_args`'s default
  excludes above): a changed path under `.git`, `.cache`, or a `.DS_Store`, or
  an existing friendly conflict copy (`*(conflict *)*`), never triggers a sync.
  So a VCS write, a cache churn, or a conflict copy fanning in from another
  machine can't kick the watcher. The watcher runs `notify` recursively over the
  brain root; when relevant changes settle for the `debounce_ms` window it
  spawns a detached `brain sync --push`. Push uses `rclone copy --update`, so
  it cannot download files or delete remote-only paths. Task CSV and counter
  merges preserve remote rows/maximum values in the upload without writing
  those values locally. This removes the prior sync-write feedback loop. The
  debounce loop accepts an injected clock in tests. Its handle owns an explicit
  stop signal and worker join, so dropping one TUI stops only that workspace's
  watcher while peer workspace watchers remain live.
- **The receiver freshness gate** (`sync/freshness.rs` +
  `tui/app_sync.rs`) reads the newest successful downstream journal row at the
  live TUI's queued-job consumption boundary, not in shared-server dispatch.
  Before SMS/email dispatch, a missing row or age over two hours starts
  `brain sync --pull` and holds the message queue until a newer journal row
  appears. The footer polls `current.json` every 250ms and displays the active
  direction. An injected receiver runtime supplies monotonic and UTC clocks,
  current/journal reads, and child launch. The production policy gives a
  launched pull five seconds to appear and permits at most three attempts; if
  none starts, the TUI warns and processes the job with local state. There is
  no periodic pull timer and no exit sync.

## The auto-rebuild

`run.sh` rebuilds `target/release/brain` whenever `Cargo.toml` or any
`src/**/*.rs` is newer than the binary, then `exec`s it. This is the only
reason an agent's source edit "takes effect" without a manual
`cargo build` — but you should still build/test explicitly while
developing (see [testing.md](testing.md)).

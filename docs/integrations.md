# Integrations

`brain` is a single binary with a persistent TUI and short-lived command
families. It has no shell-mutating one-shot commands, so there is no plan
protocol and no zsh wrapper. The TUI owns interactive file opening, Finder
reveals, PDF conversion, trash, and agent launches by spawning processes
itself. This doc covers how the binary is run and each of those handoffs, plus
the frontend lifecycle bridges, UUID-scoped state DB, shared TUI-lifetime
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
  Managed triage rows complete like any other habit here; only removal,
  revival, and skipping are refused. A caller with no id in hand instead uses
  `brain habits complete-managed-triage daily|weekly` (native) or the equivalent
  `apply_sync_rules.py --complete-managed-triage daily|weekly`, either of which
  selects the row by `system_key` and becomes a no-op when the portable feature
  is disabled.
  All Rust task mutations and bundled Python CSV/counter writers acquire the
  same SQLite immediate transaction at
  `<workspace-cache>/tasks.transaction.lock`. Portable config read-modify-write
  operations, the habits web completion route, and bundled Python project
  metadata writers use that owner too. Python CSV and JSON writers reject a
  changed read snapshot and use a synced same-directory atomic replacement.
  The protected `remove_task.py` boundary rejects enabled managed-row deletion,
  and refuses any habit row unless the caller passes `--habit`: deleting a habit
  row destroys its whole recurrence chain, so a task-cleanup pass must not be
  able to reach one. Task-cleanup callers (notably the bundled `triage` skill,
  which excludes habits entirely) never pass that flag.
- **`brain tasks doctor`**: prints a progress plan before checking the selected
  UUID-scoped state DB schema, every registry-declared lifecycle artifact,
  OpenCode executable compatibility, `rclone version`, and the centralized
  selected-workspace requirements. Hook commands are checked by event and
  current script suffix; Brain-owned bridge and plugin files require exact
  bundled contents, so stale files fail independently. It
  opens SQLite through an immutable read-only URI so a WAL database cannot be
  checkpointed by observation, passes an explicit no-config path to the rclone
  probe, and does not create cache, config, lock, journal, or rendered-skill state.
  OpenCode probes use disposable HOME/XDG roots and remove them afterward.
- **`agenda` zsh function** — `Ctrl+A` runs it via the injected `ShellRunner`.
- **`brain habits`**: when no TUI is open, elects the shared process in
  background mode, attaches a temporary browser-only workspace lease, and
  opens `/local/<exact-live-lease>/w/<selected-ingress>/habits` via the system
  `open`. A TUI can subsequently reuse that process and replace the temporary
  lease. A second start is rejected. `brain habits kill` removes the
  background lease and stops the process, but refuses while a TUI lease is
  live.
- **Palette "Open habits in browser"**: uses the current TUI lease and does
  not start or stop the shared process.
  The process itself carries no
  selected `--workspace`; each habits request instead carries an opaque ingress ID.
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
  complete Brain's managed triage occurrence without knowing its id: mark
  today's occurrence done + spawn the next, keyed on the stable `system_key`
  rather than an id that changes each cycle. The logic lives in
  `tasks::triage_habits::complete_managed` (reusing `tasks::complete`'s
  `read_csv`/`write_csv`/`spawn_next_occurrence`); the same
  `complete_managed_triage` entry point backs the daily-triage nudge's **Skip**
  button (`App::skip_triage`), so the button and the CLI share one Rust path. A
  no-op that mutates nothing when `enable_triage_habits` is off. The native
  equivalent of `apply_sync_rules.py --complete-managed-triage`.
- **Receiver server** - the machine-wide TUI-lifetime process accepts
  `POST /sms` and `POST /email`, two machine-wide paths that name no workspace,
  only for an enabled workspace with a live lease. The workspace is selected
  from the destination the provider named (Twilio's `To`, a Resend payload's
  `to`/`cc`) matched against each registered workspace's own
  `twilio_from_number` / `resend_from_email`; that selection precedes all
  workspace provider and user reads. Twilio requests must pass the exact
  URL/form HMAC — over the one machine-wide URL — and the SMS sender allowlist.
  Resend requests must pass the official `v1,<signature>` Svix verification, a
  five-minute timestamp window, and the email sender allowlist. Inbound
  addresses arrive as RFC 5322 mailboxes, so the sender and every thread
  participant are reduced to a bare address (`crate::users::normalize_mailbox`)
  before either is compared with a configured identity. Successful
  Resend deliveries receive HTTP 200, and the Receiving Email plus Receiving
  Attachments APIs supply the full body and signed download URLs. Success for
  an accepted inbound SMS or Email job follows an exact durable workspace-job
  commit or durable provider-ID dedup hit. The process stops when its final live
  TUI lease is removed or expires.
- **`<agent_cmd> …` with cwd set to `<root>`**: the brain panel's PTY,
  shared by both main views (see below).

This is the "central dispatch" design: `brain` is the single terminal command,
and each capability is either an in-process main view (tasks, brain-directory
search) or a spawned process it drives (Claude, Codex, or OpenCode for conversational work,
Finder/editor for files, `markdown-to-pdf` for conversions).

## The Brain Panel: Claude, Codex, Or OpenCode

The persistent shell's `BrainPanelState` owns the main and skill-session
`AgentController`s. The App mediator assembles launch context, while each
controller spawns the selected agent frontend inside a PTY (`pty_pane.rs`).
`BrainPanelState` derives the completion actor from the controller at install
time; callers cannot supply a second actor that disagrees with the controller
used for launch and completion validation.

```text
TuiRuntime
└── App mediator
    ├── AppContext (workspace, frontend, config, paths)
    ├── BrainPanelState
    │   └── AgentController
    │       └── frontend registry -> Claude | Codex | OpenCode adapter -> transport
    ├── AppServices (session DB, runners, receiver sync adapter)
    └── ReceiverRuntime (receiver-local queue, intent, and decisions)
```

Launch, completion delivery, receiver takeover, and cross-feature focus stay
on App. No one of those paths selects or invokes a concrete frontend outside
`AgentController`.

TUI consumers name the owning agent, state, receiver, overlay, palette, or
action module directly. The TUI root does not re-export child modules as a
wildcard namespace, so importing an integration surface cannot silently grant
access to unrelated frontend or runtime details. Receiver execution state under
`ReceiverRuntime` remains live process state; receiver jobs, logical
conversations, frontend-neutral sessions, and completion records are durable.

**Which frontend runs.** A selector flag wins; with none, the selected
workspace's machine-local `default_agent_frontend` env value decides; with that
unset (or holding an unreadable value), Claude runs. The flags are
`--claude` / `-cl`, `--codex` / `-cx`, and `--open-code` / `-oc`, and each may
appear before or after `tasks` and its delegated positionals. So
`brain env set default_agent_frontend=codex` makes Codex this machine's default,
and `brain --claude` still opens Claude for one run. The flags parse in
`Cli::selected_agent` (which returns `None` when nothing is selected, since env
can only be read once a workspace is bootstrapped) and resolve in
`agent::default_frontend`; `main.rs` resolves the pair right after bootstrap.
The OpenCode adapter
launches `opencode` with the named Brain agent, translates semantic input to
OpenCode control sequences, and supplies the trusted Brain policy through
`OPENCODE_CONFIG_CONTENT`. The installed `.opencode/plugins/brain.js` bridge
maps OpenCode `session.created` and `session.idle` events into Brain's generic
lifecycle bridges. Selecting more than one frontend exits with
`🔴 Choose one agent frontend: --claude, --codex, or --open-code.`

| Frontend | Command source | Resume/fresh command shape |
| --- | --- | --- |
| Claude | `claude_cmd` in brain env, default `claude --dangerously-skip-permissions` | `<claude_cmd> [--mcp-config <cache-json> --strict-mcp-config] --resume <id>` or `--session-id <id>` |
| Codex | `codex_cmd` in brain env, default `codex` | `<codex_cmd> --dangerously-bypass-hook-trust [-c <capability-override>...] resume <id>` when the exact session rollout remains on disk; otherwise the same base launch without `resume <id>` starts fresh |
| OpenCode | `opencode_cmd` in brain env, default `opencode` | `<opencode_cmd> --agent brain [--session <validated-id>] [--prompt <initial-prompt>]`; lifecycle uses the workspace Brain plugin. |

The crate-private `agent::ClaudeFrontend`, `agent::CodexFrontend`, and
`agent::OpenCodeFrontend` adapters own these command shapes and splice the
configured base command in verbatim so it may carry its own flags. Shared and
external callers cannot construct an adapter or issue an adapter operation;
they use `AgentController`, with `LaunchSpec` and `InputSequence` exposed only
as transport-boundary values. `agent::registry` is the exhaustive integration
table: each row owns identity, command key/default, construction, command
building, lifecycle installation descriptors, health descriptors, capability
evidence, and any compatibility probe. TUI, setup, doctor, status, and
compatibility helpers consume that registry instead of growing parallel
frontend branches. All frontend operations are fallible so the controller can
reject availability and setup before a transport side effect.

OpenCode compatibility is a supported-feature policy, not a promise about
every future release. Before launch, and during doctor, Brain checks the
configured command for a runnable version, `--agent`, `--prompt`, and
`--session`, JSON `session list --format`, `debug config --pure`, Brain's
generated capability schema, and loading of the bundled lifecycle plugin.
Those contracts are anchored to OpenCode's official [CLI](https://opencode.ai/docs/cli/),
[configuration](https://opencode.ai/docs/config/),
[agent](https://opencode.ai/docs/agents/), and
[plugin](https://opencode.ai/docs/plugins/) references.
These probes run with disposable HOME and XDG roots and bounded output/time;
they do not alter the user's OpenCode state. Successful reports are cached by
configured command for that Brain process. A future OpenCode version remains
supported when those tested surfaces remain compatible; otherwise Brain fails
with the missing capability and an `opencode_cmd` remediation.
Before the main panel claims a free resumable session, it resolves the selected
workspace's capability plan and asks the adapter for the candidate's stable
response identity. A validation or identity error therefore cannot strand a
free session as claimed. If the later transport launch fails, Brain releases
the instance claim and clears the response identity for the attempted
interactive or receiver launch.

The isolated receiver-run path uses a frontend-neutral planning seam. A
matching durable conversation binding is
offered only to the selected `AgentController`; `Resume(session_id)` is chosen
only when the adapter confirms that native history and an injected exact-session
claim succeeds. Missing history, a frontend change, a probe error, or a failed
claim produces `Fresh(session_id)`. Resumed launches carry only the current
authenticated message and attachment references. Fresh launches carry those
same current inputs after a separate portable-transcript section, bounded to
64 KiB while retaining the newest UTF-8-safe transcript suffix. Prompt,
transcript, attachment, sender, recipient, and credential contents never enter
planning diagnostics. Claude, Codex, and OpenCode translate both semantic
plans with the non-blank initial prompt through their existing launch command.
The adjacent ownership seam gives every run a unique remote
`BRAIN_INSTANCE_ID`, registers a fresh Brain-supplied ID before spawn or claims
the exact validated resume session, and never reuses the main TUI instance.
Claude may use that registered ID directly as its native session ID; Codex and
OpenCode session-start integrations must rotate it to a distinct native ID.
Only an exact locked remote instance whose complete durable registration tuple
matches the workspace, logical conversation, frontend, actor, channel,
instance, and registered ID may replace the durable binding. Equality is valid
only for Claude with that exact lifecycle evidence; unproved placeholders and
equality for Codex or OpenCode are rejected. The lifecycle-reported native ID
is retained in that registration, and the binding-only update does not rewrite
the portable transcript. Pre-launch rollback stops the controller,
uses a fallible exact-registration cleanup, and still records its bounded
durable retry before surfacing any shutdown or cleanup diagnostic; `Drop` is
only a best-effort fallback.
Controller shutdown always reaches the transport even when frontend
availability diagnostics fail, while still returning that diagnostic to
orderly shell teardown. The sole receiver tick consumes the durable queue,
passes the result through this seam, and gives every launched run its own
background tab and controller.

The TUI owns an `AgentController` for each live main or triage panel and calls
semantic type, immediate submit, busy-turn follow-up, new-session, snapshot,
completion, resume, and shutdown operations. The
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
`developer_instructions` config override. Claude and Codex place the ordinary
user prompt after their option terminator; OpenCode passes it as one quoted
`--prompt` value. Prompt text that begins with `-` therefore cannot become a
frontend flag or config override.
Fresh, resumed, interactive, SMS, email, and daily-triage
requests use the same policy construction. Unrestricted mode adds no policy
instruction.

OpenCode receives the policy through the Brain-owned `agent.brain.prompt` in
`OPENCODE_CONFIG_CONTENT`; launch always selects that agent with `--agent
brain`. Brain first parses inherited `OPENCODE_CONFIG_CONTENT` as a JSON object.
Unrelated top-level and nested values survive. Brain replaces only
`agent.brain`, `default_agent`, stale `mcp.brain_ws_*` entries, and the selected
workspace's new `brain_ws_*` MCP entries; it appends the selected actor-local
skill directory once under `skills.paths`. A non-object at any required merge
location fails before launch. The explicit inline variable therefore has
highest precedence for Brain-owned launch keys without discarding unrelated
user configuration.

The PTY starts from an empty environment. For OpenCode only, Brain restores the
ambient documented `OPENCODE_*` namespace so configuration paths, TUI settings,
and other frontend controls continue to work. It then replaces
`OPENCODE_CONFIG_CONTENT` with the validated merged value above, so inherited
user configuration survives without allowing an ambient value to displace
Brain's reserved agent and capability layer. Claude and Codex do not inherit
that namespace.

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

OpenCode selected MCPs use the same UUID-and-name-derived `brain_ws_*`
namespace and environment references for credentials, so secrets remain out of
argv and inline JSON. The selected skill directory and deny-by-default skill
permission map are added to the Brain agent. Brain can prove that it generated
and schema-probed these entries, but it cannot prove that inherited global MCP
or skill sources are excluded by OpenCode. OpenCode capability evidence is
therefore `advisory-only`, matching Codex's honesty boundary.

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
stale Codex/OpenCode runtime artifacts and abandoned Claude temporary files; a
Codex or OpenCode launch removes stale peer artifacts. Unrestricted launches remove the
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

When Brain injects a prompt into an already-open panel, the caller requests one
semantic busy-turn follow-up from `AgentController`. The selected adapter owns
the complete native sequence: Claude and OpenCode encode literal text followed
by `Enter`; Codex encodes literal text followed by `Tab`, its native queue key.
The controller does not expose or duplicate those keystrokes. The TUI may hold
a receiver job until the current turn is eligible for handoff, but once it
calls the facade the frontend's native busy-turn behavior owns the follow-up.

Injected text is always delivered as one **bracketed paste**
(`ESC[200~` … `ESC[201~`, DEC mode 2004, in `src/agent/input.rs`), exactly as a
terminal hands over clipboard content: newlines become the `CR` a real paste
carries, and only the semantic submit or queue key lands as an actual
keystroke, after the paste closes. All three frontends enable mode 2004 and
insert the payload literally, so this is the frontend-neutral way to inject a
prompt. It is what makes injection safe when the composer is in vim mode: the
`ESC CR` "literal newline" chord Brain used previously left insert mode, so the
rest of a multi-line prompt ran as normal-mode commands and the message sat in
the composer unsubmitted forever. Control characters are stripped from the
payload, so inbound message text cannot close the paste early and have the
remainder execute as keystrokes.

The paste and the submit key are **two separate writes**, and the key waits
`PASTE_SETTLE` (400 ms, `src/agent/input.rs`) before it goes out. A frontend
handles a paste and a keystroke on different paths — the paste is buffered and
applied to the composer, the key is dispatched straight to the focused handler —
so a key sharing the paste's write can be handled against a composer the paste
has not reached yet, submitting nothing and leaving the prompt sitting there.
`InputSequence` is therefore a list of `InputWrite`s (each with a `settle`
delay) rather than one buffer, and `PtyPane`'s writer thread owns the wait so
the event loop never blocks on it. The loss was measured on Claude; Codex and
OpenCode submitted either way. Every frontend is paced regardless, and the
adapter contract test asserts it for all of them.

The TUI separately tracks whether a prompt has actually been submitted.
Opening the interactive panel is therefore not itself considered active work.
Receiver work no longer consumes or replaces that panel: every SMS or email job
gets a separate background controller and PTY. The main-panel Stop response
still clears only its own active-turn state. A failed receiver launch releases
its exact remote registration and records a durable pre-acceptance retry.

Receiver behavior is frontend-neutral after authentication. An SMS or email
job carries the same immutable workspace, actor, channel, response email, and
allowed-thread recipients into an OpenCode `LaunchRequest`; the plugin's idle
event reaches the generic completion bridge, and the ordinary response worker
delivers the authorized artifact. OpenCode receives no special delivery bypass
and cannot broaden recipients.

### Skill-session tabs and their completion signal

A **skill session** runs one prompt in its own ephemeral brain-panel tab
(`BrainPanelState`, `tui/app_skill_session/`) rather than typing that prompt
into the main session. Daily triage is the builtin definition (the nudge's **Yes**
path); the rest come from the machine-local `skill_sessions` env array. Each is
launched through an `AgentController` and a fresh `LaunchRequest`, with three
deliberate differences from the main panel:

- **It is never tracked.** Its `HookMetadata` contains only
  `BRAIN_SESSION_DONE_URL` and `BRAIN_SESSION_TOKEN`. The selected adapter adds
  the common workspace identity and `BRAIN_AGENT_KIND`, while
  `BRAIN_INSTANCE_ID`, `BRAIN_STATE_DB`, and `BRAIN_RESPONSE_ID` remain absent
  (`session::env_for_skill_session`). The session-start bridge requires those
  tracking variables in addition to workspace identity, so a skill session is
  never written to `brain_sessions` and is never a resume candidate.
- **The protocol travels with the prompt.** brain cannot assume the skill it
  launches knows anything about brain, so
  `skill_session::prompt::launch_prompt` appends the completion protocol —
  "POST the token in `$BRAIN_SESSION_TOKEN` to `$BRAIN_SESSION_DONE_URL` as your
  very last action, with the paths of any outputs you were told to produce in
  `require`" — to whatever prompt the workspace configured. A user's own skill
  therefore needs no edits to participate.
- **Completion is signalled, not inferred.** A run can involve back-and-forth
  with the user, so "the agent went idle" is not a reliable done signal. brain
  connects to the shared process already attached to the TUI and passes its
  capability-protected local session-completion URL plus a one-time token into
  the session. When the run finishes it POSTs
  `{"token": "<token>", "require": ["<path>", …]}` to that URL. The process and
  the TUI are separate processes, so the signal crosses on disk: the
  `routes::session` handler records it to
  `<workspace-cache>/skill-sessions/<token>.json` via
  `crate::skill_session::signal::record_done`, and the TUI's per-tick
  `App::tick_skill_sessions` asks `BrainPanelState` for each open tab's token
  (`signal::read_signal`) and auto-closes that tab only when its token arrives
  **and** every path in `require` exists (`signal::ready_to_close`). One file per
  token is what lets several sessions run at once without one's completion
  closing another's tab; the token also means a stale signal from an earlier run
  can't close a freshly-opened tab, and the shell clears every pending signal at
  startup (`signal::clear_all`). The `require` gate means a *premature* signal
  can't close the tab before the run's declared outputs are written (the signal is
  held, re-checked each tick, until they exist). **Core knows nothing about what
  those outputs are** — `require` is empty unless the run declared a path (for
  daily triage, an extension rendered in at the
  `triage:daily-required-outputs` hook), and an empty list closes immediately, so
  the generic core (and any fork) behaves exactly as before. If a session's child
  exits on its own, the same tick closes its tab regardless.
  Tab identities are lifetime-monotonic within the App. Checked allocation
  refuses exhaustion without changing the counter or open-tab collection,
  shuts down the controller that cannot be installed, clears its pending
  completion token, and reports the launch failure in the TUI.

`brain server`'s route table therefore includes
`POST /local/<lease>/w/<ingress>/session/done` (see `server/router.rs` plus
`server/routes/session/`), an unauthenticated localhost-only endpoint consistent
with the ingress-scoped habits completion route.

### Shared ephemeral-tab storage for receiver runs

`BrainPanelState` stores skill sessions and receiver runs in one ordered
ephemeral collection, but the metadata variants remain distinct. A skill entry
owns its configured key and completion token. A receiver entry owns its durable
`ReceiverJobId` and remote instance identity. It does not borrow a
`SkillSessionKey`, completion token, or configured-skill semantics.

All entries share the checked monotonic `SessionTabId` allocator, rendered
title order, controller lookup, and orderly shutdown pass. A receiver
allocation rejected at counter exhaustion shuts down the supplied
`AgentController` without inserting a tab or advancing the counter.
The `BrainPanelState` receiver API performs insertion, observation, controller
access, and removal without touching `ShellState`; therefore background
operations preserve the current main view, effective tab, panel visibility, and
keyboard focus.
Receiver-only storage does not reveal a hidden panel. Durable FIFO claiming,
launch registration, rollback, renewal, and exact terminal cleanup remain
behind narrow `AppServices` operations. Background launch and close never
select a tab, reveal the panel, switch the main view, or move keyboard focus.

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
actions are read-only `status` and `logs`; `brain killall` is the explicit
machine-wide emergency cleanup surface for shared servers and TUI processes.
Only live TUI startup and heartbeat recovery may call the electing
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
status`, `brain receiver status -w <workspace>`, `brain receiver url`, bare
`brain receiver`, and `brain receiver {email|phone}`
skip run-log creation and
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
`/{sms,email}` route or local `/local/<lease>/w/<ingress>/...` capability route.
A provider route then resolves its workspace from the destination inside the
already-read body (`server::receiver::routing`), which yields the ingress this
process remembers for that workspace. The shared process then captures a
generation-bound ticket for the exact accepting lease. Only that ticket permits registry, root,
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
Immediately before durable admission, dispatch also reloads the exact
canonical registry record, verifies the selected workspace UUID, and requires
its persisted receiver intent to remain enabled before revalidating the live
generation and authority revision. A persisted disable that races after route
loading therefore cannot enqueue even when its best-effort refresh notification
was lost. At admission commit, that filesystem reload remains outside the
control mutex. One combined operation then acquires control, samples the
monotonic instant inside the lock, revalidates exact route and admission
identity, and performs the admission CAS before unlocking. The SQLite
transaction then accepts or deduplicates the complete immutable job while
revocation waits on that linearized admission outside the control mutex.
Unknown ingress returns 404. Receiver-disabled or no-live-TUI ingress returns
503 for provider-facing receiver dispatch. Local capability routes remain
available to a live TUI or browser-only habits lease even when inbound
receiver intent is disabled.

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

TUI startup flows through named `TuiRuntime` builder stages. They order
ownership as workspace readiness, UUID singleton, hook/skill refresh,
UUID-local `jobs.sock`, bounded connect/elect/register handshake, heartbeat
worker, assignment, terminal, App/session state, initial agent panel, startup
sync, watcher, and periodic puller. If the selected generation exits between discovery and
registration, the handshake re-enters election and registers with the winner;
an authoritative workspace rejection returns immediately.
After registration, a partial-start boundary retains the heartbeat lease before
the bound job socket across every remaining fallible stage: assignment and
terminal setup, DB/config/App initialization, initial-panel launch, and startup
workers. A startup error drops and unregisters the lease before removing the
socket. Only an otherwise-complete runtime installs the socket into the App, and
that transfer has no fallible work after it.
The worker sends one heartbeat per second. Missing transport, a stale
generation, or a lost lease triggers bounded election/reuse and re-registration;
concurrent TUIs use the same election path so only one replacement wins.
The runtime tick drains health events before skill-session, receiver, sync, and
triage work. Orderly exit stops the worker and unregisters before agent
shutdown, periodic-puller and watcher drop, session-lock release, terminal
restoration, and final singleton release. The receiver-owned `jobs.sock` stays
inside the App for the complete live runtime. Startup passes one owned
`TuiLaunch` request to `run_tui`; the runtime converts it to one internal
`AppInit` request, and the resulting `App` retains no borrowed launch data.

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

These triage rules are identical for Claude, Codex, and OpenCode. OpenCode's
plugin may observe the ephemeral root session, but the generic session-start
and session-stop bridges no-op without the tracking attribution intentionally
omitted from a skill-session request; only the one-time session-done signal closes
the tab.

## Agent sessions: lifecycle bridges and state DB

Which session to run is decided by the **lock + recency** model in
`state/` (DB at `<workspace-cache>/state.db`, WAL):

1. At ordinary command bootstrap brain resolves the local actor once. TUI
   startup first acquires the workspace singleton, then refreshes every
   registry-declared lifecycle artifact before opening or migrating the state DB,
   reaps locks held by dead
   PIDs, then walks `sessions_by_recency()` within the exact
   frontend/workspace/actor/channel scope
   and asks the selected adapter to validate each candidate. Claude requires
   `~/.claude/projects/<mangled selected-root>/<id>.jsonl` (its project-dir
   rule plus a fallback scan). OpenCode takes one read-only snapshot from
   `session list --format json` in the selected root and accepts only live,
   non-archived, non-deleted root sessions whose reported directory resolves
   to that exact root. Child sessions and another workspace's IDs are never
   resume evidence. Codex accepts a candidate only when its exact rollout
   remains on disk. If Brain claims a valid candidate it uses the adapter's
   resume shape; otherwise it
   starts fresh and, if it skipped a stale candidate, shows a status-line alert:
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
   **`BRAIN_WORKSPACE` is also an implicit workspace selector.** A `brain`
   invocation with no `-w`/`--workspace` adopts it, so a bundled skill,
   an agent, a hook, or a `reindex` child that runs `brain config get …` inside a
   `family` panel operates on `family` instead of the machine default. An
   explicit `-w` always wins, and a plain shell with the variable unset still
   uses the default — which is what a person typing `brain` wants. Because it is
   an environment variable it survives subshells, so a subagent running in its
   own shell inherits the same workspace. With neither a flag nor the variable,
   Brain discovers the workspace from the current directory (nearest ancestor
   whose registered root contains it, like git); the launching workspace
   outranks that, so a `cd` inside a session never retargets it.
   `BRAIN_WORKSPACE_ID` continues to *validate* the resolution: a selector that
   resolves to a different UUID fails rather than acting on the wrong workspace.

   Receiver work first authenticates the provider request, then resolves an
   enabled portable sender; the queued workspace UUID and actor override the
   machine default for that complete request lineage. The accepting pipeline
   also captures the initiating user's normalized `response_email` and only
   allowlisted participants from that authenticated thread. Both sides of that
   intersection, and the configured receiving address that is excluded from it,
   are reduced to bare addresses first, so a display-name `from`, `to`, or
   `resend_from_email` can neither strip the thread of recipients nor defeat
   the self-echo guard. Every outbound email reply passes through one seam
   (`App::send_email_reply`), which logs an empty recipient set instead of
   dropping the reply silently. Claude, Codex, and OpenCode
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
3. The generic **session-start bridge**,
   `scripts/agent_session_start_hook.py`, is wired into Claude and Codex
   `hooks.SessionStart`; OpenCode's workspace plugin invokes it for a root
   `session.created` event. It fires on
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
4. The generic **session-stop bridge**
   (`scripts/agent_session_stop_hook.py`) records the turn's final
   assistant message under
   `<workspace-cache>/responses/<response-id>.json` only while the exact
   frontend/workspace/session/actor/channel/instance tuple is still locked in
   the session store; an unregistered, released, or rotated completion is
   ignored. It resolves that message defensively: it prefers the payload's
   `last_assistant_message` convenience field and, when a Claude Code build
   omits it, falls back to parsing the last assistant text message out of the
   Stop payload's `transcript_path` JSONL. Delivery therefore never hinges on a
   single optional field; a turn with no recoverable final text is the only
   no-op. The hook stages a unique, synced file, starts `BEGIN IMMEDIATE`,
   rechecks that locked tuple, and updates the same predicate only when exactly
   one row matches. It publishes and syncs the artifact before committing the
   `completed` state. A publication or commit failure rolls back the database
   and removes or restores only the file owned by that attempt. A concurrent
   SessionStart rotation serializes at the transaction boundary, so a stale
   Stop event cannot complete the prior lineage. The stable response ID is
   independent of the frontend session ID, which gives Codex turns the
   same completion path as Claude and OpenCode. The artifact includes frontend,
   workspace, session, response, actor, channel, and completion status. An
   interactive turn accepts only its launched session context.
   An active receiver run additionally requires the exact durable job, remote
   instance, response ID, frontend, actor, channel, and locked session in
   `completed` state. Process spawn and screen activity are never acceptance or
   completion evidence. On a valid terminal completion, Brain sends the
   channel-specific reply, moves the exact launch directly to `done`, releases
   that remote session owner, shuts down its controller once, removes only its
   tab, reloads tasks, and starts an immediate sync push. Direct
   `launching`-to-`done` is temporary until BR-15 supplies accepted and
   processing proof.
   If the receiver child exits without that exact artifact, Brain releases the
   registration, shuts down and removes only that tab, and records a durable
   pre-acceptance retry. If claim renewal loses ownership, it performs only
   local controller and tab cleanup; it does not mutate lifecycle, reply,
   session, or job state owned by the winner.
5. When the panel closes (the agent exits) or the shell quits, brain `release`s
   its lock, floating that session to the top of the resume queue — so
   "Message brain" (`Ctrl-M`) re-opens it, and a fresh startup resumes it.

**Durable receiver conversations are separate from interactive session
selection.** `state::receiver` persists one logical conversation for the exact
workspace/user/channel/channel-key tuple. Its optional frontend/native-session
binding says which adapter may attempt an opaque native resume. A same-frontend
request returns that native ID; a different frontend receives a fresh-session
plan seeded from the Brain-owned markdown transcript. Brain never asks Claude,
Codex, or OpenCode to interpret another frontend's native history.

Receiver jobs use the same UUID-scoped database but a separate leased queue
contract. Acceptance stores the immutable inbound frame before a later ingress
ack can depend on it. Polling claims the oldest ready row without deleting it,
and every renewal, transition, or retry mutation requires the exact live owner.
An expired progressed lease changes ownership without erasing its lifecycle or
retry evidence. The current receiver consumer deliberately does not rerun a
reclaimed progressed state. It cleans up the tentative registration and leaves
that evidence intact for BR-16 recovery policy.

BR-13 connects provider ingress to these APIs. The authenticated pipeline
constructs SMS's stable workspace/user/channel identity or an uncertain fresh
Email lineage, then commits or durably deduplicates before provider success.
The existing TUI in-memory queue is no longer the ingress acceptance boundary
or an execution consumer. Prompt submission, completion, delivery, and durable
queue consumption remain in the live TUI and never move into the shared server.

Claude and Codex register the same generic bridge scripts. Claude stores
root-anchored `SessionStart` and `Stop` entries in
`<brain-root>/.claude/settings.json`; Codex stores them in
`<brain-root>/.codex/hooks.json`. Each command resolves a script below that
workspace's `.brain/hooks/` directory. Brain-launched Codex sessions include
`--dangerously-bypass-hook-trust` because Brain generated and byte-verifies the
hook sources. This avoids an interactive trust prompt for the lifecycle bridge;
it also means a user who adds unrelated enabled project hooks should review
those hooks before launching that workspace through Brain.

OpenCode installs one exact workspace plugin at
`<brain-root>/.opencode/plugins/brain.js`. On a root `session.created`, the
plugin sends `{session_id, source}` to the generic session-start bridge. On
`session.idle`, it resolves the reported session through the OpenCode client,
rejects child sessions, fetches messages for that selected directory, and sends
only the newest completed, non-synthetic assistant text to the generic
session-stop bridge. Repeated idle events remain safe because the Python
bridge authorizes and publishes against the exact active DB tuple. The plugin
passes payloads over stdin, forwards only the narrow runtime and `BRAIN_*`
environment allowlist, and logs lookup or bridge failures through OpenCode.

Every registry-declared static workspace artifact is confined to the selected
workspace before any installation directory is created. Brain resolves leaf
and parent symlink chains, canonicalizes existing ancestors while retaining a
missing tail, and rejects a destination outside the workspace without touching
the referent. A symlink whose final destination remains inside the workspace is
preserved and updated atomically. The standalone repair installer applies the
same confinement before copying a bridge or plugin.

The plugin stays deliberately thin. OpenCode-specific event names and SDK calls
belong in JavaScript, while session rotation, tuple authorization,
deduplication, atomic response publication, and receiver delivery remain in
Brain's generic Python bridges and SQLite transaction. This keeps one security
and delivery contract for all frontends instead of reimplementing DB authority
inside a frontend plugin.

All three frontends can launch non-Python commands. Brain keeps these two
bridges as Python 3 scripts because the shipped standard-library implementation
already provides the JSON, SQLite, locking, and atomic-file behavior needed at
the hook boundary, while a second Rust executable would add build and install
coordination without removing the frontend-specific OpenCode adapter. Python 3
is therefore an explicit runtime prerequisite for lifecycle integration, not an
assumption that every agent shell supplies it implicitly. The standalone hook
installer checks it before changing a workspace.

**One hook namespace, one DB per workspace.** Before the merge, `brain` and `tasks`
each ran their own SessionStart hook keyed on separate env-var namespaces
(`BRAIN_*` vs `TASKS_*`) writing separate DBs, so the two shells never adopted
each other's sessions. The merged shell has a single app-level brain panel, so
there is now exactly one generic lifecycle protocol, keyed on `BRAIN_*`, one DB
per workspace UUID (`<workspace-cache>/state.db`, table
`brain_sessions`), and
one namespace. Registry-driven installation deploys the two generic scripts into
`<brain-root>/.brain/hooks/` and registers them in that workspace's
`.claude/settings.json` and `.codex/hooks.json`.

**Hook commands are root-anchored, never working-directory-relative.** The
registered Claude command is
`python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/<script>.py"`.
Claude runs a hook in the session's *current* working directory, not the
project root, and its Bash tool's `cd` persists for the rest of the session, so
a project-relative command stops resolving the moment an agent changes
directory. `CLAUDE_PROJECT_DIR` is the project root Claude exports for exactly
this purpose; `BRAIN_ROOT` covers a session Brain launched. The Rust installer
and `install_hook.sh` emit the same
command, and reinstallation replaces a stale relative command in place (stale
entries are matched by script basename, ignoring surrounding quotes).

Codex reads the selected workspace's `.codex/hooks.json`, so Brain emits
`python3 "${BRAIN_ROOT}/.brain/hooks/<script>.py"`. `BRAIN_ROOT` selects the
workspace explicitly. No absolute path is baked into either hook file, because
both are read on every synced machine.

`scripts/install_hook.sh` deploys the generic session-start and session-stop
bridges, Claude/Codex workspace hook settings, and the
OpenCode plugin from the same lifecycle registry contract. It strips stale
legacy commands by script basename while preserving unrelated settings. Every
ordinary Brain startup does the same automatically for every existing configured
workspace before command dispatch; `brain receiver setup` also refreshes every
registered frontend. Help and version are the only public no-write exceptions.
The 0.71 lifecycle migration removes global registrations immediately, but
retains workspace-local forwarding shims for the legacy script paths that an
already-running frontend may have cached in memory. Those shims execute the new
generic workspace hook and are not referenced by current Claude, Codex, or
OpenCode configuration.
The automatic 0.72.0 migration reconciles receiver schema v6 in every
registered workspace that already has a state DB. It does not create an unused
DB merely because the workspace is registered. Its down operation removes only
the receiver tables/index and returns an existing DB to v5. Freshly attached
workspaces receive the current schema on their first `Db::open`. The automatic
0.75.0 migration adds schema-v7 launch retry origins to existing receiver jobs;
its down operation removes only that column and returns the state DB to v6. No
manual migration command is part of receiver setup.
The standalone
`./scripts/install_hook.sh [brain-root]` remains a repair path for users who
change Claude, Codex, or OpenCode integration state manually. Its root
precedence is the explicit argument,
then `BRAIN_ROOT`, with `$HOME/brain` retained only as the manual installer's
single-workspace fallback. The session-stop bridge is
required for receiver jobs: it records the completed assistant response so the
TUI can deliver it over SMS or email without exposing the full thinking trace.

Receiver setup stores provider credentials in the selected workspace's record
in the machine-local brain env store, and the public base URL in that store's
machine-global map, because one machine serves one origin. Enter the public base
URL only, for example `https://brain.example.com`; the Twilio portal receives
`https://brain.example.com/sms` and the Resend portal receives
`https://brain.example.com/email`. Both are the same for every workspace on the
machine: brain routes each inbound message by the number or address it arrived
at, so a workspace is distinguished by its `twilio_from_number` /
`resend_from_email`, never by its URL. Twilio signs the exact SMS URL, so the
receiver rebuilds that one path before verification. Ordinary provider
resolution uses the selected record plus that machine-global origin; Brain does not treat process-level `TWILIO_*`,
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
portable-user bytes, and selected registry-declared lifecycle artifacts,
excluding their
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

Receiver ingress durably admits one bounded serialized `InboundJob` carrying the
workspace UUID, immutable actor and channel, normalized sender, prompt, stable
provider email/attachment IDs, provider ID, and acceptance-time response
metadata. After final persisted-intent, route-revision, lease, and admission
checks, the shared server opens the UUID-scoped state DB with the remaining
handoff duration already installed as SQLite's busy timeout. Schema
reconciliation and the atomic acceptance transaction therefore cannot inherit
the ordinary five-second lock wait. Provider and exact-job deduplication run
before the atomic 64-row `queued` capacity check. Provider success follows only
the commit or an existing durable match. The mode-`0600`
`<workspace-cache>/jobs.sock` remains part of live-lease validation and the
legacy TUI representation, but provider ingress no longer appends to its
private memory queue and no production coordinator consumes it.
One private `ReceiverRuntime` owns persisted intent, the sync-freshness gate,
and a `DurableReceiverRun` handle. The one `App::tick_receiver` call is the sole
production consumer. It runs only while receiver intent is enabled, renews an
already claimed job before a pending freshness pull, and otherwise does no work
while a receiver tab is active. When ready, it claims the oldest durable job by
`(received_at_unix_ms, job_id)`, loads the immutable job and conversation,
plans through the selected `AgentController`, prepares launch through the exact
claim, registers a unique remote instance, and spawns a new controller and PTY
for Claude, Codex, or OpenCode. The new receiver tab is inserted in the
background without selecting it or changing view, visibility, or focus. No
receiver path types into, submits through, or otherwise injects the main panel.

The active tick renews only its exact claim. A second arrival stays durable and
unclaimed until the active run closes, then the next tick applies the same FIFO
order. Completion requires the exact artifact and exact locked remote session;
neither process spawn nor screen activity is completion evidence. Terminal
cleanup releases that session owner, shuts down the controller once, closes only
the matching receiver tab, preserves the immutable provider reply context,
reloads tasks, and starts the sync push without changing the active view or
focus. Spawn failure and child exit without valid completion perform explicit
registration cleanup and durable pre-acceptance retry. Lost claim ownership
permits local tab cleanup only. Progressed stale states are not rerun before
BR-16 defines their recovery policy.
The shared HTTP listener uses four
blocking workers, a 1 MiB body limit, constant-time HMAC verification, and a
bounded provider-ID coordinator keyed by workspace, channel, and provider ID.
It excludes simultaneous duplicates while the first request is in flight but
does not remember successful durable SMS or Email acceptance; every later
retry rechecks the workspace DB. A signature-verified unavailable Resend ID is
retained as a permanent discard in a separate 1024-key set. If that exact ID is
already in flight, Brain defers the discard, preserves the reservation, and
returns 503 until the pending acceptance resolves. Known unavailable ingress is
resolved before selecting that exact workspace's signing credential, and no
root, user, prompt, or job socket is opened for this verification. A later live
replay is rejected before Receiving API access. Persisted disable uses this
same exact-workspace path even when live refresh is blocked or fails. Dispatch retains the original
route ticket, reserves five seconds for
the response, derives one handoff deadline capped at two seconds and at that
response cutoff, then revalidates the exact generation, authority incarnation,
enabled state, and live lease immediately before durable admission. SQLite
configuration and reconciliation use that remaining duration. Dispatch then
refreshes and rebinds the remainder before acceptance lock waiting, and the
deadline is rechecked after commit; continuous progress cannot renew it.
Resend's received-email and attachment-metadata calls each have a ten-second
maximum and a 1 MiB response cap; an oversized stream is stopped after one
proof byte beyond the cap, before JSON parsing. Verified unavailable, ignored,
and permanent discarded Resend events return provider success and remain
outside the queue. The one exception is an exact in-flight unavailable
duplicate, which receives 503 until pending acceptance resolves; invalid
signatures remain authentication failures, and
internal 500/502 outcomes remain failures rather than provider success.
Delayed email dispatch refreshes signed attachment access from stable provider IDs, and
processing plus final replies preserve the original subject and message
lineage without widening recipients. Receiving-API rejection or malformed
provider JSON returns 502.
Provider
credentials, message bodies, and signed media URLs are passed to `curl` through
standard input rather than process arguments. Provider output is captured so it
cannot corrupt the TUI. Outbound Twilio/Resend calls are serialized through a
bounded background delivery worker, preserving reply order without blocking
keyboard input or shell shutdown.

An inbound message whose entire body is `/new` or `/restart` (case- and
whitespace-insensitive) is a control command, read in
`server/receiver/control.rs` and applied in
`tui/app_brain/receiver/control.rs` rather than sent to the agent. `/restart`
is applied from durable queued state ahead of every new dispatch gate. It
atomically drops only older unclaimed backlog, keeps an active run and later
arrivals, and rolls only the command's exact logical conversation. `/new` waits
its FIFO turn, then atomically retires its exact conversation, moves later
unclaimed work onto a fresh empty conversation, and finishes without launching
an agent. Neither command enters a PTY or the main panel.
Both acknowledge their own sender through `reply_to_job`,
which addresses the job's own recipients rather than whatever reply state is
live. A dropped job is not the message currently in flight.
The idle claim transaction repeats the exact restart check while holding its
SQLite immediate writer reservation. A restart committed before that boundary
blocks the ordinary claim; a restart committed afterward cannot retroactively
replace the already active owner. If intent is disabled while a claimed `/new`
waits on freshness, the command still completes and acknowledges its sender,
but following work stays queued until re-enable.

Every outbound email carries both parts of the Resend payload: `text` is the
agent's markdown verbatim, and `html` is that markdown rendered by
`server/reply/html.rs` (`pulldown-cmark`) inside brain's styled card. Raw HTML
in the answer is escaped to visible text rather than passed through, and link or
image destinations outside `https:`/`http:`/`mailto:` are dropped — a reply
quotes inbound message text, so both are attacker-influenced. Element styling
rides in a `<style>` block; a client that drops it still renders a correct,
structured document.

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
  `.config/workspace.json`, setup claims `.config/workspace-claims/**`, Python
  bytecode (`__pycache__/**`, `*.pyc`), task
  schema metadata `tasks/SCHEMA.json`, friendly conflict copies `*(conflict *)*`, raw
  markers `*.__brainconflict__*`, and every in-root transaction artifact:
  journals plus their staged/backup/restore scratch (`.brain-*`, and the
  `.<live-name>.brain-triage-…` siblings) and transaction locks
  (`*.transaction.lock`)) plus any
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
  Because that gate *reads* the local manifest, a machine that has never had the
  workspace obtains one first: `src/sync/identity/adopt.rs` reads the remote
  manifest, refuses any whose UUID differs from the registry record, and writes
  it locally so `receiver_ingress_id` matches its peers. This is the only lane
  that copies the manifest remote → local; every other identity write publishes
  local → remote, and bisync excludes the manifest entirely, so minting locally
  on a joining machine would fork portable identity irreparably. All three
  identity lanes share one remote-read rule in `src/sync/identity/read.rs`: a
  successful read with no bytes means absent, because `rclone cat` of a missing
  object exits 0 on B2.
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
  synced around on a later run — which also means the remote's copy of the
  marker is invisible to every later sync, and only `brain sync resolve` can
  remove it. Because the rename leaves zero leftover markers,
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
  first. Resolving also clears the **remote** losers for that original
  (`src/sync/command/resolve_remote.rs`): rclone's marker lands on both sides
  but only the local root is renamed, and both conflict patterns are bisync
  excludes, so nothing else can ever collect that remote object.
  `conflicts::remote_losers_for_original` matches it by *either* naming form
  (raw `<original>.<MARKER><N>` or friendly), the lane lists just the
  original's own remote directory (`rclone lsf --files-only`) and removes each
  loser with `rclone deletefile` — one object at a time, never `delete`, which
  would take a directory and recurse. `resolve` therefore does invoke `rclone`,
  but never bisync and never the journal: it is still deletion only, so the
  skill runs one ordinary `brain sync` afterward to push the merged canonical
  out. A missing remote config or absent `rclone` degrades to the old
  local-only behavior; an unreachable remote is reported as such rather than
  as a clean one, so a silent listing failure can't read as "nothing there."
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
  [data-model.md](data-model.md) for the rules. Habit rows also dedup by
  `(task_name, due_date)` right after the row union, so a habit occurrence
  spawned independently on two machines before they sync collapses to one
  instead of leaving a duplicate row behind; see
  [data-model.md](data-model.md) for the fold rules. Distinct UUIDs that claim one
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

## Auto-sync triggers (startup, periodic, change, and receiver)

The auto-sync layer (`src/sync/{lock,watch,periodic,trigger,freshness,current,follow}.rs`,
wired into `src/tui/runtime/builder.rs`, `src/tui/runtime/mod.rs`, and
`src/tui/app_sync.rs`) drives the
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
  entry point the startup, periodic, watcher, receiver-freshness, and receiver
  completion triggers use. It
  spawns the current exe as
  `brain --workspace <canonical-name> sync [--pull|--push] --if-idle` fully
  detached, with `BRAIN_WORKSPACE_ID=<selected UUID>` as a defense-in-depth
  expectation. Bootstrap compares that environment value to the registry UUID
  selected by `--workspace` and refuses the command on malformed or mismatched
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
  excludes above): a changed path under `.git`, `.cache`, `node_modules`, or
  `__pycache__`, a `.pyc` file, a `.DS_Store`, an
  existing friendly conflict copy (`*(conflict *)*`), or a transaction
  journal/scratch/lock (`.brain-*`, `*.brain-triage-*`, `*.transaction.lock`)
  never triggers a sync.
  So a VCS write, a cache churn, a conflict copy fanning in from another
  machine, or a `brain user` edit mid-transaction can't kick the watcher;
  mid-transaction is precisely when a push would carry a half-applied group. The watcher runs `notify` recursively over the
  brain root; when relevant changes settle for the `debounce_ms` window it
  spawns a detached `brain sync --push`. Push uses `rclone copy --update`, so
  it cannot download files or delete remote-only paths. Task CSV and counter
  merges preserve remote rows/maximum values in the upload without writing
  those values locally. This removes the prior sync-write feedback loop. The
  debounce loop accepts an injected clock in tests. Its handle owns an explicit
  stop signal and worker join, so dropping one TUI stops only that workspace's
  watcher while peer workspace watchers remain live.
- **The periodic puller** (`sync/periodic.rs`) owns a five-minute
  `recv_timeout` loop for each sync-configured live shell. Every timeout starts
  a detached pull. Dropping its handle signals and joins the worker immediately;
  the workspace lock coalesces ticks from peer shells or an active sync.
- **The receiver freshness gate** (`sync/freshness.rs`,
  `tui/receiver/runtime/sync.rs`, and `tui/app_sync.rs`) reads the newest
  successful downstream journal row at the
  live TUI's queued-job consumption boundary, not in shared-server dispatch.
  Before SMS/email dispatch, a missing row or age over two hours starts
  `brain sync --pull` and holds the message queue until a newer journal row
  appears. The footer polls `current.json` every 250ms and displays the active
  direction. `ReceiverRuntime` owns the gate attempt and deadline state; its
  pure transition consumes caller-supplied clock, journal, and running-process
  observations. The tick exposes that transition as the sync-freshness effect.
  The `AppServices`-owned injected sync adapter performs those reads and the
  detached child launch outside the receiver module, then App returns readiness
  through the runtime's semantic gate operations. The production policy gives a
  launched pull five seconds to appear and permits at most three attempts; if
  none starts, the TUI warns and processes the job with local state. The same
  status poll watches for successful downstream journal advancement and reloads
  task state automatically. A verified receiver completion starts a detached
  push before provider delivery. There is no exit sync.

## The auto-rebuild

`run.sh` rebuilds `target/release/brain` whenever `Cargo.toml` or any
`src/**/*.rs` is newer than the binary, then `exec`s it. This is the only
reason an agent's source edit "takes effect" without a manual
`cargo build` — but you should still build/test explicitly while
developing (see [testing.md](testing.md)).

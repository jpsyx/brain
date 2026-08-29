# Features

`brain` is the central terminal dispatch for the second brain and task
system. Everything below is reachable from the persistent shell (bare
`brain`), as a subcommand, or from a command palette.

## The merged shell: three main views + one brain panel

Bare `brain` (and `brain tasks …`) opens a persistent shell with **three main
views** and one app-level **brain panel** (see [glossary.md](glossary.md)):

- **Tasks view:** the startup default. The task-management surface over the
  selected workspace's `tasks/{tasks,habits}.csv`: tabbed sub-views (`today`, `mit`,
  `past_due`, `week`, `habits`, `backlog`, `all`; `Tab`/`Shift+Tab` cycle
  them), vim navigation, notes expand/render, mark-complete / remove / defer /
  open-links, agenda (`Ctrl+A`), and the daily-triage startup nudge. The full
  key list is in [keybindings.md](keybindings.md).
- **Brain-directory (search) view** — the fuzzy search across projects,
  areas, resources, and archive (the picker described later in this doc);
  formerly what bare `brain` opened.
- **Logs view:** a scrollable view of the current run log, opened from the
  palette or the main-view cycle.
- **Brain panel** — a live, interactive agent session in an embedded PTY,
  running this machine's `default_agent_frontend` (Claude unless set), or the
  frontend named by `--claude` / `-cl`, `--codex` / `-cx`, or `--open-code` /
  `-oc` for one run, open at startup and shared by all
  main views. It does not belong to a view: switching views leaves it open;
  closing it (`Ctrl+X`, or the agent
  exiting) makes the active main view full-width.

Switch main views with `Ctrl+L`/`Ctrl+H` (cycle) or `Ctrl+T` (tasks) /
`Ctrl+B` (brain directory). `Alt+S` opens the keyboard-shortcuts help modal
from either view (the compact footer's `Alt+S  all shortcuts` hint points at
it). Startup focuses the tasks view with the brain panel open but unfocused.

The rest of this document describes the brain-directory view's picker + brain
panel in detail; the tasks view's behavior mirrors the pre-merge `tasks`
shell.

## The brain panel and the search view

`Alt+H` focuses the left panel and `Alt+L` the right (vim-style, spatial);
the focused panel's border brightens and the unfocused one dims. The shell
starts focused on the tasks view so task navigation works immediately; the
brain panel is still spawned at startup with the selected frontend ready
one `Alt+`-switch away. It resumes your most recent conversation in that
workspace, skipping any the frontend can't reopen — one that never got a real
exchange, one another live agent is still in, and any belonging to a background
agent you started from the panel. If a resume is refused anyway the agent quits
at once, so brain opens a fresh session in its place and tells you *"couldn't
resume your last conversation; started a new brain chat"*: you always get a live
panel, never a dead one. `Alt+U` / `Alt+D` scroll the focused panel a half-page
up / down (the brain panel by half its visible rows, the search panel by a page
of its match list): a keyboard-only alternative to the wheel that fires even
while the selected agent has focus or the filter is being typed. macOS
Option-produced equivalents are accepted too, so richer keyboard reporting in
embedded frontends does not strand the scroll binding.

Every live main or ephemeral-tab session sits behind an `AgentController`.
Keyboard, render, scroll, completion, receiver-run, and close paths call semantic
operations on that facade; only the Claude, Codex, and OpenCode adapters know their
commands, input sequences, session rules, and hooks. Whole-shell teardown
explicitly shuts down all controllers before their transports are dropped.

**Closing vs quitting.** Exiting the agent (for Claude, `Ctrl-C` to end the
turn, then `Ctrl-C` again to exit) **closes the brain panel** — the main view goes
full-width and the shell keeps running. It does *not* quit `brain`. To
**re-open** the panel, run **Message brain** (`Ctrl-M`, or the palette row —
which only appears while the panel is closed); it resumes your latest
session. To **quit `brain`** entirely, press `Esc` or `Ctrl-c` from the
**search** panel.

**Start a new session.** `Ctrl+N` starts a fresh agent conversation in the
brain panel: it sends `/new` to the running frontend for you. It fires
from either panel while the panel is open (no need to focus the brain panel
first). When the panel is closed, `Ctrl+N` keeps its search meaning (move the
selection down).

**Skill sessions: one tab per single-prompt run.** A **skill session** is a
dedicated, ephemeral agent session for *one* prompt — typically a slash command
for a single skill, hence the name, though the prompt can be anything. It runs in
its **own brain-panel tab** so a long run doesn't tie up your main session, and it
**closes itself** when the run finishes.

Daily triage is the **builtin** one. Saying **Yes** to the startup "Today's
triage isn't done. Run it now?" nudge no longer types `/triage` into your main
session and blocks it for the whole pass; the panel grows a **Daily triage** tab
holding a separate session seeded with `/triage`, and the pass runs there while
tab 1 stays free. The builtin is offered only while the workspace's daily-triage
check is enabled (`brain config set enable_daily_triage_check=…`, or the
palette's *Disable/Enable daily triage alert* row), and it is not editable or
removable — unlike the ones you declare yourself.

**Declare your own.** Any prompt you run often and want out of your main session
goes in the machine-local `skill_sessions` env array:

```sh
brain env set skill_sessions '[
  {"title": "Email triage", "prompt": "/email-triage", "command_label": "Run email triage"}
]'
```

Or, with no value, `brain env set skill_sessions` walks you through
add / edit / delete. Each entry contributes:

- **`prompt`** — what the session is seeded with (the only required field);
- **`title`** — its brain-panel tab title (defaults to the prompt);
- **`command_label`** — its **command-palette row**, verbatim (defaults to
  `Run <title>`).

Pick that row in the palette (`Ctrl+P`) and the session starts in a new tab.
**While it runs, its row disappears**, so the same session can't be started
twice. Several *different* skill sessions can run at once, each in its own tab.

**Switching tabs.** Cycle with **`Alt+[`** / **`Alt+]`** (previous / next) from
either panel; the panel shows a `1 Brain` · `2 Daily triage` · `3 Email triage` …
strip while any ephemeral tab is live, in the order tabs were opened. The **command
palette** also carries **Show main brain session** and one **Show <title>
session** row per open skill tab (the works-anywhere alternative). (`Alt+1` selects the
main session and `Alt+<n>` the nth ephemeral tab directly too, but terminal
`Alt+digit` handling is unreliable, so the bracket cycle and palette rows are the
dependable paths.)

The same strip and slot order can hold a distinct background receiver-run tab.
Receiver insertion never selects that tab, reveals a hidden panel, changes the
main view, or moves keyboard focus. When receiver intent is enabled, the single
event-loop consumer preserves the sync-freshness gate, claims the oldest ready
durable job, and launches a new PTY and frontend-neutral `AgentController` in
that background tab even while the main panel is busy. Later arrivals remain
durable until the active run reaches a valid exact completion. Answer commit
releases agent ownership at `answer-ready`; cleanup shuts down only that
controller and waits for the transport to confirm that exact child exited.
Exact session release and private artifact removal stay fenced until that
confirmation is durably acknowledged. If the origin dies, authoritative
startup lock reaping atomically records the same handoff before it unlocks the
exact session; PID equality or reuse is never takeover proof. Those two cleanup
effects retry independently, then task reload
and sync finish the handoff while leaving the active view, tab, and focus
unchanged. A bounded cleanup-only registry detaches completed receiver tabs,
retains their exact controllers, and gives each pending shutdown one fair retry
per pass. The ordinary receiver lane can launch and complete later FIFO jobs
while those exits remain unconfirmed; only a full eight-controller registry
holds the next completed controller in its exact tab. Cleanup retries do not
block a later job. A fresh App applies the same fairness to durable cleanup
rows: an incomplete oldest row moves behind its eligible peers before another
pass, so its persistent session, artifact, reload, or sync failure cannot
starve a later answer's exact cleanup. Timestamp saturation shifts only the
workspace's peer ordering timestamps in the same transaction, so the failed row
still moves last at `i64::MAX`. Only a proved
  synchronous spawn failure enters bounded retry. A later child exit without
  exact completion preserves the fenced post-spawn job for BR-16.

Every skill session is **ephemeral**: never recorded in the session DB, never
resumed. Because a run can involve back-and-forth with you, "the agent stopped
talking" is not a reliable done signal — instead brain appends a short completion
protocol to the prompt it sends, and the run POSTs a one-time token to the local
brain server once it truly finishes. brain then **auto-closes that tab**,
dropping you back to tab 1 with a `✓ <title> complete` flash. The run may also
declare output files that must exist first, and brain holds the tab open until
they do. Closing a tab yourself is `Ctrl+X` while on it (it ends only that
session). If you quit `brain` mid-run the session is simply lost — nothing to
resume, so start it again (and the daily-triage nudge fires again next launch).
See [integrations.md](integrations.md) for the completion-signal wiring.

**Saying Skip is deterministic — no agent.** The nudge's third button, **Skip**,
means "not today." Skipping is pure bookkeeping (mark today's Morning Triage
occurrence done and spawn tomorrow's — nothing to decide), so brain does it
**in-process** the instant you press it: no brain panel opens, no prompt is
typed, no agent runs. It is the exact mutation of
`brain habits complete-managed-triage daily` (see below), and it **respects
`enable_triage_habits`**: with the feature off, Skip just dismisses the nudge
and touches nothing. The agenda refreshes in place and a `✓ daily triage
skipped` flash confirms it. (Contrast **Yes**, which is agent-driven because a
real pass involves judgement.)

**Session resume.** On startup Claude resumes the most recent candidate whose
workspace transcript exists. OpenCode asks the configured command for
`session list --format json` in the selected root and resumes only a live,
non-archived, non-deleted root session whose reported directory is that exact
root. If a stale DB row no longer has matching frontend evidence, Brain skips
it and starts a fresh chat with a status-line explanation. Codex resumes when it
still holds the session's rollout on disk, and starts fresh when it does not. If you type `/new` (or `/clear`) inside an agent, or press
`Ctrl+N`, the generic lifecycle bridge records the new root-session ID when
the frontend emits its start event. Brain permits one live TUI per workspace UUID:
a second TUI for the same UUID receives a clear
already-running message, while TUIs for different workspace UUIDs may run at
the same time. If the
candidate is stale, Brain starts a fresh chat and says so in the status line.
See [integrations.md](integrations.md) and
[data-model.md](data-model.md) for the lock-and-recency model.

Claude is selected per run with `--claude` / `-cl`; Codex with `--codex` /
`-cx`; OpenCode with `--open-code` / `-oc`. The selectors may appear before or
after `tasks` and its delegated positionals, stop at `--`, and reject mixed
frontend selection. With no selector, this machine's `default_agent_frontend`
env value decides (`claude` when unset), so `brain env set
default_agent_frontend=codex` makes Codex the default here without changing what
any other machine on the workspace launches.
The adapters use `codex_cmd`, `opencode_cmd`, and `claude_cmd` from brain env.
Codex
participates in the same frontend/workspace/actor/channel session store but
resumes when the exact session rollout remains on disk and starts fresh when
that rollout is missing. Every
ordinary Brain command refreshes the registry-declared lifecycle artifacts in
all existing configured workspaces before bootstrap, so remote prompts and
completion delivery use the same current protocol. When brain
injects a prompt into an already-open Codex panel, it sends `Tab` as the final
native busy-turn queue key. Claude and OpenCode receive `Enter`. Text and the
adapter-defined final key are one semantic facade operation, so callers never
construct frontend keystrokes.

Workspace-only launches also resolve portable logical MCP and skill allowlists
against only the selected workspace's machine record. Claude receives selected
MCPs through a cache-local runtime config while preserving the shared login.
Brain reports this selection as strict only when `claude_cmd` is a safely parsed
direct Claude invocation with no conflicting Brain-owned flags; indirect or
shell-ambiguous commands are reported as advisory. Codex receives documented
per-call config overrides, but its inherited
global MCP and skill sources cannot currently be proven excluded. OpenCode
receives Brain-owned `agent.brain`, `default_agent`, selected `brain_ws_*` MCP
entries, and a selected skill path through merged inline configuration, but
inherited global sources also cannot be proven excluded. Selected skill names
are trusted guidance for all three frontends. `brain skills status`
labels each requested capability as `strictly-selected`, `advisory-only`, or
`unavailable`, rather than claiming isolation the frontend does not provide.
Unrestricted launches skip capability parsing and remove stale workspace-only
artifacts before using the frontend's ordinary global configuration. TUI
startup therefore ignores malformed `allowed_mcps` or `allowed_skills` values
only in unrestricted mode; it still validates `access_mode` and all live TUI
settings, while workspace-only startup validates the capability lists too.

**Swap the layout.** The palette's "Move brain panel to the left/right"
command flips which side the brain panel sits on; the choice is persisted
(`<workspace-cache>/state.db`), so it sticks across runs. `Alt+H`/`Alt+L`
follow the new layout (always left/right).

**Opening files never closes the shell.** Selecting a file in search opens
it without tearing the shell down (see "The fuzzy picker" below): text files
open in a **new iTerm2 tab**, everything else hands off to the system
`open`. You stay in brain.

## The command palette (`Ctrl-p`)

The full list of things `brain` can do lives in the **command palette**,
opened with `Ctrl-p`. It opens as a **modal overlay** on top of the current
search, so pressing `Esc` (or `Ctrl-c`) just closes it and drops you back
where you were — it does **not** exit `brain`. Its rows, in order (rows with
a direct keystroke show it dimmed in `[…]`):

- **Create PDF for '<file>'** `[^G]` — convert the highlighted markdown file
  to a colocated same-name PDF and open it. **Shown only when a `.md` file is
  highlighted**, where it leads the list (default-selected) so the palette
  opens ready to run it; a long filename is elided in the label
  (`Create PDF for 'really-long-na...md'`). See "Create a PDF from markdown"
  below.
- **Open file '<file>'** `[↵]` — open the highlighted file (text → a new
  iTerm2 tab / `$EDITOR`; blob → system `open`), exactly what plain `Enter`
  does in the picker. **Shown only when a file is highlighted** (a directory
  has no file to open); a long filename is elided head+tail like the PDF row.
- **Open dir '<dir>'** `[^↵]` — reveal the highlighted entry's directory
  in Finder (a file → its parent dir, a directory → itself), exactly what
  `Ctrl-Enter` does. **Shown whenever an entry is highlighted.** The label
  never shows the absolute path or the filename: it leads with the bucket
  category (`projects/`, `areas/`, `resources/`, `archive/`) and, when too
  long, elides the *middle* keeping the tail (`resources/.../final/parts`).
1. **Message brain** `[^M]` — open the brain panel (resume your latest
   session), or focus it if already open. Shown **only while the panel is
   closed**.
2. **Open tasks** `[^T]` — switch to the tasks main view (task management,
   agenda, triage), in-process.
3. **Search projects:** rescope search to the selected workspace's `projects/`.
4. **Search areas:** rescope search to the selected workspace's `areas/`.
5. **Search resources:** rescope search to the selected workspace's `resources/`.
6. **Search archive:** rescope search to the selected workspace's `archive/` (retired material).
7. **Global search** — search across projects, areas, resources, and archive.
8. **Enable receiver / Disable receiver** toggles persistent intent for the
   selected workspace without starting or stopping the shared process.
9. **Move brain panel to the left / right**: swap the layout (label names
   the direction the panel would move).
- **Delete '<file>'** `[^D]` — move the highlighted entry (file **or**
  directory) to the Trash. **Shown whenever something is highlighted**, and it
  **trails** the list (never default-selected) so a stray `Enter` on open
  can't delete; the label is elided with the same threshold as the PDF row.
  See "Delete an entry" below.

The search rows rescope the left panel **in place**; "Open tasks" switches
to the tasks main view in-process (the same as `Ctrl-T`). The keystrokes
(`Ctrl-g`, `Ctrl-d`, `Ctrl-m`, `Ctrl-t`) also fire directly without opening
the palette first — `Ctrl-g` and `Ctrl-d` open a confirmation modal (see
below); the rest run their action. `Ctrl-R` **refreshes** the search list
(re-walks the current scope, keeping the query); the list also auto-refreshes
after a PDF is created or an entry is deleted, so the change shows without a
manual refresh.

The palette is a filterable text input: typing narrows the rows. Each
row's matchable text includes its 1-based number, so you can type a digit
(`6`), any word from the label (`message`), or several words (`search
projects`) and the list narrows to the hits. Navigate the filtered list
with ↑/↓, `Ctrl-k`/`Ctrl-j`, or `Ctrl-p`/`Ctrl-n`;
`Backspace`/`Ctrl-u`/`Ctrl-w` edit the query; `Enter` runs the highlighted
row (and exits the picker into that action); `Esc` or `Ctrl-c` closes the
overlay and returns to the underlying search (no cd, no claude, no error).

## Subcommands

Bare `brain` (no subcommand) opens the shell on the tasks view. Short-lived
management and reporting commands stay outside the persistent shell.

| Command | Behavior |
| --- | --- |
| `brain` | Open the persistent shell on the tasks view (the startup default) with the brain panel on this machine's `default_agent_frontend` (Claude unless set). |
| `brain --claude` / `brain -cl` | Open the same shell with Claude in the brain panel, whatever this machine's `default_agent_frontend` says. |
| `brain --codex` / `brain -cx` | Open the same shell with Codex in the brain panel. |
| `brain --open-code` / `brain -oc` | Select the OpenCode brain-panel adapter. Brain launches OpenCode in the selected workspace, passes the initial prompt separately, tracks the OpenCode session ID, and delivers completion through the shared controller lifecycle. Selecting two frontends exits with `🔴 Choose one agent frontend: --claude, --codex, or --open-code.` |
| `brain env set default_agent_frontend=<claude\|codex\|opencode>` | Choose which frontend the brain panel launches on **this machine** when no selector flag is passed. Machine-local, so each machine on a workspace can differ. |
| `brain --workspace <workspace>` / `brain -w <workspace>` | Select a workspace by canonical name or alias before an ordinary command runs. Omitting it selects the machine default. The option may appear before or after a subcommand or delegated task positional. `--workspace=<workspace>` is equivalent; `--` ends option extraction. |
| `brain tasks [view/date/query] [flags]` | Open the shell on the given tasks view/selector/search. `--claude` / `-cl`, `--codex` / `-cx`, or `--open-code` / `-oc` may be passed before or after `tasks` and its delegated positionals. `--` stops selector extraction. |
| `brain tasks --no-tui …` | Print the resolved task list as plain text (no TUI). |
| `brain tasks complete <id>` | Mark a task or habit complete natively, no TUI. Also re-syncs the day's agenda markdown the completion just invalidated (see [Keeping the day's agenda in sync](#keeping-the-days-agenda-in-sync)). |
| `brain tasks sync-agenda [<id>] [--action done\|defer\|touch] [--date YYYY-MM-DD]` | Re-sync the day's agenda after any task/habit mutation, without mutating anything itself. This is the one implementation of that sync; native completion runs the same code in-process and the bundled `/todo` mutator scripts shell out to it. Omitting the id refreshes only the CSV-derived snapshot sections; `--action` defaults to `touch`, which never edits the plan. |
| `brain tasks add --name <name> --type <type> --priority <p0..p4> [OPTIONS]` | Create a task or habit through native Brain logic, preserving assignment, project/Linear metadata, chunking, validation, and CSV behavior. `--habit` (with `--interval`/`--unit`) creates a recurring habit and accepts `--ideal-time "6:45 AM"` to place it in the habits views' Morning/Afternoon/Evening grouping; `--ideal-time` is rejected for a plain task. Use `--json` for automation; plain output prints each created ID. |
| `brain tasks set <id> [--name\|--due\|--priority\|--status\|--notes\|--project\|--linear-issue\|--duration\|--ideal-time]` | Edit fields on one existing task or habit by absolute value (aliases: `edit`, `update`). Accepts `t123`/`H43`/a unique fuzzy name; `--due` takes `YYYY-MM-DD`, `today`, `tomorrow`, or empty to clear. Deliberately never touches `defer_count` — this is the surface an external tracker mirrors onto, and someone else's reschedule is not the user's slip. A habit row requires `--habit`. Reports each `before → after`; `--json` for automation, and a no-op edit writes nothing. Omitting every field drops a human into an interactive field picker. |
| `brain tasks all --no-tui --linear-issue <ID>` | Find the local task mirroring an issue-tracker identifier (case-insensitive exact match on `linear_issue`). `--linear-issue` is a global filter, so it composes with any view and with `--include-done` / `--include-deferred` to reach closed or parked mirrors. |
| `brain tasks remove <id> [--habit]` | Delete one task (aliases: `rm`, `drop`). Removing a **habit** destroys its whole recurring chain, so it needs the explicit `--habit` opt-in — which is what keeps a task-cleanup pass structurally unable to reach one. Managed triage rows are refused outright. |
| `brain tasks defer <id> <+Nd\|YYYY-MM-DD> [--no-count]` | Push a task out, with the defer penalty: `defer_count` climbs, the `mit` tag is shed, and a `p0` drops to `p1`. A task that is `waiting` or `blocked_by` another defers for free, as does `--no-count`. Later chunks of a chunked task cascade forward only when they would otherwise land before it, and never inherit the penalty. |
| `brain tasks touch <id>` | Bump `last_touched` to today and change nothing else — the chronic-ignore "yes, I still care" acknowledgement. |
| `brain tasks assign <id> <user>` | Hand a task or habit to another portable workspace member. The user ID must name a real member. |
| `brain tasks lint [--fix]` | Check the task automation rules over both CSVs and the task ↔ project links; `--fix` applies what can be applied mechanically. What the check reports is exactly what the fix would do. Exits non-zero when a check finds issues. |
| `brain tasks chronic [--json\|--count]` | List chronically-ignored tasks: not done, not parked, actionable, deadline imminent or absent, and either 21 days untouched, 14 days stuck in progress, or old-thin-and-never-started. |
| `brain tasks stale-waiting [--threshold N] [--json\|--count]` | List tasks stuck in `waiting` longer than N days (7 by default). A row with no `waiting_since` is surfaced regardless — not knowing how long it has waited is itself the problem. |
| `brain tasks linked [--open-only] [--json\|--count]` | List tasks carrying an external issue-tracker link. Brain never contacts the tracker; this is the read a caller reconciles from. |
| `brain tasks streak <name> [status\|mark\|unmark] [--date]` | Count consecutive days on which a named thing happened, and record or forget one. Deliberately generic: brain stores the dates and counts the run; what the name means is the caller's. |
| `brain tasks agenda-pdf [--date]` | Render the day's agenda to `<agenda_dir>/agenda-<date>.pdf`, stripping HTML comments from a copy first. Unlike the mutation path's regen, this builds one whether or not a PDF already exists — asking for it *is* the request. |
| `brain tasks agenda-appendix --content <file> [--date]` | Bake caller-supplied markdown into the day's agenda as one generic appendix section. Re-running replaces it rather than duplicating; core never discovers the content. |
| `brain backlog` | Review the backlog, stalest first, with how long each has been parked. `--json` for automation. |
| `brain backlog park\|restore <id>` | Park a task indefinitely (schedule cleared, hidden from every active view) or bring one back to `not_started` without a due date. |
| `brain backlog purge [--dry-run\|--report]` | Delete tasks parked for more than six months, leaving a breadcrumb in any project they belonged to. Silent by design. |
| `brain backlog dedupe [--dry-run\|--report]` | Delete parked tasks an active task has already superseded — a near-identical twin created *after* the parking date. Silent by design. |
| `brain habits defer <id> [--occurrences N]` | Skip a habit's next occurrence by advancing its due date one recurrence interval (or N), using the same catch-up maths completion uses. Nothing is marked done. |
| `brain habits cleanup` | Sweep completed habit occurrences older than a week. Managed triage rows are never swept: removing one is a transactional decision. |
| `brain triage state [--mark]` | Report whether today's weekly triage is also this month's monthly one, or record that it has happened. "Monthly triage" is the first weekly pass of a calendar month, so this is one bit of state. |
| `brain contacts [add\|edit\|delete\|list\|find\|get\|fallback]` | The workspace's local contacts book. Bare `brain contacts` lists everyone; `--pretty` prints a table instead of JSON. Ids are `C###`, assigned in sequence and never reused. Resolution takes an id, then an exact name, then a name fragment, and **refuses** rather than guessing when more than one contact matches. |
| `brain project new <slug> --title <t> --priority <p0..p4> [--status] [--due] [--description]` | Scaffold a PARA project: the folder, its full `.METADATA.json`, and a README with the H1 and description — then rebuild `projects-lookup.csv`. Refuses to overwrite an existing project, and refuses a due date that isn't an absolute `YYYY-MM-DD` or `none`. Deciding the namespace, outcome slug, and priority stays with the person; writing the record exactly does not. |
| `brain project set <slug> [--title\|--status\|--priority\|--due]` | Change a project's state. Only the fields you pass move, each is validated, unknown fields in the file are carried through untouched, and the lookup is rebuilt — so there is no way to leave it stale. Reports only what actually changed. |
| `brain project archive <slug>` | Move a project to `archive/projects/<slug>`, keeping its folder name, repointing `.METADATA.json:directory`, and rebuilding the lookup. |
| `brain project show <slug> [--json]` | Describe a project, plus how many of its tasks are open and how many the chronic-ignore sweep flags. When *every* open task has been ignored it says so outright (`died_quietly`): a project whose tasks all went quiet stopped rather than finished, and archiving it should be a decision rather than a way of papering over rot. |
| `brain clean [--dry-run]` | Remove tool byproducts (Finder metadata, Python caches, editor scratch) from the workspace root. Idempotent; the pattern list is conservative and closed. |
| `brain tasks doctor` | Run the state/hook health check, no TUI. |
| `brain tasks search <q>` | Open the shell with an initial search over all tasks. |
| `brain config [list\|get\|set]` | Read or change persistent, portable config (see below). |
| `brain env [list\|get\|set]` | Read or change your machine-local brain env. Bare `brain env` (and `env list`) breaks the whole machine down: machine-global values plus one block per registered workspace. `get`/`set` act on the selected workspace only; use `brain env set name=value` for direct or dotted updates, or omit the assignment to choose a variable interactively. |
| *(any ordinary command)* | Before running, Brain makes the selected workspace usable: it **creates the root directory** when this machine has never had it (`env.json` rides between machines, so registering a workspace on one registers it on all), writes the portable manifest from the UUID the registry already holds, pulls from the configured sync, and seeds PARA + the task/habit CSVs + ID counters when there is nothing to pull. See [Workspace setup on first use](#workspace-setup-on-first-use). |
| `brain workspace list` | List every attached workspace in canonical-name order, including default, root, aliases, local-user readiness, receiver state, and portable access mode when present, **followed by each workspace's required/optional feature health**. Add `-w <name>` to report health for that one workspace only. A workspace that still needs setup renders a one-line note naming its repair command instead of failing the listing. |
| `brain workspace {create\|attach\|rename\|alias add\|alias remove\|default\|remove\|repair\|migrate}` | Manage the schema-v2 registry, portable manifest, and coordinated legacy rollout. Omitted human values prompt on `/dev/tty`; every value also has a noninteractive flag or positional form. |
| `brain sync [--push\|--pull] {setup [--adopt-workspace-id <UUID>]\|repair\|status\|conflicts\|resolve}` | Manually sync the selected workspace root to its private Backblaze B2 target via `rclone bisync` (see below). Opt-in per workspace: does nothing until `brain sync setup` configures that record. Setup's dedicated UUID flag is the noninteractive authority for adopting a nonempty manifestless target. `conflicts` takes `--json` for structured output; `resolve <original>...` deletes resolved conflict copies, locally and on the remote. |
| `brain check` | Read-only report of pending sync changes (what a `brain sync` would push/pull), via dry-run `rclone bisync` plus task/habit CSV baseline diffs (see below). |
| `brain reindex [--projects\|--resources\|--tasks]` | Rebuild the derived lookup CSVs (`projects-lookup.csv`, `zotero-lookup.csv`) from the canonical `.METADATA.json` + `notes.md`, and re-apply the task/habit automation rules. Bare `brain reindex` does all three; the flags narrow it. This is the `/second-brain reindex` and `/todo reindex` operation (see below). |
| `brain persona [show\|list\|get\|set\|edit]` | Read or change one workspace member's persona (identity + tag styles), keyed by portable user ID. Bare `brain persona` runs onboarding when the person at this machine has nothing set, else shows their current values (see below). `brain personalize` is a hidden alias. |
| `brain skills sync [--root <dir>]` | Render + install bundled skills into the selected brain root's `.agents/skills`, then link them into that root's `.claude/skills`, `.codex/skills`, and `.opencode/skills`, and prune skills brain rendered before but no longer produces. `--root` selects a sandbox workspace (see below). |
| `brain skills status` | Show each selected workspace capability's requested state, machine availability, and separate Claude/Codex/OpenCode enforcement level without printing connection material or credentials. |

After a Brain version update, the first ordinary invocation migrates the core
skills into every registered workspace, even when legacy global skill copies
still exist. Old global files are retained; TUI startup syncs the selected root
before opening the brain panel.
| `brain server {status\|logs}` | Inspect the shared process without starting, stopping, or repairing it (see below). |
| `brain killall` | Stop every running Brain shared server and TUI process on this machine, including receiver-serving server processes. |
| `brain habits` | Open today's habits page. Always available: it reuses whatever is already serving (an open TUI's shared server, or an earlier background one) and elects a background server only when nothing is running. A workspace with no route yet registers a background lease of its own, so `brain habits -w family` works while a `brain` TUI is open. |
| `brain habits kill` | Stop a background habits server. It is rejected while any brain TUI is open. |
| `brain --with-receiver` | Persistently enable receiver ingress for the selected workspace before its TUI lease registers, then open the TUI. |
| `brain config set enable_daily_triage_check=false` | Open the TUI without ever showing the daily-triage startup nudge. Portable config, so every machine on the workspace agrees; the palette still toggles it per session. |
| `brain receiver {setup\|set\|start\|stop\|status\|url\|email\|phone\|logs}` | Configure receiver providers, persistently enable or disable the selected workspace, inspect intent and live availability, print the provider webhook URLs or configured addresses, or read shared-process logs. No receiver command starts or restarts a process. |
| `brain receiver` | Report this machine's one webhook URL per channel (with the public base URL it is built from), then every registered workspace's receiver details: intent, live TUI/server/accepting state, and the configured email and phone — the addresses that route a message to that workspace. `-w` narrows the workspace blocks to one; the URL block is machine-wide either way. Informational and read-only: an unconfigured value reads `not set` and an unreadable workspace names its repair command rather than failing the whole listing. Provider secrets are never printed. |
| `brain receiver {email\|phone}` | Print the bare address the selected workspace's receiver answers on (`resend_from_email` / `twilio_from_number`), on stdout with no styling, so a script or an agent can read it without parsing a status block. This address is also the **routing key**: it is what selects this workspace out of every workspace sharing the machine's one URL. `-w` picks another workspace. An unset address names the variable and both ways to set it, and exits non-zero. |
| `brain receiver url [--sms\|--email]` | Print the exact webhook URLs to paste into the Twilio/Resend portals. There is **one URL per channel for the whole machine** (`<public-url>/sms`, `<public-url>/email`); nothing in a URL names a workspace, so `-w` cannot change the answer and every workspace's portal gets the same pair. The output says so, alongside the paste-exactly rule. Informational: it reads this machine's machine-global `brain_receiver_public_url`, so it works before receiver ingress is ever enabled or running. Both channels by default. |

`brain tasks mark <id> [as] done` is rewritten to `brain tasks complete <id>`
before clap parses it.

Every run writes brain's diagnostic log to a timestamped file under `/tmp/`.
`--verbose` additionally mirrors that log to stdout and prints the log path at
exit. In the persistent TUI, logs still go to the file but never to stdout;
use the command palette's **Show receiver
server logs** or **Show brain logs** rows to switch the main panel to a
scrollable log view. While there, `q`, `Ctrl-C`, or the palette's **Return to
main view** action returns to tasks, and the palette hides unrelated commands.
The log includes command dispatch, normalized
arguments, task CSV paths and mutation results, sync/rclone phases, server
lifecycle decisions, doctor probes, and skill install counts.

### `brain config`

Reads and writes brain's persistent, **portable** config
(`<brain-root>/.config/config.json`) — the values that are right on every
machine (Linear workspace, triage settings, the calendar id, …). Rides whatever
syncs the brain directory.

- `brain config list` (or bare `brain config`) — aligned table of every
  variable, its effective value, and its description, followed by a muted note
  naming the variables another store answers.
- `brain config get <name>` — the effective value of one variable.
- `brain config set <name>=<value>` — set and persist a variable (unknown
  names rejected).

Three variables — `response_email`, `allowed_sms_senders`, and
`allowed_email_senders` — name facts the portable `users.json` roster owns.
`list` and `get` resolve them from that roster first and fall back to
`config.json` only when no portable user answers, so a receiver configured
through `brain receiver setup` reports the addresses it actually accepts
instead of `(unset)`. Only `inbound_allowed` identities count. `set` refuses
all three and names `brain user list`, `brain user add`, and
`brain receiver setup` instead, because writing them to `config.json` would
persist a value nothing enforces.

`config` runs before the `markdown-to-pdf` prerequisite gate, so it always
works even when that tool is missing. See [config.md](config.md) for the schema
and the prerequisite/auto-discovery rules.

### `brain env`

Reads and writes your **machine-local** brain env inside the selected workspace
record in the schema-v2 registry (`$XDG_CONFIG_HOME/brain/env.json`, falling
back to `~/.config/brain/env.json`). These are values that would be *wrong* if
copied to another machine: `markdown_to_pdf_path` (a machine-specific binary path, auto-discovered and
self-healing, and **machine-global**: stored once for the machine rather than per workspace), `claude_cmd`/`codex_cmd` (this machine's functional agent launch commands),
`opencode_cmd` (the machine-local OpenCode launch command),
`default_agent_frontend` (which of the three this machine opens by default), and the
Backblaze `sync` block (written by `brain sync setup`, below — see
[config.md](config.md) for its fields). Mirrors `brain
config` exactly, over the env store instead:

- `brain env list` (or bare `brain env`) — the **whole-machine env breakdown**,
  the counterpart to `brain workspace list`: the registry path, a **Global**
  block holding every top-level `env.json` key outside `workspaces`
  (`schema_version`, `default_workspace`, and the machine-global `env` values —
  lifted to their bare names, so `markdown_to_pdf_path` reads the way you type
  it), then **one block per registered workspace** (headed like `workspace list`, with `*` on the
  default and `(default)` / `(selected)` labels) listing every declared variable
  — `(unset)` included — plus that workspace's own nested dot-separated paths,
  and finally a **Variables** legend explaining each name once. Every row is
  resolved against its own workspace's root, so no block shows a peer's value;
  `(empty)` marks a value that is set to an empty string, and credentials show
  as `(set)` in every block. See [config.md](config.md) for the layout and the
  redaction rule.
- `brain env get <name>` — the effective value of one variable or nested path,
  such as `sync.b2_bucket`.
- `brain env set <name>=<value>` — set and persist a variable or nested path,
  preserving sibling values. Nested values can be addressed as
  `objName.key1.key2`. Structural fields such as `root`, UUID, aliases,
  local-user selection, receiver enablement, and access policy are rejected.

`env`, like `config`, runs before the `markdown-to-pdf` prerequisite gate.
The registry is never Backblaze-synced (it lives outside every workspace root
on purpose). A legacy flat `root`, the read-only `~/.config/brain-root`
pointer, and `~/brain` are one-time migration inputs; migration creates one
structural `WorkspaceRecord.root`. See
[config.md](config.md) for the full store/schema description and
[data-model.md](data-model.md) for the `sync` block's fields.

### `brain workspace`

Manages the schema-v2 registry at the fixed machine path without going through
the selected record. Canonical names and aliases are trimmed, ASCII
lower-cased selectors that must match `[a-z0-9][a-z0-9_-]*`;
`--workspace/-w` is global and may be placed around nested
subcommands or after a delegated task positional. The long equals form is also
accepted. A `--` option terminator leaves later selector-looking tokens in the
delegated task values.

- `workspace create [--name <name>] [--root <path>]` normalizes the root
  against explicit home/current-directory inputs, creates that requested root,
  writes a strict portable `.config/workspace.json`, and registers that UUID.
  A missing name derives from the root basename;
  the first record becomes default and later creates preserve the current
  default. The complete registry candidate is validated before root creation.
  `RegistryStore` serializes every writer before its load. If persistence
  or later directory creation fails, brain preserves the created manifest and
  never automatically deletes a created path because verification and deletion cannot be coupled atomically
  with the supported safe standard-library API. The structured error preserves
  the original failure as its source and lists only paths this invocation
  created, deepest first, for manual inspection and cleanup. An
  `AlreadyExists` path belongs to the competing actor and is preserved without
  being listed as invocation-created.
- When the selected workspace contains only Brain setup metadata (including
  `.brain/`, `.claude/`, `.codex/`, and `.opencode/` lifecycle artifacts) and
  empty PARA directories, the first tasks launch completes initialization before
  loading the task view. It creates the portable config, task and habit CSVs,
  task counters, lookup CSVs, and `projects/`, `areas/`, `resources/`,
  `archive/`, and `tasks/`. A configured workspace sync is completed before
  this check, and a successful initialization is pushed afterward. Any user
  file makes the workspace non-empty, so Brain leaves it untouched.
- `workspace attach [<root>]` requires a strict, compatible manifest, adopts
  its stable workspace UUID, and otherwise leaves the directory unchanged.
  Invalid or colliding identities do not mutate registry bytes or root contents.
- `workspace rename [<workspace>] [<name>]`, `workspace alias add/remove`, and
  `workspace default [<workspace>]` preserve the complete selected record and
  persist through the interprocess registry transaction and atomic-save
  boundaries. Adding an alias
  already present on the same record, including a case-folded equivalent, fails
  without changing registry bytes.
- `workspace alias add [<workspace>] [<alias>]` and
  `workspace alias remove [<workspace>] [<alias>]` are the exact nested alias
  forms shown by clap.
- `workspace remove [<workspace>]` detaches only a non-default registry record.
  It never deletes root, config, cache, sync, or remote data. Choose another
  default first when removing the current default.
- `workspace repair [--manifest] [--local-user-id <id>]` retains the legacy
  manifest and local-ID repair surface. New portable workspaces select an
  existing person with `brain user local <id>`.
- `workspace migrate` explicitly runs or resumes the legacy-to-multi-workspace
  rollout. It creates a UUID-scoped machine-local journal and retained portable
  backup, runs a final legacy semantic sync when configured, maps every
  unresolved sender and canonical `assigned_to` value before mutation, and
  activates task UUID merge identity. After the final sync it reloads config,
  portable users, and assignments before that mapping gate, so newly pulled
  senders or triage policy apply to this run. The schema transition publishes
  current task and habit CSVs, durably establishes their UUID baselines, and
  publishes `tasks/SCHEMA.json` last. It then rebuilds derived data and verifies
  every identity boundary before removing the journal.
  Each mapping question names the legacy phone, email, or assignment value in
  plain English and offers every existing portable person as a numbered row
  before the row that adds someone new; an answer is a row number or an exact
  member ID. Adopting an existing person for an assignment records a rewrite
  instead of inventing a second person for the same human: the cutover moves
  those `assigned_to` rows onto the chosen member inside the same journaled task
  transaction, and the retained backup keeps the pre-rewrite values. A legacy
  assignment value that is not valid lower-case kebab case can only be adopted,
  never kept as a new ID. Synced headless use requires explicit
  `--workspace <workspace>` selection plus
  `--acknowledge-all-machines-updated`; incomplete mapping prints exact
  `brain user ... -w <workspace>` remediation, offering both
  `brain user add` and `brain user reassign` for an unresolved assignment. A failed step reports the
  retained backup and the exact resume command. Every journaled failure is
  resume-only, including a failure before the remote-publication step is
  recorded, because a remote write may have succeeded before the local journal
  update. The retained backup is for forensic or coordinated manual recovery,
  never a one-machine restore while the rollout journal exists.
  Before current-schema publication, migration repairs duplicate `task_uuid`
  values left by older writers. The first row in deterministic tasks-then-
  habits order keeps its UUID; later rows receive workspace-scoped
  deterministic replacements. Resumed verification republishes repaired
  current CSVs and baselines while the migration lock is held, allowing the
  remote copy to converge without manual file edits.
- `workspace list` uses themed semantic tokens and becomes deterministic plain
  text under `NO_COLOR`. Valid portable modes include an honest three-line
  access/enforcement/sandbox status. It then appends the selected workspace's
  redacted requirements matrix. Required availability is distinct from
  optional `off`, `ready`, and `incomplete` feature state. The list path does
  not seed missing modes, repair setup, render skills, create locks, or inspect
  a peer workspace as a fallback. Malformed selected config is reported as
  incomplete rather than guessed as unrestricted. The **task schema** row
  reports whether the workspace declares `tasks/SCHEMA.json`; a workspace
  without it cannot sync, so it renders `incomplete` instead of `ready`.
- The brain panel's title names **the workspace**, not the product:
  `family · Claude`, or `family · Daily triage · Claude` on the triage tab, with
  ` exited` appended when the frontend has stopped. With more than one workspace
  open, the title was the one place that could tell them apart and said `Brain`
  for all of them.
- A workspace Brain creates is seeded with **`AGENTS.md`** (how an agent should
  behave in this workspace: PARA rules, the `second-brain` skill, media/note
  coupling, link repair on rename, which files Brain owns, and what not to put in
  a synced root) and **`README.md`** (the same orientation for a person). Both
  are written only when absent — from the moment they exist they are the user's
  documents — and only for a workspace Brain is initializing, never dropped into
  a root that already holds content. Templates live in `templates/workspace/`,
  embedded into the binary.
- **Crash-recovery journals never leave the machine that wrote them.** Brain's
  multi-file writes (portable users, triage habits, task schema) stage a journal
  plus backup copies inside the workspace, and its setup locks live there too.
  Those artifacts are excluded from sync and never trigger the change-watcher,
  because a journal means "undo this" and is only ever true locally: transferred,
  the next machine to read that file would roll its own copy back to the backup
  and then push the rollback outward, reverting the workspace from one
  interrupted edit.
- Machine-local agent build artifacts inside a workspace (`node_modules/**` and
  `__pycache__/**` at any depth, `*.pyc`, plus
  `.opencode/{package.json,package-lock.json,bun.lock,.gitignore}`)
  are excluded from sync and never trigger the change-watcher. Every machine
  rebuilds them for itself. Brain's `.opencode/plugins/brain.js` bridge is still
  synced.
- A sync whose remote is missing `tasks/SCHEMA.json` **publishes it** rather than
  refusing, provided the remote's task CSVs hold no legacy rows. Remote CSVs that
  genuinely predate the current schema still refuse, naming
  `brain workspace migrate` as the remedy. Whether the remote is legacy is
  decided by what its CSVs contain, never by whether CSV files exist.
- A machine opening a workspace it has never had gets its **task store seeded
  before the first sync** (both CSVs, both id counters, and the schema document),
  because the sync's CSV lane reads them and bisync excludes them. The schema
  document is taken from the remote when it has one, so a customized workspace
  schema reaches every machine; Brain's canonical document is the fallback.
- A machine opening a workspace it has never had, whose remote is already synced,
  **adopts that workspace's portable identity from the remote** and says so
  (`Adopted <name>'s portable identity from the remote`) before its first sync. A
  remote owned by a different workspace UUID is refused without writing anything.
  A remote with no manifest falls back to minting from the registry UUID, which
  is the genuinely-new-workspace case.
- Every workspace is seeded with the canonical `tasks/SCHEMA.json` when it has
  none, on both the empty-workspace and ordinary root-initialization paths, so a
  workspace created before Brain shipped the document repairs itself on the next
  command. An existing document is never overwritten.

For create, attach, remove, and repair, brain collects and validates all missing
values from `/dev/tty` before legacy classification, migration, or mutation.
EOF/cancellation leaves legacy env and pointer bytes, root contents, manifests,
backups, and registry bytes unchanged. Complete flags and positional values do
not open the terminal; after preflight they perform any required migration and
execute normally.

Every ordinary command crosses one readiness gate after selection. A workspace
must have a compatible manifest whose UUID matches the registry. When
`.config/users.json` exists, it must contain at least one portable person and
the machine's `local_user_id` must name one of them. The first interactive
ordinary command creates that first person, optionally collecting receiver
contacts already configured for the workspace, selects the person locally,
and continues. When exactly one person already exists and no local user is set,
any command (interactive **or** headless) silently adopts that sole person as
this machine's local actor and continues, printing a one-line note; a user is
never told to run `brain user local` when there is only one possible choice.
When two or more people exist and none is this machine's, the interactive
prompt **offers the roster** — `Who is this machine?` followed by
`<n>) <id> (<name>)` per member — and takes a row number; an exact ID still
works, and an answer matching neither re-asks instead of ending the command the
user actually ran. So `brain receiver setup` on a shared workspace asks who you
are and carries on rather than sending you off to run `brain user local`.
Headless invocations with a genuinely ambiguous gap never open `/dev/tty`; they
stop with exact `brain user add` and `brain user local` commands. Create and attach remain
registry-only setup operations. For compatibility, an existing workspace with
no `users.json` and a non-empty legacy local ID remains ready and is not
silently migrated only when the ID is already exact lower-case kebab case.
Brain treats that ID as a compatibility actor for the request without writing
portable user data. A malformed nonblank legacy ID stops with an exact
`brain workspace repair --local-user-id` command instead of failing later in
actor bootstrap. Help, version, and hidden internal server execution never
prompt.

Every public command except help and version runs automatic machine migrations
before workspace bootstrap. On upgrade Brain removes superseded global
lifecycle hooks and installs or repairs the workspace-local Claude, Codex, and
OpenCode lifecycle artifacts in every existing configured workspace. Matching
versions reconcile the same target state, so a deleted managed hook comes back
on the next command. Workspace-local compatibility shims keep hook commands
cached by already-running agent processes forwarding to the new generic hooks;
current frontend settings never register those legacy paths. The version stamp
is best-effort during ordinary startup:
if its directory is read-only, idempotent reconciliation repeats next time and
the requested command retains its own diagnostics. `install.sh` detects an existing binary and performs the
same forward transition during upgrade or the registered reverse transition
before downgrade. The receiver cleanup fence has its own 0.84.8 boundary.
Upgrade reconstructs a missing fence half only from one fully attributed
registration and native-session row that match the durable conversation's
frontend, user, channel, and native binding plus the job's workspace,
conversation, channel, and known cleanup half. Exact acknowledgement repeats
that proof before releasing a complete tuple. Ambiguous or mismatched evidence
fails closed without releasing either unproved resource. Downgrade to 0.84.7 terminalizes
cleanup-pending recovery, preserves the exact cleanup tuple and lock, and makes
the work non-replayable by the older binary. Users do not run a migration
command.

After readiness, the selected workspace and one resolved actor are pinned in
the command context for the invocation's lifetime.
Root-local config and personalization, task paths, reindex scripts, TUI state,
locks, responses, and sync runtime files all derive from it. Two workspace UUIDs
may hold TUIs and run syncs concurrently; a second TUI or sync for the same UUID
is still rejected or coalesced. Changing the machine default affects only a
future invocation. Brain-owned child scripts receive the selected workspace
and immutable actor identity through `BRAIN_WORKSPACE_ID`, `BRAIN_WORKSPACE`,
`BRAIN_ROOT`, `BRAIN_ACTOR_ID`, and `BRAIN_CHANNEL`. Agent panels also receive
`BRAIN_AGENT_KIND`.

The first record becomes the default; later create/attach operations preserve
it. Rename updates the default's canonical name when needed. Changing the
default workspace never changes access mode, UUID, root, local user, receiver
switch, or env. Removing a workspace detaches the machine record only.

The first migrated or created workspace defaults to `unrestricted`; later
created or attached workspaces default to portable `workspace_only`. A selected
valid schema-v2 record is checked before use: missing modes are seeded from its
current default/nondefault status, while valid existing modes are preserved.
Listing or explicitly migrating the registry checks every record. Changing
the machine default cannot rewrite either mode. Every interactive, SMS, email, resumed,
fresh, and daily-triage agent launch snapshots the selected workspace mode from
trusted config. `workspace_only` adds advisory system/developer instructions,
selected-root cwd, and a filtered child environment. The PTY evaluates the
configured frontend command without loading login or interactive shell
profiles, so those profiles cannot restore filtered variables. An initial
prompt uses the adapter's protected prompt argument (an option terminator for
Claude/Codex and one quoted `--prompt` value for OpenCode), so option-looking
user or inbound text stays prompt data. `workspace_only` is advisory prompt
enforcement plus best-effort capability filtering, easy to bypass, and not
tenant isolation. It reduces accidents and naive leakage among trusted users.
Real adversarial or sensitive isolation requires an external OS, VM, machine,
or container boundary. Claude, Codex, and OpenCode continue to use the
user's shared frontend login; selecting a workspace does not create another
identity.
A pure literal-path check can warn about obvious absolute or `~/` paths outside
the root, but paraphrasing, aliases, links, and indirect requests can bypass
it; it is deliberately not a prompt-injection detector.

### `brain user`

Portable members live in `<brain-root>/.config/users.json` and travel with the
workspace. IDs use exact lower-case kebab case and identify people, not devices
or authorization roles. The same person may use the same ID on multiple
computers; machines do not create distinct identities for that person.
`brain user list` shows every member.
`brain user add` and `brain user update` accept a
display name, repeatable `--phone`/`--email` or
`--add-phone`/`--add-email` values, and an optional `--response-email`.
Interactive invocations prompt for missing required values.

Phones are stored as unambiguous E.164 values; common North American formatting
is accepted only when it can be normalized without guessing. Emails are
trimmed and ASCII-lowercased, with no provider-specific alias rewriting.
Only contacts explicitly marked by the add/update commands are enabled for
inbound identity resolution. An enabled phone or email may identify only one
portable person. Compatibility setup offers an email only when the email
receiver allowlist is configured. A legacy response email migrates to the first
person only when it matches that allowlist; otherwise it remains unresolved
for explicit review.

`brain user reassign [<from>] [<to>]` moves every task and habit assigned to one
raw `assigned_to` value onto an existing portable person. `<from>` is any literal
value found in the task files, including one that was never a portable member
(`me`, a first name, a retired ID); `<to>` must already exist. Interactive
invocations list the assignment values that name nobody in the registry, then
list the members, and accept a row number or an exact value at either prompt. The
reassignment reports how many tasks moved, never adds or removes a person, and
writes nothing when no row matches. Both CSVs are replaced through the same
grouped portable transaction used by removal.

`brain user local [<id>]` selects an existing portable person for this machine.
`brain user remove [<id>]` refuses to remove the last person and scans both
`tasks/tasks.csv` and `tasks/habits.csv` for `assigned_to` (plus the legacy
`assignee` heading). If work is assigned, removal requires
`--reassign-to <existing-id>`. Brain stages and syncs replacement files plus
mode-preserving backups, publishes a portable recovery journal, then installs
assignment files before `users.json`. The workspace UUID-scoped machine lock
serializes this sequence. Ordinary errors restore the complete old generation;
if the process stops after the journal is published, the next portable-user
load performs the same recovery before continuing. Journal removal is the
commit point, after which leftover staging files are safe to clean up.

Task and habit readers temporarily accept the legacy `assignee` heading and
prefer `assigned_to` when both appear. Any later write migrates the heading by
name, preserves the value, and emits only `assigned_to`. New rows default to
the immutable effective actor for the request. An unrelated edit never changes
assignment; `--assigned-to` creation overrides and explicit reassignment must
name a portable workspace member. One-person workspaces keep filling the ID but
hide assignment detail, controls, and filters. Shared workspaces expose those
surfaces without changing the actor default. The task shell resolves this mode
once from the selected workspace's portable registry. Shared task cards show
their assignment, `Ctrl+P` adds **Add task** and **Filter by assignee**, and a
task's `Enter` actions add **Reassign this task**. The filter opens a captive
numbered member picker with an **All assignees** clear row and remains visible
in its own task-header row while active. That live row is the header's only
assignment state; static chips retain other CLI filters but never a stale
startup assignee. Switching members and clearing to
**All assignees** always work from the complete current-view data. Add and reassign hand the interactive choice
to the embedded agent's `/todo` flow; the scripts remain the noninteractive
path. `brain tasks --assigned-to <user-id>` validates the ID against the
selected workspace and initializes the same process-scoped filter used by the
picker; plain output applies the equivalent final filter. A one-person or ready
legacy workspace keeps assignment-specific TUI surfaces hidden, but an explicit
valid startup filter remains recoverable because task-view `Esc` clears it
before quitting.

New task and habit rows also receive immutable UUIDv4 `task_uuid` values.
Commands still locate rows by mutable display `task_id`, then preserve the
matched UUID during completion and edits. A spawned habit occurrence receives
a new UUID while retaining assignment and `system_key`. Deterministic UUIDv5
conversion for legacy rows is activated only by explicit workspace migration.
It requires the rollout-owned last-legacy-sync state, an existing durable
machine-local backup base, and an explicit destination beneath that base.
Existing legacy CSV sync remains keyed by `task_id` until coordinated
migration. The coordinator holds the UUID sync lock from local CSV migration
through remote CSV and baseline publication, with schema metadata last. An
active rollout journal blocks ordinary sync and setup until migration resumes.
The helper rejects backup/workspace path overlap, creates a deep backup
path one component at a time while syncing every actual parent, durably syncs
every exact backup, and journals the three-file replacement so a retry
recovers from failure or interruption at any replacement boundary. Journal
publication errors also remove their temporary file before returning. Once a
coordinator activates schema version 2, sync switches to immutable
`task_uuid` identity while `task_id` remains a mutable display label.

Managed triage habits are a portable per-workspace feature, enabled by
default. Brain identifies its daily and weekly chains with
`brain.triage.daily` and `brain.triage.weekly`, independent of visible names.
TUI startup, task reindex, and a successful explicit `brain sync repair`
restore one open occurrence per enabled chain. Ordinary CLI, TUI, web, and
skill mutation paths refuse to remove, revive, or skip managed rows.
**Completing one is never refused**: being managed means Brain owns the chain's
existence and cadence, not that you may not tick an occurrence off, so
`brain tasks complete`, the TUI's mark-complete, and the habits page all treat a
managed occurrence like any other habit (done today, next one spawned, still
carrying its `system_key`). The triage skill additionally has a narrow
marker-aware helper that completes the chain by `system_key` when it has no id
in hand. Every native
or bundled-script task, habit, and display-counter writer participates in one
workspace UUID-scoped task-store lock. Python writers also verify the exact
bytes they read before atomically replacing a CSV, and `/todo remove` uses the
protected removal script instead of editing a row directly.

`brain config set enable_triage_habits=false` stages config, both task CSVs,
and affected derived references as one recoverable transaction. It removes
open and completed managed history while preserving unmarked similarly named
habits and unrelated transcripts. The startup modal stays suppressed, but
manual daily and weekly triage continue without habit mutation. Re-enabling
creates fresh UUIDs and does not restore history. The grouped journal is bound
to the selected workspace ID and root, accepts only schema-defined live targets
and exact sibling artifacts, and rejects duplicates, symlink ancestors, and
path escapes. Existing live files are replaced atomically from synced staged
files, so readers never observe a missing-file interval.

### `brain reindex`

Rebuilds brain's **derived lookup indexes** from their canonical sources.
The two lookup CSVs are derived, not authored:

- `projects/projects-lookup.csv` — one row per `projects/<name>/.METADATA.json`
  (archived projects excluded); `directory` comes from the filesystem path, so
  a renamed folder is reflected without editing JSON.
- `resources/zotero-lookup.csv` — one row per `resources/**/.METADATA.json` plus
  a scan of the colocated `notes.md` for `has_summary` / `has_other_notes` /
  `annotation_count`. Resource metadata is heterogeneous (a numeric or string
  `year`, an `item_type` or `type` key, optional/`null` fields), so parsing
  coerces defensively rather than dropping records.

`brain reindex` (bare) rebuilds both CSVs and re-applies the task/habit rules;
`--projects` / `--resources` / `--tasks` narrow the run. The `--tasks` half
shells out to the shared `/todo` rule scripts with the selected root in
`BRAIN_ROOT`, so non-default workspaces are reindexed in place.
Output is LF-terminated, matching brain's other CSVs. This is the operation the
`/second-brain reindex` and `/todo reindex` skill rows invoke.

### `brain sync`

Manual, bidirectional cross-machine sync of the selected workspace root to a
private Backblaze B2 bucket, via `rclone bisync`. Sync
is **opt-in**: with no configured `sync` block (see [config.md](config.md)),
`brain sync`, `brain sync repair`, `brain sync status`, and `brain check` print a
plain explanation that cloud sync is not set up yet and end with the exact next
command: `brain sync setup`. These commands create the configured brain root on
demand if it does not exist yet; `brain env` does not create it because env
configuration lives outside the brain root.

During a configured run, sync prints the current phase as it proceeds: the
workspace-scoped lock, local manifest validation, remote identity probe,
comparison and selected direction, rclone's live file progress, task/habit CSV
merge, and journal write. If `rclone` is missing, it stops before remote work
and prints two clearly labeled installation choices: the Homebrew command
(`brew install rclone`) or the official installer command.
If a rollout journal is active, ordinary sync refuses immediately after taking
the UUID lock and directs the user to resume `brain workspace migrate`.

- `brain sync` (bare) — bidirectional sync; a same-file conflict is resolved
  by newest edit.
- `brain sync --push` — upload local additions and edits with a one-way,
  non-deleting `rclone copy --update`. It never downloads remote-only files;
  deletions reconcile during a later bidirectional or pull-biased sync.
- `brain sync --pull` — biases this run remote-wins on a same-file conflict.
- `brain sync setup` — the first command for a machine that has not enabled
  cloud sync yet. It says that it is enabling cloud sync on this machine, then
  runs a guided walkthrough. It first asks *"do you already have
  a Backblaze private bucket to connect to?"*; answering no prints a step-by-step
  guide to creating one (private bucket, Default Encryption **enabled**, Object
  Lock **disabled**, and a bucket-scoped application key) and waits for you before
  continuing. Then it collects the B2 bucket + credentials (writes the `sync`
  block into **brain env**, not brain config, see [config.md](config.md)). The
  bucket must already exist and be reachable. Setup validates the selected
  workspace's existing local manifest and probes remote
  `.config/workspace.json`. It displays the local canonical workspace name and
  UUID, configured remote target, observed remote status, and remote UUID when
  a compatible manifest supplied one. A matching identity proceeds; an empty
  remote first receives an append-only exact-manifest claim under the selected
  UUID. Publishing a new claim is a staging attempt only: setup stops before
  canonical publication or credential persistence and asks the user to retry.
  A retry enumerates and validates the durable claim set, deterministically
  elects one UUID, and only the winner may publish and read back the canonical
  manifest. This two-phase rule remains safe when object copy is not atomic and
  a lower competing UUID arrives after another claim. A nonempty
  manifestless remote requires an explicit `y`/`yes` confirmation, or
  `--adopt-workspace-id <UUID>` with the exact selected UUID for noninteractive
  authorization. A generic `--yes` does not authorize adoption. Mismatched,
  malformed, incompatible, and present-but-unreadable remote manifests remain
  hard refusals. Every authorized initialization or adoption publishes and
  verifies the manifest before credentials or any other remote data are written.
  Setup holds the workspace UUID sync lock across that identity protocol,
  any safe empty-remote task-schema transition, creation of the `RCLONE_TEST`
  check-access marker on both sides, and the initial bisync baseline. It saves
  the candidate credentials only after that baseline returns `Clean`;
  `NeedsAttention`, `Aborted`, or transport failure leaves them unsaved. A
  current but unconfigured local workspace may publish its current task CSVs,
  baselines, and schema marker to an empty compatible remote before the first
  baseline. It refuses to overwrite legacy remote CSVs. If a workspace
  migration journal is active, setup refuses before remote identity work.
- `brain sync repair` — (re-)establish the bisync baseline for a machine that
  already has `sync` env configured. A normal sync automatically performs this
  narrow repair when rclone reports a missing check-access marker, announcing
  that it is running the repair and why. Use this command directly for an
  explicit repair or when rclone reports another baseline problem. It
  recreates the `RCLONE_TEST` marker on both sides before the resync. It does
  **not** collect Backblaze credentials or enable cloud sync; if it is run
  before setup, brain explains that `brain sync setup` must come first.
- `brain sync status` — if a sync is running right now (in a detached
  background process or another shell), a `syncing now: <dir> · started … ·
  pid …` line first; then the last completed run (from the local sync
  journal), the startup-pull/change-push/message-pull policy (with the
  watcher's debounce window shown as `(3000ms debounce)`), and the count of
  open conflicts.
- `brain sync conflicts` — list open conflict copies using the same strict
  friendly-name parser as the structured form. `--json` emits those same
  groups as JSON (one object per canonical original, its
  `original_exists` flag, and its `copies` with `host`/`date`/`modified`/
  `bytes`) instead of the themed line-list — meant for agents/skills to
  consume, e.g. the `/second-brain resolve-conflicts` skill.
- `brain sync resolve <original> [...]` — after you've merged a conflict back
  into its canonical file, safely delete that original's leftover conflict
  copies (never the canonical itself). Refuses (and deletes nothing) if the
  named original doesn't exist — merge into it first. Bare `brain sync
  resolve` (no arguments) drops into an interactive picker over the currently
  open conflict groups. Deletion only: it never runs a sync itself.
  - **Both sides.** rclone writes the losing copy on the local root *and* the
    remote, and both conflict-name patterns are sync excludes, so a normal
    `brain sync` can neither remove the remote object nor bring it down.
    `resolve` therefore also deletes that original's loser objects from the
    remote (matching rclone's raw `<original>.__brainconflict__<N>` names,
    which only the local side ever gets renamed away from), reporting them as
    `(removed 1 copy, 1 remote object)`. It still never runs a sync, and never
    touches anything but that original's losers. With no remote configured, or
    no `rclone` installed, it quietly stays local-only. A remote it *cannot
    reach* is reported as `could not check the remote` rather than passed off
    as clean.
  - Because the remote half is independent of the local half, resolving an
    original whose local copy is already gone still collects the remote orphan
    (`resolved X (no local copies, 1 remote object)`) — which is how a conflict
    resolved by an older brain, or by another machine, gets cleaned up.
  - *Caveat:* a conflict copy is recognized purely by its
    `name (conflict <host> <YYYY-MM-DD>).ext` shape, so a genuine file you
    happened to name exactly that way is indistinguishable from a real
    conflict copy — `conflicts`/`resolve` would treat it as one. Don't hand-name
    files in that pattern.

Like `config`/`env`/`persona`/`skills`, `sync` is dispatched **before**
the `markdown-to-pdf` prerequisite gate, so it always works even when that
tool is missing.

**Live progress.** A running sync is no longer a silent block. Before any slow
work begins, brain prints the sync mode, local root, remote target, and the
plain-English plan. It then announces lock coordination, local manifest
validation, the remote identity probe, safety-marker check, rclone handoff,
task/habit CSV merge, and journal write before each phase starts. During the
rclone phase, file progress streams to the terminal live, with a one-line
update roughly every 10 seconds (files/bytes transferred, percent complete,
transfer rate and ETA). This is useful on the first sync of a large brain,
which can take a while.

Every sync (foreground or background) mirrors that same progress to a machine-
local log (`<workspace-cache>/sync/current.log`) and records a small `current.json`
"a sync is in progress" marker while it runs. That is how a background sync
stays observable without ever printing to a terminal: `brain sync status` reads
the marker, and a `brain sync` run started while another sync is already going
**attaches and follows** that live log to completion instead of starting a
second sync or erroring (Ctrl-C stops watching; the sync keeps running).
The marker, log, journal, lock, rclone workdir, and task/habit baselines all
belong to the selected workspace UUID. A status or follower invocation cannot
read another workspace's current run or history, and two different workspaces
may hold their sync locks concurrently.

**Never renders into the TUI.** Automatic syncs run in a **separate detached
process**, never on a thread inside the persistent shell, so their output can
never bleed over the TUI. (This is also why quitting the shell can't interrupt a
sync — see below.)

**Crash-safe / resumable.** brain owns rclone's bisync working directory
(`<workspace-cache>/sync/bisync`) rather than leaving it at rclone's default. Since
brain's workspace lock already serializes that workspace's syncs, any leftover rclone lock
file in that workdir is necessarily from a dead, interrupted run (a quit shell,
a powered-off machine), so brain reaps it before each run. If an interrupted run
left the baseline listings unusable, the next sync detects it and self-heals
with a one-time resync automatically — you never have to know a sync was
interrupted, and turning off the machine mid-sync never leaves a stuck state.

**Automatic sync (startup pull / periodic pull / change push / receiver
freshness / receiver completion push).** A configured shell checks for remote
changes every five minutes in addition to reacting to local and receiver
events. There is no exit sync.

Every trigger below spawns a **detached background `brain sync` process** (with
the canonical `--workspace <workspace>` plus `--if-idle`, so changing the machine
default cannot redirect it and an alias is never propagated). The child also
carries the selected UUID in `BRAIN_WORKSPACE_ID`; bootstrap refuses to run if
that expected UUID disagrees with the selected registry record. None runs a
sync on a thread inside the shell. The shell never waits on, and can never be
interrupted by, the network.

- **On start.** Opening any sync-configured shell always kicks a pull-biased
  background sync so local state catches up with the remote. The first frame
  renders immediately, while the footer shows that sync is active.
- **Every five minutes while open.** Each sync-configured shell starts a small
  timer that launches a pull-biased background sync every five minutes. The
  workspace lock coalesces timers from multiple shells, so a long-running main
  or receiver TUI cannot remain stale indefinitely.
- **Live watcher (`sync.watch`).** While the shell is open, a filesystem
  watcher starts a one-way, non-deleting upload after edits under the brain
  root settle (`debounce_ms`, default 3000ms). A burst coalesces into one push.
  It does not download remote files, write task CSV merges back locally, or
  advance the downstream freshness timestamp, so it cannot create a
  self-triggering sync loop. Each live TUI owns one watcher for its immutable
  workspace context. Closing it stops only that watcher, without affecting a
  peer workspace's watcher.
- **Before receiver work.** Before an inbound SMS/email starts LLM work, brain
  checks the latest successful downstream journal row. If it is more than two
  hours old (or missing), brain queues the message, starts a pull, shows
  `syncing brain before receiver message` in the footer, and dispatches only
  after that sync completes. This gate lives at the exact live TUI job-consumption
  boundary, so it delays only that workspace's queued job; the shared server
  does not own it. This is a threshold check at message time, not a two-hour
  timer. Launch detection and retries are finite: brain polls at 250ms, allows
  five seconds for a pull to appear, and tries at most three launches before
  continuing with local state and a visible warning.
- **After receiver work.** A completed receiver turn immediately starts a
  push, so files or task rows created by the agent do not wait for the watcher.
  A request to add, create, capture, remember, or track a task is explicitly
  treated as task capture, unless the sender also asks Brain to perform it now.
  Every successful downstream sync also reloads the live TUI's task state, so
  newly pulled task rows appear without a manual refresh.

All five are journalled like manual syncs and **coalesce** through a
workspace-UUID lock: concurrent triggers (startup + watcher + receiver gate + a second shell + a
manual `brain sync`) never run two rclone syncs for the same workspace at once.
Different workspaces may sync concurrently. A redundant background
trigger exits silently; a user-run `brain sync` instead *follows* the in-flight
one. All are best-effort: a held lock, an unconfigured brain, or a spawn
failure never crashes the shell. With no configured `sync` block, no automatic
sync runs. `brain sync status` and the command palette's **Show sync status**
action report whether a sync is active.

**The daily-triage nudge waits for the startup sync.** Today's triage may have
been done or skipped on another machine, and that only reaches this machine's
`habits.csv` once the startup sync lands. So on a sync-configured machine,
brain does **not** show the triage modal at open: the shell is usable
immediately, with no modal to dismiss. It waits for the startup sync to finish,
reloads the synced tasks/habits, and only *then* shows the "run today's triage?"
modal — and only if triage is still not done for today. If another machine
already handled it, no modal ever appears. If Help, a palette, or another
captive overlay is open when the sync finishes, Brain keeps the refreshed
decision pending and shows the triage modal as soon as that overlay closes.
With sync unconfigured, the check runs immediately at open as before.

**The nudge never waits for the sync.** It is evaluated as soon as the shell
opens, so on a sync-configured workspace it appears immediately rather than after
a pull completes. The post-sync refresh then *reconciles* it: if the synced habits
show today's triage was already completed (on another machine, or by someone else
on a shared workspace) while the nudge is still on screen, Brain withdraws it and
flashes "daily triage was already done on another machine" instead of leaving a
question that has already been answered. If the sync instead reveals outstanding
triage and no nudge is up, it raises one.

**Opting out.** Setting `enable_daily_triage_check=false`
(`brain config set enable_daily_triage_check=false`, portable config that rides
the workspace to every machine) suppresses only the alert. Every shell launched
against the workspace starts with the nudge disabled until the value is set back
to `true`. Brain still arms the post-sync refresh gate, then
strictly reloads portable config, reconciles managed triage policy, and reloads
the task tables after a clean startup sync; it skips only the modal check. The
modal cannot open while suppression remains enabled for the running process.
Mid-session, the command palette's **Disable/Enable daily triage
alert** toggle flips the same state *and saves it*, so a long-running TUI that
spans several days can suppress or restore the nudge without a restart, and the
answer sticks. A failed config write is reported and the running session still
honors the flip. If the
palette restores the alert while startup sync is still pending, Brain defers
the check until synced config, managed policy, and tasks have refreshed. To
disable the nudge permanently instead, clear the `daily_triage_name_pattern`
config variable.

#### Migrating a machine to sync

A short runbook for bringing sync online: once for a new bucket, then once
per machine you want to join it.

1. **One-time: create the bucket.** Before the first machine can connect,
   someone creates a private Backblaze B2 bucket to sync to — Default
   Encryption **enabled** (B2 manages the keys), Object Lock **disabled**, and
   a bucket-scoped application key. This step happens in the Backblaze
   console, outside brain; if you tell `brain sync setup` (below) you don't
   have a bucket yet, it walks you through creating one and waits for you to
   finish before continuing.
2. **Per machine: `brain sync setup`.** On every machine you want on sync —
   including the one that just made the bucket — run `brain sync setup`. It
   confirms `rclone` is installed, collects the B2 bucket name and the
   application key/keyID (either from a bucket you already have, or the
   walkthrough from step 1), writes the `sync` block into that machine's
   `~/.config/brain/env.json` (see [`brain env`](#brain-env) above — this is
   machine-local and never rides into the bucket itself), and establishes the
   bisync baseline. On a brand-new machine with an empty selected root, that
   initial baseline is effectively a full pull of everything already in the
   bucket. If the selected target already contains legacy data but no workspace
   manifest, review the identity summary and confirm the adoption. Automation
   must pass `--adopt-workspace-id <UUID>` with the exact local UUID; `--yes`
   alone is insufficient.
3. **Verify the triggers.** Run `brain sync status` and confirm it reports
   startup pull, five-minute periodic pull, change push, the two-hour receiver
   freshness policy, and the last run.
   Auto-sync is on by default the moment `brain sync setup` finishes — you
   don't need to flip anything else on.
4. **Env auto-migration.** Before a valid schema-v2 registry exists, the legacy
   `~/.config/brain-root` pointer and
   `config.json`'s `markdown_to_pdf_path` are migrated into
   `~/.config/brain/env.json` during the one-time migration path. Once the
   registry is valid, ordinary commands never consult legacy root/config input.
   Migration is a no-op on a brand-new machine with no legacy pointer or config —
   see [`brain env`](#brain-env) above. No manual step needed; it happens
   quietly, before `sync setup` even starts.
5. **Confirm it actually works, across two machines:**
   - An edit, add, or delete on machine A shows up on machine B after each
     machine's next `brain sync` (or automatically, once the triggers above
     are live).
   - `tasks.csv` and `habits.csv` merge silently — editing or completing
     different tasks on both machines never leaves behind a `(conflict …)`
     copy of either file.
   - Editing the *same* prose file on both machines at once (before either
     syncs) produces exactly one keep-both `(conflict …)` copy, which
     `/second-brain resolve-conflicts` merges back into the canonical file and
     clears with `brain sync resolve`.

Once these five steps check out, the machine is fully onboarded: sync runs
itself from here via the triggers in the previous section.

**Optional: syncing your own env.json.** `brain` itself doesn't care how
`~/.config/brain/env.json` gets onto a machine — it just reads a standard XDG
config file. If you want to skip re-running `brain sync setup` by hand on
every new machine, you can track that file privately in your own dotfiles
repo (or similar) and let it carry your bucket + credentials across your
machines. This is entirely optional and outside brain's own sync mechanism.

### `brain check`

A read-only report of what a plain `brain sync` would do, without doing it.
Runs the same `rclone bisync` argv as `brain sync` (bare, `Direction::Both`)
but with `--dry-run` appended, so the normal file lane transfers and writes
nothing. Detected file changes are classified and grouped exactly like the
live sync's detection phase. Before doing remote work, it prints the phase it
is entering: first the rclone dry-run for regular files, then the task/habit
CSV baseline check.

Because `tasks/tasks.csv` and `tasks/habits.csv` are excluded from bisync,
`check` also performs a read-only CSV pass: it compares the cached CSV
baseline with the local CSV for rows to push, fetches the remote CSV with
`rclone copyto` into a temp file, and compares that remote text with the same
baseline for rows to pull. CSV summaries show row deltas as `+A ~C -D rows`
(added, changed, deleted), and a failed remote CSV fetch becomes a warning
instead of a false clean report. If this machine's CSV baseline is missing,
`check` avoids the confusing "everything pushes and pulls" preview: identical
local/remote CSVs report cleanly, and when both sides are non-empty it treats
the remote CSV as a provisional snapshot for local row deltas. The command
never writes local CSVs, remote CSVs, or baselines. It resolves the active task
schema once from `tasks/SCHEMA.json`: inactive workspaces compare by `task_id`,
while active schema-v2 workspaces compare by `task_uuid`. Malformed schema
metadata, CSV records, or duplicate active identities produce a themed warning
that names the baseline/local/remote generation and CSV instead of panicking or
claiming the workspace is in sync.

- Nothing pending on either side: a single `✓ In sync — nothing to push or
  pull.` line.
- Otherwise: a `Changes to push (N):` and/or `Changes to pull (M):` heading
  (only for the side(s) that have pending changes), each followed by grouped
  file summaries (e.g. `2 changes in notes/`) and CSV row summaries
  (e.g. `tasks.csv: +2 ~1 -0 rows`), then a suggestion line naming the right
  follow-up (`brain sync` to push, to pull, or to push and pull). When CSV row
  summaries are present, the report says they are comparisons against this
  machine's cached baseline, not proof that another machine made the change;
  `brain sync` still merges `tasks.csv`/`habits.csv` by id and refreshes the
  baseline.

Like `sync`, `check` is dispatched before the `markdown-to-pdf` prerequisite
gate and needs no configuration beyond what `brain sync setup` already wrote;
run against an unconfigured or baseline-less brain, it prints the same
"not configured" / "no baseline yet" guidance as `brain sync` instead of
erroring.

**Auto-resume (never-miss guarantee).** If rclone reports a missing prior
baseline listing, brain never reports the run as done. It announces the
condition and makes one internal resync attempt before continuing. This is
based on rclone's listing error, not on a missing-rclone run: when rclone is
not installed, brain exits before invoking it and records no sync attempt. If
the resync reports a missing check-access marker, the normal sync then
automatically runs the narrow marker repair and announces that recovery.

**Deletions propagate both ways.** `rclone bisync` mirrors deletes as well as
edits: deleting a file locally removes it from the B2 bucket on the next
sync, and removes it from every other machine the next time *that* machine
syncs. This is guarded by the `--max-delete` safety check (see
[integrations.md](integrations.md)) so a wiped or never-initialized side
can't wipe out the other; short of tripping that guard, delete is a real,
bidirectional operation, not just a local one.

**Selective sync (optional, off by default).** `sync.exclude` (extra rclone
exclude patterns) and `sync.max_size` (skip files above a size cap) let you
keep large or unwanted paths out of the bucket entirely — e.g. bulky media or
scratch data under a resources directory. Both default to empty, so an
unconfigured brain syncs everything, unchanged from before; see
[config.md](config.md) / [data-model.md](data-model.md) for the fields.

**Keep-both conflicts.** When the same file changed on both sides, rclone
doesn't drop the losing edit: it keeps that copy, and brain renames it to
`name (conflict <host> <date>).ext` alongside the winner, so nothing is
silently lost. Conflict copies are themselves excluded from sync, so they
don't fan out to every machine. `brain sync conflicts` lists them (`--json`
for the structured, agent-consumable form); the `/second-brain
resolve-conflicts` skill reads that JSON, merges each group into its
canonical file, then clears the copies with `brain sync resolve <original>`.

**Task CSVs merge by their active schema identity, with no conflict copies.** `tasks/tasks.csv` and
`tasks/habits.csv` don't go through the keep-both path above at all. Brain
also excludes `tasks/SCHEMA.json`, so the generic file lane cannot advertise a
new merge identity ahead of its CSVs and baselines. Brain reconciles the CSVs
itself with a
three-way merge (a cached local baseline + your local copy + the remote
copy), writing the merged result back to both sides. Two machines that each
add, complete, delete, or edit different fields on the same task converge
cleanly, so neither file ever produces a `(conflict …)` copy. A side that
marks a task `status=done` always wins that row's status and completed date;
a same-field disagreement otherwise resolves by whichever side's
`last_touched` is more recent. Both `tasks.csv` and `habits.csv` carry that
column; legacy rows without a parseable timestamp fall back to a deterministic
tiebreak, journalled as a soft conflict. After the id-keyed merge, habit rows
get one more guarantee: a recurring habit's next occurrence is a brand-new
row, so if the same occurrence is spawned independently on two machines
before they sync, the two spawns carry different `task_uuid`s and merge as
two unrelated "added" rows — the id-keyed merge has no way to see they're the
same occurrence. A dedup pass collapses any rows sharing the same habit name
and due date back down to one, folding their fields through the same
done-wins and last-touched rules described above, so a sync never leaves
duplicate habit occurrences behind. This never fires on `tasks.csv`, whose
schema carries no recurrence columns. Legacy tables remain keyed by
`task_id`; schema-v2 tables are aligned by column name and keyed by immutable
`task_uuid`. Before reading or merging either remote CSV, Brain fetches and
validates the remote `tasks/SCHEMA.json`. A missing marker or the recognized
pre-v2 marker (with `tasks_csv` and `habits_csv` keyed by `task_id`) means
legacy. Every other present marker must be valid JSON with a typed, supported
schema version and the UUID merge key; malformed, incomplete, wrong-typed,
incompatible, or newer metadata, and a legacy/current mismatch,
refuse the whole lane before publication. A compatibility writer adding or populating that column does not
activate UUID merge identity before `tasks/SCHEMA.json` does so. If distinct
UUIDs claim the same `T###` or `H###`, the smaller
UUID keeps it and the other rows receive deterministic IDs above the greatest
number visible on either side. `blocked_by` chains and project metadata task
lists are rewritten to the final labels; composite `see_also` values are too,
including space-separated and punctuation-wrapped task IDs, without changing
URLs or longer identifiers that merely contain the same characters.
Current-schema output uses one full canonical order for known task columns and
sorts declared forward-compatible columns lexically, so independently migrated
or merged machines serialize byte-stably. Unsupported schema versions, missing identity columns,
legacy rows without `task_id`, or undeclared unknown columns refuse the whole
task/habit operation before any CSV, baseline, metadata, remote, or counter
write. See [data-model.md](data-model.md)
for the merge rules and
[integrations.md](integrations.md) for the transport.

When a configured legacy machine migrates after another machine has already
published schema v2, the migration coordinator uses a dedicated pre-authority
join. It runs the generic rclone lane without the ordinary task CSV lane, then
reconciles the legacy baseline, local legacy rows, and current remote rows by
`task_id` without publishing. Matching rows retain the remote `task_uuid`;
local-only rows receive their deterministic migration UUID during the next
journaled cutover step. Before that join step completes, both local counters
are max-merged with any usable remote counters and floored beyond the exact
joined task and habit display IDs. Missing or malformed counters fall back to
those floors. Replay is byte-stable, the bridge never publishes CSVs or
counters, and schema-last publication remains the only point that can expose
the joined current generation remotely.

The two id counters (`tasks/.tasks_next_id`, `tasks/.habits_next_id`) that say
which id to hand out next are also excluded from bisync and reconciled
separately: take the highest local/remote counter, then raise it beyond the
greatest display ID in the reconciled tables. Push-only sync also applies that
floor locally before another task or habit can be allocated. Neither machine
ever re-hands-out an id the other already used, so there are no id collisions
regardless of which machine synced last.

**Doctor.** `brain tasks doctor` prints one themed report for the selected
workspace. It validates the UUID-scoped session DB and every registry-declared
lifecycle artifact independently. Hook events must contain the current
session-start and session-stop bridge commands; bridge and plugin files must
exactly match Brain's bundled bytes. OpenCode compatibility additionally checks
the configured command's version, required TUI flags, JSON session listing,
generated capability schema, and plugin load in disposable HOME/XDG roots.
Claude compatibility requires a runnable Claude Code 2.1.196 or later whose
version command returns one exact `major.minor.patch (Claude Code)` record,
because receiver observation depends on its `prompt_id` hook field. Older,
malformed, identity-free numeric, noisy, ambiguous, or unavailable configured
Claude commands are reported with an upgrade or `claude_cmd` remediation before
Brain attempts a launch. It reports the rclone probe, and appends the same
redacted requirements matrix used by other status surfaces. Missing rclone with
sync off is informational and does not fail doctor. After the common startup
migration has reconciled lifecycle artifacts, Doctor opens an existing SQLite
database read-only, probes rclone with an explicit no-config path, and never
creates cache, journal, or skill-render state. Hook repair names the exact
installer and selected root.

### `brain persona`

Reads and writes **personas** — content *about the people using this
workspace*, stored beside the config store at
`<brain-root>/.config/personalization.json` (just another brain config, inside
the brain root so it travels with the brain).

A workspace can have several members, so the store holds **one persona per
portable user ID**. Commands address the person at this machine
(`local_user_id`) unless told otherwise. `brain personalize` remains a hidden
alias of `brain persona` so older muscle memory and scripts keep working.

- `brain persona` (bare) — onboarding when the person at this machine has
  nothing set yet: a short, skippable prompt for their name, role, and who they
  work for, then two toggle-checklists for their **project namespaces** and
  **task tags** (all items pre-checked; space toggles, `a` adds
  comma/semicolon-separated new ones). Otherwise prints their current values
  (same as `show`).
- `brain persona show [--user <id>]` — a stable, keyed block (`user:` / `name:`
  / `role:` / `works_for:` / `namespaces:`) for one member. `namespaces:` shows
  the effective set (their list, or the generic defaults when unset).
- `brain persona list` — every member's block, blank-line separated, in user-ID
  order, with the person at this machine marked `(this machine)`. This is what
  the bundled skills read at runtime to learn who they are assisting and who
  else shares the workspace. A member of `users.json` who has personalized
  nothing still appears, with `(unset)` values.
- `brain persona get <user> [<field>]` — everything brain knows about one
  member, or just one field of theirs (`name`, `role`, `works_for`).
- `brain persona set <field>=<value> [--user <id>]` — set and persist one
  member's identity field, leaving every other member's entry untouched. A user
  ID the workspace has never heard of is rejected, naming the members it knows.
- `brain persona edit` — open the raw JSON in `$EDITOR` (edit tag-style
  emoji/labels here; the tag and namespace *sets* are edited with the checklist
  via `brain config set tags|namespaces`, which always edit the local person's).

**Missing personas are collected, not forgotten.** When the person at this
machine has no persona, the *next* `brain` command of any kind runs the short
onboarding prompt before doing its work. With no terminal (a pipe, a script, a
cron run) it instead prints one line naming the command to fix it and continues
— it never blocks or fails the command that triggered it. `brain persona …`
itself and `brain workspace migrate` are the exceptions, since one is already
collecting the answer and the other must not interleave prompts with a
transactional schema change. Other members' missing personas are **never**
prompted for on somebody else's machine; they surface as the `other members' personas` optional feature in `brain workspace status`.

**Legacy stores migrate on read.** The pre-multi-user file was a single unowned
persona. Whichever machine reads it claims it for that machine's local user (the
only person who can truthfully claim it) and the next write persists the keyed
schema.

**Tag styles.** The task renderer's tag → emoji+label mapping is part of a
persona, and the renderer uses the local person's. The binary ships only a tiny
universal default set (`mit`, `personal`, `work`); any other tag renders as its
raw name until a style is added under that persona's `tag_styles`. So the public
binary carries no personal taxonomy.

**Every mutation re-renders skills.** `persona set`/`edit`, onboarding, and
`config set` all run the active deterministic render/install pipeline so the
installed skills stay aligned with the selected workspace.

Like `config`, `persona` runs before the `markdown-to-pdf` prerequisite
gate, so it always works. See [config.md](config.md) and
[data-model.md](data-model.md) for the store layout and schema.

### `brain skills`

Manages the **bundled brain skills**: the skills that ship with brain and are
scoped to the selected brain root.

- `brain skills sync` — render each bundled skill into
  `<brain-root>/.agents/skills/<name>`, then link the project-local Claude,
  Codex, and OpenCode skill directories to it. Idempotent; re-run any time.
- `brain skills sync --root <dir>` — install everything under the selected
  **sandbox** root. Used for testing so a run never disturbs another workspace
  or any global frontend registry.

**Removed skills are pruned automatically.** A sync is a full reconciliation,
not just an install: when a plugin is deleted or renamed, or a bundled skill
leaves the binary, the copy brain previously rendered into `.agents/skills` and
its Claude/Codex/OpenCode links are removed in the same run, and the output says
what it pruned. Frontend links left dangling by a skill that is gone are swept
too. Brain only ever removes what it rendered — it recognizes its own output by
a `.brain-rendered` marker file — so a skill you wrote by hand directly in
`.agents/skills` is always kept and linked. (A skill rendered by a brain older
than this reconciliation carries no marker, so it reads as hand-written until a
sync re-renders it; delete such a leftover once by hand.)

The skills are embedded in the binary, so a fresh clone needs no extra files.
Installing is also triggered automatically in two cases when `skills_auto_sync`
is `true` (the default since the B4 cutover; set it `false` to sync only on
demand):

- after a `config`/`persona` change, and
- **the first time a new brain version runs.** Any command that opens a
  workspace (i.e. anything but `--help`/`--version`) checks the brain version
  that last rendered your skills; if the binary has since been updated, it
  re-renders them once (printing a short `Brain updated: refreshing installed
  skills` line) and records the new version. So a brain update ships its
  bundled-skill changes automatically, the same way it ships code changes, with
  no manual `brain skills sync`. It is deterministic (no LLM), a no-op once
  recorded, and skips cleanly when the workspace still needs setup.

Bundled today: `article-summarizer`, `triage`, `brain-knowledge-capture`,
`second-brain`, `contacts`, and `todo`. See [config.md](config.md) and the
sub-project B spec.

Before writing anything, `brain skills sync` prints the built-skill directory,
the shared registry directory, the number of frontend skill directories it will
fan out to, the extension/plugin source directories it will read, and the prune
step. That
progress trace is default output; `--verbose` remains only for detailed run
logs. Automatic skill refresh after `brain config set` / `brain persona set`
uses the same principle: it prints that installed skills are being refreshed
before writing built copies or registry links.

**Customizing skills without forking.** Two mechanisms, both stored with your
brain (synced, never committed to the repo):

- **Extensions** — personalize a *bundled* skill without a new skill. Put a
  `<root>/.config/extensions/<skill>.md` file with `[hook]` sections; the sync
  injects each hook's text at the skill's matching `<!-- brain:ext hook -->`
  marker in the **built copy** (the repo skill is never touched). Content with no
  matching marker is appended as a "Personal extensions" section, so nothing is
  lost. This is how, e.g., the bundled `triage` skill declares
  `triage:daily-open` / `triage:daily-subagents` / `triage:daily-linear` /
  `triage:daily-merge` / `triage:daily-required-outputs` / `triage:weekly-inboxes`
  / `triage:weekly-linear` hooks so a personal extension can bolt an email pass,
  an issue-tracker reconcile, and a cloud in-basket onto the generic core. The
  `triage:daily-subagents` / `triage:daily-merge` pair lets an extension run work
  **in parallel** with daily triage (launched at the start, collected before Step
  9) and fold its output into the run's output. The tab-close is then gated
  generically: the completion signal carries a `require` list of output paths the
  run declared (an extension supplies them at `triage:daily-required-outputs`;
  core supplies none), and the skill-session tab will not close until every one
  exists, so a premature "done" can't kill the session before an extension's
  printable is written. An empty list closes immediately, keeping the generic
  core and any fork identical to the old behavior.
  The bundled `todo` skill similarly exposes `todo:agenda-after-build` as a
  generic no-op seam. Any installed extension supplies its own runtime content
  and paths explicitly; core does not discover or name extension artifacts.
- **Plugins** — whole skills you own, in `<root>/.config/plugins/<name>/`. The
  sync installs them alongside the bundled cores, into the same registry and
  frontends.

### Habits and receiver servers

The habits server remains one machine-shared, local-only service. The selected
workspace is explicit in `GET /local/<lease>/w/<ingress>/habits` and
`POST /local/<lease>/w/<ingress>/habits/done`. The exact live lease capability keeps local reads and mutations unavailable on
the provider-facing `/sms` and `/email` paths, which carry no capability and no
workspace at all.
The opaque ingress first resolves through the
shared process's live lease table. Only then does the server reload schema v2,
verify the exact registry workspace and root plus matching portable manifest,
and read or write that workspace's habits CSV. Missing, malformed, unknown,
no-live-TUI, unavailable, or mismatched routes never fall back to the machine
default; the unavailable cases return 503. Receiver enablement gates
provider-facing routes, not local capability-protected habits and triage pages.
Local POST routing and live-lease checks happen before body IO, and local
habits/triage action bodies larger than 16 KiB return 413. A provider POST is
the one exception: its workspace lives in the payload, so its (1 MiB-bounded)
body is read first, under the same connection deadline. TUI links retain the ingress accepted at
registration, while `brain habits -w <workspace>` asks the live shared process
for the exact selected workspace's accepted ingress.

The shared process exposes authenticated `POST /sms` and `POST /email` routes —
two paths for the whole machine, naming no workspace. Brain reads the webhook
body first, takes the destination the provider named (Twilio's `To`, a Resend
payload's `to`/`cc`), and selects the workspace that publishes that number or
address as its own `twilio_from_number` / `resend_from_email`. An address no
registered workspace publishes is a plain 404; two workspaces publishing one
address is refused as ambiguous rather than delivered to a guess. The selected
workspace then serves the request only while it has receiver enablement and a
live TUI lease, resolving its remembered ingress before loading provider
credentials, users, prompt content, or the UUID-scoped state DB. Brain verifies
the Twilio or Resend/Svix signature — against the workspace the message was
addressed to, so holding a peer workspace's credential reaches nothing — then
re-checks the now-authenticated destination against that workspace's own
published address before resolving
the normalized sender through the selected workspace's enabled portable phone
or email identities. Unknown and disabled senders are rejected. Resend
timestamps must be within five minutes. Request bodies and serialized job
frames are capped at 1 MiB. The durable ingress queue accepts at most 64
`queued` rows per workspace; progressed, retrying, failed, and done rows do not
consume queued capacity.
Each Resend received-email or attachment-metadata response is also capped at
1 MiB and ten seconds. Unavailable, ignored, and permanent discarded Resend
events receive HTTP success, except that an exact in-flight unavailable
duplicate receives 503 until the pending admission resolves and remembers its
deferred discard. Invalid signatures remain authentication errors. Receiving
API failures return 502.
Email addresses are matched as bare addresses, so the usual
`Display Name <someone@example.com>` header form authenticates normally and
still reaches the reply thread. An email with no plain-text part is reduced
from HTML to readable text, and any inbound message is capped at 16 KiB with
an explicit truncation notice before it is included in the isolated receiver
run's initial prompt.
Accepted provider IDs are deduplicated durably inside their exact workspace
and channel before queued-capacity rejection, including after shared-process
restart. Process memory excludes only simultaneous duplicates and remembers
verified unavailable Email discards; an unavailable duplicate is deferred
without releasing its in-flight reservation. It is not acceptance authority. SMS
numbers use exact E.164 matching, including the leading `+` and country code. A malformed configured
SMS number produces a persistent yellow warning in the TUI status line. The
former generic `/webhooks/capture` route has been removed.

Each ready TUI binds a UUID-scoped job socket for lease validation, registers a
validated live lease, heartbeats it, recovers the shared process after a crash,
and unregisters before removing its socket. Provider ingress no longer sends
accepted jobs through that socket. It commits the complete immutable job and
logical conversation to the addressed workspace DB, and only that commit or a
durable dedup hit permits provider success. Disabled, missing, storage-failed,
and full durable-queue targets receive the existing channel-appropriate
unavailable response with no new row. The shared process never launches an
agent or consumes the queue. The TUI's one durable receiver tick is the only
execution consumer.

The shared HTTP boundary admits exactly four active connections with a fixed
worker set and no application request queue. It caps request heads and local
action bodies at 16 KiB, starts every connection with one absolute two-second
parse deadline, and revalidates a captured live route ticket after workspace
filesystem checks. Receiver body plus local provider verification remain in
that phase; successful verification starts one bounded 30-second provider,
handoff, and response phase only if the parse deadline is still open. Brain
reserves the final five seconds for the response, caps durable admission at two
seconds, and revalidates the retained route ticket again immediately before
enqueue. One absolute handoff deadline bounds SQLite configuration, schema
reconciliation, lock waiting, and the acceptance transaction; completion is
checked again before provider success. Byte-by-byte progress and a slow response
drain cannot renew any deadline. Signed ignored email events are logged as
accepted without enqueue, not as rejected requests.
Conflicting `Content-Length`/`Transfer-Encoding`, repeated
or unsupported transfer codings, invalid field names, and malformed bounded
chunk/trailer grammar are rejected. Framing values accept only `SP`/`HTAB` as
optional whitespace; controls, Unicode whitespace, and chunk extensions are
outside the supported safe subset. Route tickets also carry an authority
incarnation: heartbeats preserve it, while disable/re-enable or identical
unregister/re-register transitions cannot revive an earlier ticket. A revision
overflow rejects its whole enablement or replay transition without changing
lease state. Slow or
partial clients therefore cannot grow the thread set, block control requests,
or make a stale lease authoritative.

Inbound messages never take over, focus, type into, submit through, or wait on
the interactive main panel. The one recurring TUI tick keeps at most one live
receiver run for the workspace process, launches it in a background tab with a
dedicated `AgentController` and PTY, and leaves later arrivals durable and
unclaimed. Claude, Codex, and OpenCode use the same launch, lifecycle,
completion, and shutdown facade. Receiver launch and terminal close never
change the current main view, selected tab, panel visibility, or keyboard
focus; ordinary interactive input remains available on the user's selected
tab. There is no receiver screen sampling, panel-activity wait, warm-panel
lease, or local-input lock.

Each active tick renews the exact claim, resolves the lifecycle-owned native
session for the isolated receiver tab, and asks that tab's `AgentController`
for one bounded content-free observation. Missing evidence remains pending.
Token-, instance-, session-, and owner-matched newer evidence durably proves
`accepted` and `processing`; one snapshot containing both facts applies them
atomically and advances its revision once. After exact progressing evidence,
each distinct later tool event can advance the same revision stream with a
content-free progress pulse while retaining the first progressing timestamp.
Pulse-only reads cross the same `AgentController` facade and exact state
transaction for Claude, Codex, and OpenCode; unrelated turns, children, prior
sessions, wrong scope, duplicate events, and reordered timestamps cannot renew
the job. Claude binds each pulse to the accepted `prompt_id`, Codex binds it to
the accepted `turn_id`, and OpenCode requires the tool callback's assistant
message to name the accepted root user message as its exact parent. A later
non-marker root prompt revokes eligibility in all three integrations. The same
state transaction requires
that exact session tuple to remain locked and registered to the conversation;
stored observation-session continuity cannot substitute for a current lock.
The poll cursor is rebuilt from the durable revision, current attempt's
boundary timestamps, and current attempt's latest progress evidence on every
tick, so process restart does
not replay a prior boundary. Malformed, unrelated, ambiguous, equal-revision,
or regressed evidence leaves the job unchanged. The producer descriptor-confines
its owner-only cache, observation directory, lock, temporary, and replacement
operations, rejects symlink races, and clamps new timestamps across wall-clock
rollback before validating the constructed snapshot. Producer revision saturation
also preserves the last valid snapshot rather than emitting an unrepresentable
revision. An exact Stop against a trusted saturated accepted or progressing
snapshot is a successful terminal settlement without a snapshot mutation. The
stop bridge may then publish its independently validated completed session and
artifact, while malformed, mismatched, and failed writes remain rejected. A
later producer event cannot repair or replace an untrusted prior
entry: symlinks, non-owner-only files, malformed or truncated JSON, wrong
identity, and ambiguous lifecycle shapes are preserved for the strict App poll
to reject in its stable category.

A committed pulse stores its producer timestamp only as monotonic evidence.
Brain samples fresh local authorization time after validation, renews the
five-minute progress deadline from that time, and clamps it to the immutable
30-minute accepted-work deadline. Renewing the 30-second claim remains only a
writer fence and cannot extend either liveness limit. The recurring durable
reconciler now consumes these bounded facts. Recovery-launch App effects remain
separate work.

The stop bridge settles a completed observation inside its session transaction
before publishing the artifact or completed-session state. The TUI requires
that exact completed session and an exact answer artifact in its atomic answer
transaction, so neither surface is accepted from a partially published stop.
Lifecycle completion without an answer is not terminal: Brain may retain its
accepted or progressing facts, but it keeps the agent run active and FIFO
blocked until the exact artifact exists. When both appear in one tick, all
normalized boundaries plus the revision/session cursor are merged atomically
with the artifact answer. Its producer
timestamp remains the durable terminal evidence time, even if it is later than
the renewed lease. Producer evidence never authorizes completion. After exact
artifact and lifecycle validation, Brain samples a fresh App clock for the
lease check; without a lifecycle completion boundary, that same fresh value is
also the durable completion-time fallback. Artifact-only completion records a
representable terminal cursor at revision one or later and the exact completed
native session, without inventing accepted or progressing timestamps.
The immediate answer transaction validates the exact job, token, owner, live
claim, conversation, instance, registered and actual session, frontend,
actor/channel scope, and lifecycle evidence. It appends one authenticated user
turn and the exact assistant answer to the portable transcript, freezes one
final-answer delivery envelope, replaces the native binding, moves the job to
`answer-ready`, and releases the agent claim. The sender is the canonical
number or mailbox proven by authenticated ingress, including normalized
human-formatted receiver configuration, and is never reread from mutable
environment at completion. Email delivery requires the exact bare lowercase
configured-mailbox syntax, including valid local-part dots and domain labels.
Invalid legacy sender shapes terminalize atomically.
If an email job has no trusted accepted recipient, the same
transaction advances transcript and cleanup authority but persists an
unclaimable terminal authorization outcome instead of leaving the run active.
An identical duplicate returns
the existing delivery without another transcript turn or outbox row only when
the persisted completion proof exactly matches the original job, token,
conversation, instance, frontend, actor/channel, registered, actual, and
completed sessions, answer, envelope, rendered turn, and lifecycle cursor.
Later conversation turns or binding changes do not affect that proof; any
conflict fails closed. No provider IO begins in this transaction. After commit,
Brain closes the exact controller and durably opens its cleanup fence. Exact
registration release and exact-instance file removal then retry independently;
file removal may succeed while session release remains pending. Task reload and
the completion push wait for both. Exact controller, session, and artifact
progress remains machine-local and durable until successful, so a later tick or
fresh App retries it without
restoring agent ownership or blocking the next job. Artifact completion, child exit,
lost ownership, and orderly shutdown remove only the exact instance's response,
observation snapshot, and observation lock while preserving durable facts and
unrelated instance files. Poll diagnostics use one content-free shape containing
only opaque job and instance IDs, frontend, prior phase, observed boundary or
`none`, and a stable category. Child exit or orderly shutdown after `launched`
without terminal evidence never replays the prompt. Each enabled receiver tick
now reconciles stalled work and generic response delivery before restart
controls, executes exact cleanup, persists control acknowledgements and
dropped-job notices, rechecks delivery, and claims a due same-session recovery
before later ordinary FIFO work. Active `answer-ready` and `delivering` rows
with an exact semantic outbox row are not agent FIFO blockers, so an older
delivery cannot starve a later due recovery; incomplete delivery states remain
fail-closed blockers. Response delivery uses a separate
oldest-due claim and bounded provider worker. Acknowledged provider references
finish the job, safe failures schedule bounded retries, and provider-specific
ambiguity becomes either an exact Resend replay or terminal failure. The exact
outbound number or email sender is frozen with the authenticated inbound job, so a config
change cannot retarget later attempts; only provider credentials remain live.
Resend HTTP 5xx and concurrent-idempotency conflicts safely replay the same
delivery key and bytes, while changed-payload conflicts fail terminally.
Twilio HTTP 5xx is terminal ambiguity because retrying could duplicate an SMS.
Curl proxy-resolution, host-resolution, and TCP-connect exits 5, 6, and 7 are
safe pre-provider retries for either provider; timeout and neighboring generic
process failures remain conservative. If Brain cannot construct its provider
worker, it records one definitely-not-accepted transport attempt through this
same bounded retry policy instead of immediately reclaiming the row in a loop.
The same tick reconciles malformed or missing semantic-response authority before
claiming the oldest due row. It records only content-free phase and stable
terminal-reason counts (`retry-exhausted`, `permanent-rejection`,
`ambiguous-acknowledgement`, `idempotency-window-expired`, and
`no-safe-fallback`). `brain receiver status` reads and themes those counts
without creating a database or running a migration. BR-18 retains only the
narrow legacy job-socket lifetime representation; it no longer owns notice,
control, delivery-status, or outbox reconciliation work.

### Durable receiver model foundation

Every workspace state database has the active durable receiver job and conversation
model. It preserves immutable accepted inputs,
provider delivery IDs, explicit queued through terminal lifecycle states,
bounded retry metadata, and expiring claim ownership across Brain or machine
restarts. Claims never pop a job from storage. If a consumer crashes, another
owner can replace an eligible proved-pre-spawn or delivery lease. Successful
process spawn is the no-auto-replay boundary. Brain retains the spawned
controller and exact registration as one local capability while it
reauthorizes, resolves the exact `launched` commit, reserves a tab, and performs
the final owner proof. A store error keeps that same capability for a later
tick; an exact visible commit distinguishes the case where the write succeeded.
Proven owner or allocation loss enters shutdown-first cleanup, and neither the
registration nor native-session lock is released until shutdown succeeds.
After shutdown, every successfully spawned recovery uses one atomic exact
cleanup protocol. It either establishes or redrives the same terminal
job/token/instance/session tuple, then releases the registration and lock only
through exact acknowledgement. This remains true after the 30-second writer
claim expires, when a launch write was visible despite a caller error, and when
deadline reconciliation wins first. Incomplete or changed registration,
lifecycle, scope, source, or PID evidence fails closed and keeps the lock.
The same rule applies after launch. If another Brain process terminalizes an
active recovery between this App's reconciliation and its claim renewal or
observation commit, the active controller becomes cleanup-pending authority for
the exact durable tuple. Shutdown, artifact, acknowledgement, and store
failures retain that controller for retry. A live recorded PID, a missing tab,
or a wrong cleanup effect cannot release the session; only the matching exact
acknowledgement can do so. After the owning App exits, a reopened App may finish
the tuple only after proving that exact PID dead, and unrelated registrations
remain unchanged.
Deadline reconciliation can also derive the exact Resume tuple before a launch
observation exists, so repeated shutdown failure never opens the session to a
competing TUI. An orderly shell exit leaves that tuple for dead-PID restart
cleanup instead of releasing it directly.
Schema v10 separately
persists the two-minute launch, 90-second acceptance, five-minute progress,
recovery, and immutable 30-minute accepted-work deadlines. The renewable
30-second claim remains only a writer fence. Exact claims, launch commits,
acceptance, and every exact progress pulse establish or renew the corresponding
lifecycle limits from trusted local authorization time, never from a
future-skewed producer timestamp.
The pure policy can classify a durable snapshot as wait, safe pre-acceptance
requeue, one same-session recovery, terminal failure, or an incomplete legacy
completion state. Recovery count and attempt kind remain separate from the
existing three-attempt launch retry budget. One immediate store transaction
evaluates the oldest blocking snapshot and applies at most one transition. It
safely requeues an unaccepted timeout, persists one accepted stall as an
ownerless due recovery, or terminalizes exhaustion, absolute expiry, missing
native evidence, and legacy completion ambiguity with a pending
unavailable-notice intent. The accepted transition preserves lifetime identity
and first facts, resets only the superseded attempt cursor, binds the exact
observed native session, records an exact cleanup-pending instance/session
fence, and spends the recovery budget before any claim. Recovery discovery
after restart can claim only that persisted attempt, only after exact cleanup
acknowledgement, and only when it is the workspace's globally oldest claimable
or blocking row. An ownerless recovery still terminalizes at its recovery or
absolute deadline after reopen. Every terminalized live run with an exact
instance/session pair preserves and redrives those opaque cleanup identifiers
across later ticks and restarts. Exact acknowledgement then releases
the retained registration and native-session lock from either the due recovery
or any cleanup-fenced terminal failed state, but only while its
registration/session attribution still matches the exact job and durable
conversation. The pending notice becomes one semantic durable outbox row
independently from cleanup progress. Local cleanup remembers
successful shutdown and artifact removal so a later tick can finish the
remaining step before later FIFO work launches. Pre-spawn owner-store failures
also remain distinct from proven owner loss. They clean only the exact
ephemeral controller and registration, retain cleanup authority if either
operation fails, and then retry the same persisted recovery claim without
replaying inbound content or selecting a different frontend. Ordinary retry recording rejects recovery
attempts; planning, registration, spawn, or shutdown failure for an exact live
recovery owner instead terminalizes with pending-notice intent. Controller
cleanup, native-history inspection, and recovery launch are separate App
effects; every notice uses the generic outbox. An accepted recovery creates a fresh receiver instance
through `AgentController` for the frontend stored in the conversation, validates
and claims only that frontend's exact native session, and sends a bounded
resume-only instruction. It never selects Fresh, replays the inbound message,
stages the original attachments, parses `/new`, or uses the current TUI's
default frontend. Planning, registration, spawn, and shutdown failures use
typed recovery reasons without spending an ordinary launch retry. Automatic
startup repair preserves this authority when
one cleanup identifier is missing only if one fully attributed durable
registration and session row match the job plus the conversation's frontend,
user, channel, and native binding.

An accepted ordinary Fresh fallback can retain a real prior conversation
binding while its launch placeholder rotates to a different lifecycle-native
session. Reconciliation treats that exact three-ID lineage as unsafe for
same-session recovery, preserves the prior binding, and terminalizes with the
fixed unavailable notice. Its cleanup fence names only the observed Fresh run;
exact acknowledgement removes that run's lock and placeholder registration,
including a null or prior-valued registration actual ID, while leaving the prior
session untouched. Restart cleanup repeats the same attribution proof and still
requires the recorded process to be dead. If the exact proof is absent or has
changed, Brain releases nothing and stores no newly derived cleanup tuple, so
the terminal job remains fail-closed without holding FIFO behind an impossible
acknowledgement.

An ordinary claimed run may complete freshness and a durable `/new` control
while receiver intent is disabled, but neither an ordinary nor recovery claim
may start a new process until intent is enabled again. The claim remains
renewed so re-enable continues the same FIFO work.

Terminal notice intent uses the schema-v12 durable response lane. Same-version
repair and enabled-tick reconciliation convert a legacy BR-16 pending bit into
one immutable `unavailable-notice` envelope and clear the obsolete local lease.
A storage failure preserves the pending source transaction for an exact later
retry; only a deterministic authorization or render failure clears the bit.
`/new` and `/restart` acknowledgements, plus one notice for every dropped job,
commit atomically with their source-job and conversation changes. Final answers,
notices, and acknowledgements share exact claims, provider-result policy, retry
deadlines, and restart reconciliation. A provider outage cannot block later
agent work, and concurrent TUIs cannot claim the same delivery tuple.

Fallback selection is pure and fail-closed. It considers only alternate
destinations frozen at acceptance, excludes the failed provider and attempted
recipients, and chooses at most one short safe notice. Current single-channel
jobs freeze no alternate authority, so they stop rather than consulting later
user or machine configuration. Every terminal outcome persists either
`fallback-planned` or `no-safe-fallback`. A planned notice is inserted in the
same exact transaction as the source terminal transition, remains durable
across restart and concurrent result delivery, and cannot create another
fallback notice if its own provider attempt fails. Once that notice is
acknowledged, its durable relation to the terminal source keeps the job done
through reopen, repeated repair, downgrade, and later re-upgrade without a
resend.

A logical conversation belongs to one workspace, portable user, channel, and
channel-specific key. SMS uses one stable key for that tuple. Email reuses only
verified provider thread lineage; Resend currently supplies no stable verified
thread key, so each new delivery creates an uncertain fresh conversation while
a provider retry resolves to the original durable row. Subject text is never a
merge key. Each conversation stores
Brain-owned markdown and, when available, its frontend plus opaque native
session ID. Brain may resume that ID only with the same frontend. Selecting a
different frontend starts a fresh native session from the portable transcript.
Every raw receiver launch prompt is capped at 47 KiB and begins with the same
trusted task-capture policy. Adaptive POSIX quoting keeps a maximally escaped
prompt plus 12 KiB reserved for the configured command, trusted policy, and
options within a 96 KiB shell argument ceiling. `AgentController` also checks
the completed command and rejects it before spawn if it exceeds that ceiling,
leaving margin below 128 KiB single-argument platforms. A resumed ordinary session
relies on its native history and receives only the bounded current authenticated
message and local attachment paths. A fresh ordinary launch also includes the portable
transcript, reserving
up to 8 KiB for its newest context and up to 16 KiB for the current message
before local attachment paths can use the remaining space. Final prompt planning
runs after attachment staging, retains a deterministic prefix of complete
JSON-encoded path records, and never truncates inside a usable path. Oversized
sections carry explicit transcript, message, or attachment omission markers, so
long workspace paths cannot silently erase the conversation or current turn.

BR-12 established the storage contract, BR-13 moved authenticated provider
admission onto it, and BR-14 made the isolated TUI coordinator its sole
execution consumer. Provider success follows durable insert or deduplication;
the shared process still requires a live enabled lease and owns no execution.
BR-15 added acceptance/progress evidence. BR-16 added one accepted same-session
recovery and pending unavailable-notice intent. BR-17 now migrates that intent
and every receiver-owned reply to the durable delivery outbox; BR-18 retains
final representation cleanup.

### Steering the receiver from SMS or email

Two messages are read as instructions to brain rather than questions for the
agent. A message counts as a command only when the command is the **entire**
message, ignoring surrounding whitespace and case — `/NeW` and `/new ` both
work, while "what does /new do?" is an ordinary question and gets an ordinary
answer. Both commands acknowledge their sender, because a sender with no screen
cannot otherwise tell an obeyed command from an ignored one.

| Command | Effect |
| --- | --- |
| `/new` | Waits its durable FIFO turn, retires only that logical conversation, and starts a fresh boundary. Later messages in that same conversation use a new agent session; other conversations, channels, and workspaces are unchanged. The literal command never reaches an agent. |
| `/restart` | Durably drops everything unclaimed **ahead of it** and tells each of those senders their message could not be processed and should be resent. The active answer, later arrivals, and unrelated conversations remain intact. The literal command never reaches an agent. |

The two are applied at deliberately different moments. `/restart` takes effect
from durable queued state without waiting for a free panel: a sender restarts
*because* they are stuck behind something, so making the command queue behind
the backlog it clears would leave it useless. It does not interrupt the answer
already in flight, and anything sent *after* the restart is kept, since that is
new work nobody asked to abandon. `/new`, by contrast, waits its turn and is
applied only between messages, because its whole value is *where* the boundary
falls. Running it mid-turn would cut the conversation in the wrong place and
kill an answer someone is already waiting on. Its durable transaction preserves
the retired transcript and binding while moving only later unclaimed work in
that exact logical conversation onto the fresh boundary.

Every outbound SMS is shaped for a medium with no renderer. Brain converts the
agent's answer to plain text before it is posted: headings, emphasis,
strikethrough, code spans and fences, link and image syntax, blockquotes,
horizontal rules, and markdown escapes lose their markers, bullets of any
flavour or depth flatten to one `- ` line each, table rows become
comma-separated cells, and repeated blank lines collapse. A link keeps its label
and, when the target is actually reachable from a phone, the bare URL after it
(`the invoice (https://…)`); a local or relative target is dropped as noise.
Code-span and fenced content is delivered verbatim, and anything that only
looks like markup (`2 * 3`, `snake_case_name`, an unclosed `**`) is left exactly
as written. Stripping happens *before* the 480-character measurement, so
markers never spend the SMS budget or trigger a needless "ask for a longer
reply" truncation. The same response shown in the TUI keeps its markdown.

Email goes the other way. An email client renders HTML, so every reply is sent
as both parts: the agent's markdown verbatim as the plain-text part, and that
same markdown **rendered** as the HTML part — headings, lists, tables, task
lists, block quotes, code spans and fences, strikethrough, and clickable links,
inside brain's styled card. Markdown is parsed by `pulldown-cmark`, not by
anything brain hand-rolls. Two things are neutralized on the way out, because a
reply quotes message text a stranger wrote: raw HTML in the answer is shown as
the text it looks like rather than passed through as markup, and a link
destination that is not `https:`, `http:`, or `mailto:` is dropped.

When cloud sync is configured, receiver dispatch also applies the two-hour
freshness gate described above. The HTTP acknowledgement remains immediate,
but stale local state is pulled before the queued message reaches the selected
Claude, Codex, or OpenCode frontend. Task-capture language is passed with an
explicit instruction to create the task rather than perform it, unless the
sender asks for immediate execution. After a verified response completes,
Brain starts a push before delivering the reply.

Receiver enablement is persistent workspace intent, separate from process and
lease availability. `brain receiver start`, `brain receiver stop`, startup
`--with-receiver`, and both command palettes share one transition and an exact
canonical-name plus UUID registry transaction. A live shared process is
notified by workspace UUID after persistence and reloads the authoritative
record; no process is elected for a short-lived mutation. `brain receiver
status` reports Receiver, TUI, Server, and Accepting independently. An enabled
workspace without a live TUI therefore reports `Accepting no`.
Persistence is the successful mutation boundary. If the optional live refresh
fails, Brain keeps the committed CLI or palette state and shows a warning.
Status requires both persisted intent and an enabled exact live lease before it
reports `Accepting yes`; a live but disabled lease reports `TUI live` and
`Accepting no`.

Bare `brain receiver` answers the machine-wide version of that question. It
opens with the machine's own block — the public base URL and the one webhook URL
per channel built from it, plus the rule that explains why one URL can serve
every workspace — because no URL names a workspace any more. Then, with no `-w`,
one block per registered workspace in registry order; `-w` narrows those blocks
to the one workspace that was asked about while the URL block stays machine-wide.
Each workspace block reports the same four intent-and-liveness rows as
`receiver status`, then the configured email and phone, which are exactly what
route an inbound message to it. An unconfigured value reads `not set`; with no
public base URL the machine block prints no webhook row and names the
machine-wide variable to set, because there is no origin to build one from and
the listing spans every workspace. A workspace whose record
cannot be read reports `unavailable` with its repair command instead of taking
the whole listing down, and a shared process that cannot be asked reports
`live state unavailable` rather than claiming the server is stopped. Selected
`receiver status` also prints content-free counts for `answer-ready`,
`delivering`, `retrying`, `ambiguous`, `failed`, and `done`, followed by stable
terminal-reason counts for retry exhaustion, permanent rejection, ambiguous
acknowledgement, idempotency-window expiry, and no safe fallback. These rows never
include sender, recipient, answer, envelope, transcript, provider response, or
credential material. The listing prints the receiver's own published addresses; it never prints a
provider credential.

`brain receiver email` and `brain receiver phone` print just that address, on
stdout with no styling, so a script or an agent can consume it directly. Since
routing is by destination, printing the address answers both "where do people
write to this workspace" and "what makes a message arrive here". An unset
address names the variable and both ways to set it, exactly as `receiver url`
does for a missing public base URL, and exits non-zero.

`brain server status`, `brain receiver status -w <workspace>`,
`brain receiver url -w <workspace>`, bare `brain receiver`, and
`brain receiver email` / `brain receiver phone` are literal
read-only probes. They do not write a diagnostic run log, migrate or repair
configuration, create a users transaction lock, refresh installed skills,
write the skill render stamp, or elect/start/churn the shared process. Receiver
status uses one generation-bound control response for both server and exact
workspace facts. A live control failure is reported, and neither status request
expires leases or changes server lifecycle state.

Receiver status preserves its four lifecycle rows, then adds receiver, SMS,
and email health for the selected workspace. Persisted receiver intent is the
feature switch. When it is off, both channels are off even if stale provider
fields remain. When intent is on, a channel becomes active from any provider
field or an inbound-enabled portable `users.json` mapping; a malformed or
partial active channel is incomplete. A ready channel requires its complete
machine-local provider fields, the machine-global public URL, and at least one
matching portable inbound mapping. Status never prints provider secrets or sender addresses.

If all TUIs are closed, the final unregister stops the server immediately, so
an inbound text reaches no Brain process and receives no Brain response. If
some other workspace TUI remains live but the target workspace is disabled,
closed, expired, at its 64-row queued capacity, or unable to commit state, the
sender receives one unavailable response and no new row is created. Otherwise
the accepted message is retained in the target workspace DB before success. A
crashed final TUI leaves only its renewable lease; expiry after TTL stops the
process, but already committed jobs survive for the later durable consumer.

- `brain server status` reports process reachability and the live TUI lease
  count only, or says that no process is running. It neither elects a starter
  nor exposes workspace message data and needs no selected workspace.
- `brain server logs` prints the machine-wide infrastructure log, or says that
  no log exists. It is likewise read-only and workspace-independent.
- `brain server run --generation <uuid> --port <p>` is the hidden blocking
  loop used only by an elected starter. A matching token must already own
`election.lock`; direct or tokenless startup is rejected.

An elected child has a two-second bootstrap window to receive its first live
TUI registration. If the electing TUI disappears before registering, the child
exits and cleans its PID, control socket, and election token instead of becoming
an unowned background daemon.

The habits command may explicitly elect a background server and attach a
browser-only lease; `brain habits kill` removes that lease only when no TUI is
live. The lifecycle layer exposes `connect_or_elect` for long-lived TUI startup
and `connect_or_elect_background` for this habits path. The TUI
registers before launching its agent through one bounded handshake. If the
selected generation exits before registration, the handshake re-enters election
and registers against the winner; authoritative identity rejection is not
retried. Registration compares the normalized TUI-resolved root to the reopened
registry, derives the UUID-local job socket from machine paths, and verifies the
live singleton plus a deadline-bounded listener probe before accepting the
lease. A retry after an accepted response is lost succeeds only when generation,
lease, workspace identity, PID, and derived endpoint are unchanged; competing
registrations remain rejected. The TUI then heartbeats
once per second, re-elects and re-registers after a missing or stale generation,
and unregisters before its workspace job socket is removed. Two workspaces may
hold leases concurrently; the last orderly exit shuts the shared process down.

`brain receiver setup` walks through the selected channel's provider
credentials, the machine's one public base URL (machine-global, so setting it
for one workspace sets it for all), and a portable-user address mapping. It lists
existing people from the selected workspace's `users.json`; the user may choose
one or create a new ID and display name. SMS requires a phone and email requires
an email, each with an explicit inbound-allowed state. Complete noninteractive
flags provide the same channel, provider, user, address, and allowed-state
values. Secrets are hidden while typing and stored only in the selected
machine-registry record. Blank keeps an existing provider value. `/clear` is
accepted as input, but setup rejects the resulting blank when that provider
value is required by a selected channel. Guided and headless setup share the
same HTTPS-origin, channel-requirement, sender-normalization, and redacted-error
validation before writing.
Supplying `--channels` without `--user-id`, as in
`brain receiver setup -w family --channels sms`, keeps the selected channel
and interactively collects the missing portable-user mapping.
The setup output shows the exact `/sms` and/or `/email` URL to enter in the
provider portal, and says that both are machine-wide: the number and address it
just saved are what will route a message to this workspace. Setup and `receiver set` notify only the selected live
lease to reload; they never start or restart a process. Provider,
portable-user, and hook writes form a rollback-bounded setup transaction: any
later failure restores the selected pre-state, leaves peer workspace state
alone, and suppresses reload. The
shared process prefers port `8787` and serves receiver routes only while at
least one TUI lease keeps that process alive. A selected workspace accepts
receiver work only while its own lease is live and enabled; another live
workspace cannot make that route accept. Email body and attachment content is
retrieved through Resend's Receiving APIs; HTML-only messages and attachment
download URLs are preserved for the agent.

`brain habits` elects a background process when none exists, then prints the
local
`/local/<exact-live-lease>/w/<selected-ingress>/habits` URL
before handing it to the system browser. A TUI reuses that process by replacing
the browser-only lease, and **inherits that lease's capability**, so a habits
page already open in the browser keeps loading and marking habits done after
the TUI starts instead of dying with a not-found local route. The inherited
capability lives exactly as long as the TUI lease that took it over.
`brain habits kill` refuses while a TUI is open.

`brain habits revive <fuzzy name>` (alias `brain habits fix`) repairs a **lapsed
habit** — a recurring habit whose every occurrence is `done` with none pending,
so it silently dropped off the agenda (the usual cause is an instance marked
done outside `brain tasks complete`, which skips the spawn-next step). The
query is matched case-insensitively against habit names, tolerating word
reordering: each whitespace token must appear in the name, so
`brain habits fix send team status update` resolves "Send status update to
team". A single match is revived immediately; multiple matches are listed for
interactive selection over `/dev/tty`; no match is reported plainly. Reviving
appends one fresh `not_started` occurrence anchored to the latest scheduled
instance, using the same anchor-to-due recurrence math as completion (the first
occurrence strictly after today). A habit that still has a pending occurrence
is reported healthy and left untouched — the command never creates duplicates.

`brain habits skip <id|fuzzy>` opts out of a habit **for today**, with
cadence-aware semantics (the native port of the old `skip_habit.py`, so nothing
in `/todo` needs to reason it out in-context). It accepts an id (`H43`, `43`) or
a fuzzy name; a task id is rejected with a pointer to `brain tasks complete`.
The rule:

- **Daily habit** (`recur_interval == 1` and `recur_unit == days`) → today's
  occurrence is "handled": it's marked `done` (recording `completed_date=today`)
  and tomorrow's occurrence is spawned, exactly like completion. A daily habit
  is back tomorrow regardless, so "skip today" *is* "today is handled".
- **Non-daily habit** (weekly, monthly, every-N-days, …) → not marked done; its
  `due_date` is deferred to tomorrow (today + 1). It simply reappears tomorrow.
- **`--until YYYY-MM-DD`** (either cadence) → `due_date` is deferred to that day,
  never marked done. Must be strictly after today.

Like every other native mutation, skip keeps the day's agenda in step: a daily
skip is a completion, so the habit leaves the plan and joins Completed today;
any other cadence is a one-day defer, which only drops it from the plan. See
[Keeping the day's agenda in sync](#keeping-the-days-agenda-in-sync).

`brain habits complete-managed-triage <daily|weekly>` completes Brain's managed
triage occurrence **without needing its id**: it marks today's occurrence done
and spawns the next, keyed on the stable `system_key` (which survives
recurrence and renames) rather than an id that changes every cycle. Completing
the same row by id through `brain tasks complete` is equally allowed — only
removing, reviving, and skipping a managed row are refused while
`enable_triage_habits` is on. This is the
deterministic mutation the daily-triage nudge's **Skip** button runs in-process,
now exposed as a first-class CLI so an agent (or you) can do it non-interactively.
It **respects `enable_triage_habits`**: with the feature off it is a pure no-op
that mutates nothing (the day is acknowledged handled), so a fork with the
feature disabled behaves identically.

## Keeping the day's agenda in sync

The day's agenda is a markdown file — `<agenda_markdown_dir>/<YYYY-MM-DD>.md`,
`/tmp` by default — that whoever builds the agenda writes, and that the user
reads (and prints) all day. It is a **snapshot of the CSVs**, not a second
source of truth: the CSVs decide, the agenda reports. So the moment a mutation
lands in a CSV, the snapshot is out of date.

Brain closes that gap itself. **Every native path that writes `tasks.csv` or
`habits.csv` runs the sync**, in-process, right after the write:

| Mutation | What it tells the agenda |
| --- | --- |
| `brain tasks complete`, the tasks view's mark-complete | Completed: drop it from the plan, hand a chunked task's slot to its next chunk |
| The habits browser page's done button | Completed |
| `brain habits skip` on a **daily** habit | Completed (a daily skip marks done and respawns) |
| `brain habits skip` on any other cadence, or with `--until` | Deferred: drop it from the plan |
| `brain habits complete-managed-triage`, the daily-triage nudge's **Skip** | Completed |
| `brain tasks set --status done` | Completed |
| `brain tasks set --due <another day>` | Deferred |
| `brain tasks set` on any other field | Refresh the snapshots only — renaming a task is not a statement that it left today |
| `brain tasks add` | Refresh the snapshots only (a new habit due today joins Today's habits) |
| `brain habits revive` | Refresh the snapshots only, which is provably a no-op: the occurrence it spawns is dated strictly after today |

`brain tasks sync-agenda` exposes the same code to every other mutator, and the
bundled `/todo` mutator scripts (`defer_task.py`, `defer_habit.py`,
`touch_task.py`, `backlog_task.py`) shell out to it rather than carrying a
second copy. Nobody has to remember to rewrite the agenda by hand, and a
freehand rewrite is exactly how sections used to get dropped.

One sync does three things, and nothing else:

1. **The actionable sections lose the mutated id.** The MIT callout, `Suggested
   order`, and `Cut order` drop every line naming it, and the numbered lists are
   resequenced from 1. Sections are matched by heading prefix (`## ❗`,
   `## Suggested order`, `## Cut order`), so the author's exact wording is free.
   On a completed **chunked** task whose next chunk is unfinished and not yet on
   the agenda, that chunk inherits the vacated callout line and suggested-order
   slot (keeping its number and time), so exactly one actionable chunk stays
   visible.
2. **The snapshot sections are re-derived from the CSVs.** `🔁 Today's habits`
   (pending habits with no due date or one on/before today, then today's
   completions, ordered by ideal time, duration, then name) and
   `✅ Completed today` (habits, then tasks, completed on that date) are rebuilt
   from scratch every run — which is what catches a habit flipped to done
   outside this process. A section with nothing to show is removed; a missing
   one is inserted before any appended optional content, never after it.
3. **The printable is regenerated, but only if one already exists.** A CSV
   mutation is not a request for a fresh printout, so no PDF on disk means no
   PDF. When `<agenda_dir>/agenda-<date>.pdf` does exist it must stay current,
   so it is re-rendered from a comment-stripped copy of the markdown (the
   renderer has no concept of HTML, so an unstripped comment would print as
   visible text; the marker stays in the source, where the appendix baker greps
   for it).

**Everything else is reassembled byte-for-byte** — the title, `**Load:**`,
`**Bottom line:**`, and any section Brain has never heard of. The sync is
idempotent: re-running it on an already-accurate agenda writes nothing and
regenerates nothing.

It is also strictly best-effort. The CSVs are already written and committed by
the time the sync runs, so a missing agenda, an unreadable file, or a broken PDF
renderer is logged and swallowed — never a reason to fail a completion that
already succeeded. No agenda for the date at all is the ordinary case, and it
is a clean no-op.

### Prerequisite: `markdown-to-pdf`

Only TUI and task routes cross this prerequisite gate. Workspace, config, env,
sync, persona, skills, server, receiver, habits, check, reindex, version,
help, and the internal server-run route dispatch before it. A gated route fails
fast with a red `❌` error if the `markdown-to-pdf` command cannot be resolved
(it is needed for "Create PDF"). Its path is auto-discovered on first run and
stored as `markdown_to_pdf_path` **in brain env**
(`~/.config/brain/env.json`, not `config.json`); see
[config.md](config.md).

## The fuzzy picker

The search panel of the persistent shell (the brain-directory main view). It
collects entries under the bucket roots and renders a filterable, grouped
list.

- **Typing** filters live. Matching is substring-based: every
  whitespace-separated word in the query must appear as a contiguous run
  in the entry. Slug separators (`-`, `_`, `.`) are stripped before
  matching, so `afloat` finds `ann-afloat` and `annafloat` and
  `ann afloat` all hit. See [data-model.md](data-model.md).
- **Grouping**: matches are grouped under section headers (Projects →
  Areas → Resources → Archive) showing a per-section count. Headers occupy
  a row but aren't selectable.
- **Highlights**: matched characters are colored; the highlight offsets
  are mapped back from the normalized string to the original display
  bytes so they line up exactly.
- **Selecting**:
  - `Enter` → **open directly, in place**. Text-like files open in a **new
    iTerm2 tab** (cd'd to the file's directory, then `$VISUAL`/`$EDITOR`/`nvim`);
    everything else hands off to the system `open`; a directory reveals in
    Finder. The brain shell stays open in all cases.
  - `Ctrl-Enter` → **reveal in Finder**. Files resolve to their parent
    directory.
- **Command palette**: `Ctrl-p` opens the top-level command palette (the
  menu) as a modal overlay for any action `brain` can run; `Esc` closes it
  back to the picker. The tasks-view palette includes **Sync brain now**, which
  kicks off a nonblocking background `brain sync`, plus **Show sync status**,
  which reports whether a sync is active. It also includes a
  **Show sync status**, which opens a modal tailing the running sync's live
  transcript — the same lines `brain sync` prints to your terminal — and says
  "No sync is running right now." when there is none (an earlier run's log is
  deliberately not offered, since it looks like an answer to "what is happening
  now?"). `j`/`k` scroll, `G` returns to following the tail, `Esc` closes. It
  also includes a
  **Disable daily triage alert** / **Enable daily triage alert** toggle (the
  label swaps to name the action it will take) — the runtime counterpart to the
  portable `enable_daily_triage_check` config variable, which it **writes** as
  well as flipping live, so the choice survives a restart and reaches the
  workspace's other machines. Because a TUI can stay open across day
  rollovers, this flips the daily-triage nudge on or off for the current session
  without a persistent config change; enabling it re-checks immediately, so an
  outstanding triage surfaces the modal at once. The tasks-view palette also
  carries one **Run \<label\>** row per skill session the workspace offers (see
  "Skill sessions" above) and, while any is running, **Show main brain session**
  plus a **Show \<title\> session** row per open tab. None of these has a direct
  shortcut.
- **Cancel**: `Esc` / `Ctrl-c` exits with no action.

See [keybindings.md](keybindings.md) for the complete key table including
movement, paging, and query editing (`Ctrl-u`, `Ctrl-w`, Backspace).

## Create a PDF from markdown

When the highlighted entry is a markdown file (a name ending in `.md`), two
extra affordances appear:

- The **command palette** grows a leading **"Create PDF for '<file>'"** row
  (`[^G]`), default-selected so `Ctrl-p` → `Enter` runs it immediately.
- **`Ctrl-G`** fires it directly, first raising a small green **yes/no
  confirmation modal** ("Would you like to create a PDF for '<file>'?").
  `Enter`/`y` confirm, `n`/`Esc` cancel, `←`/`→`/`Tab` toggle the buttons.
  (The palette row skips the modal — choosing it is already a confirmation.)

Either way, `brain` converts the markdown to a **colocated, same-name PDF**
(`plan.md` → `plan.pdf`, in the same directory) using the user's
`markdown-to-pdf` tool, then opens the result with the system `open`. In the
persistent shell this happens **in place** — the brain shell stays up, and the
search list **auto-refreshes** so the new PDF appears immediately. Any
pre-existing PDF at that exact path is replaced so the output name always
matches the source. Only `.md` is offered (the converter's sole input); the
label elides a long filename to fit the palette. See
[integrations.md](integrations.md) for the tool handoff.

## Delete an entry

When any entry is highlighted (a file **or** a directory), two affordances
appear:

- The **command palette** grows a trailing **"Delete '<file>'"** row (`[^D]`).
  It is deliberately last and never default-selected, so a stray `Ctrl-p` →
  `Enter` can't delete.
- **`Ctrl-D`** opens a small **red** yes/no confirmation modal ("Delete
  '<file>'? It moves to the Trash."). Because deleting is destructive the
  modal **defaults to No**: `Enter` cancels, and deleting takes a deliberate
  `y` (or a toggle to Yes first). `n`/`Esc` cancel. The palette's "Delete" row
  routes through the same modal (the guard is never skipped).

Confirming moves the file or directory to the **Trash** (via Finder, so it's a
recoverable, user-style delete — a `Put Back` away, not an `rm`), then the
search list refreshes so the entry disappears. This happens **in place** and
the shell stays up. The label elides a long filename with the same threshold
as the "Create PDF" row.

## Open tasks

`Ctrl-t` (or palette item "Open tasks") switches to the **tasks main view**,
the ratatui task-management surface backed by the selected root's
`tasks/{tasks,habits}.csv`
(today / MIT / past-due / week / habits views, agenda, triage). It is the
startup default and `brain tasks` opens straight onto it. The tasks view is
in-process — a main view of the same shell, not a separate binary — so the
switch is instant and the brain panel stays open beside it. See
[integrations.md](integrations.md) for the tasks-view shell-outs
(agenda / habits).

## Help and version

`brain --help` / `brain -h` print the clap-generated usage (with the
long-form command descriptions and the TUI key summary). `brain --version`,
`brain -v`, and `brain version` all print the same crate version line:
`brain <semver>`. Version output exits before the startup prerequisite gate.

The crate version in `Cargo.toml` is the single source of truth. Every committed
change bumps it according to SemVer: before v1, additive user-visible features
bump the minor version and compatible fixes/internal changes bump the patch
version. The user will explicitly decide when `brain` is ready for `1.0.0`.

### Workspace setup on first use

`brain -w family` on a machine that has never had `~/family` sets it up instead
of reporting what is missing. `workspace::initialize::initialize_workspace_directory`
runs before readiness for every ordinary command and is idempotent:

1. **The root directory.** Created when missing. If its *parent* is missing too,
   Brain refuses instead: an unmounted volume looks exactly like a missing root,
   and quietly creating an empty workspace over one would read as data loss.
2. **The portable manifest.** Written from the workspace UUID the registry
   record already carries, so joining a machine needs no
   `brain workspace repair --manifest`. Never written over one that already
   exists, so a manifest that just arrived over sync stays authoritative.
3. **The configured sync**, when there is one. The direction is decided once:
   - **This machine has never synced this workspace** → a full **both-ways**
     establish. Content created before sync was configured has never been
     uploaded, and a pull-only run would strand it locally forever.
   - **Already established, but the root is empty** → **pull**; the remote is
     the source of truth.
   - **Otherwise** → nothing extra. The ordinary startup pull and the
     change-triggered push own the steady state, so this costs two filesystem
     checks per command once a workspace is established.
4. **PARA and the task tables**, when the root is still empty afterwards:
   `projects/`, `areas/`, `resources/`, `archive/`, `tasks/`, `tasks.csv` and
   `habits.csv` with their headers, both ID counters at `1`, and the two lookup
   CSVs. Existing files are never overwritten, and an explicit
   `enable_triage_habits: false` in portable config is honored rather than
   reset. If sync is configured, the freshly seeded workspace is then pushed.

Two command families opt out of everything past step 1. A **sync** command
(`brain sync`, `sync status`, `check`) owns the network for its own run, and a
**registry-management** command (`workspace rename`/`alias`/`default`/`list`/`migrate`)
is not a request to use a workspace, so neither writes portable config as a side
effect.

### What `brain sync` reports

Sync output names the step, **what it found**, and **what it decided** — a step
name alone leaves a long pause ambiguous between "working" and "nothing to do":

```
Probing the remote workspace identity…
  found: remote belongs to this workspace (dfbc1768-…) → proceeding
Starting rclone sync; live file progress follows…
  found: no files differed between this machine and the remote
Merging task and habit CSVs by row id…
  found: no task or habit rows differed
```

A run that moved files reports the counts (`found: 3 file(s) transferred`); a
failed one names the error count and the verdict (`→ the run is not clean`); a
phase that is skipped says why (`decision: skipping the task/habit merge — the
file sync aborted, so its result cannot be trusted`). The same lines are what the
palette's **Show sync status** modal tails.

### Which workspace a command acts on

In precedence order:

1. **`-w` / `--workspace`** — always wins.
2. **`BRAIN_WORKSPACE`** — every process Brain launches (agent panels, lifecycle
   hooks, `reindex` children) receives it, so anything run from inside a
   workspace's session acts on that workspace with no flag. This is what makes
   the bundled skills correct: they call `brain …` without a selector, and inside
   a `family` panel those calls reach `family`. It is inherited by subshells, so
   a subagent in its own shell gets the same answer.
3. **The current directory** — Brain walks up from where you are, the way git
   finds its repository, and selects the workspace whose registered root contains
   it. `cd ~/family && brain sync` syncs `family`; so does running it from
   `~/family/projects/work__thing/notes`. Roots and the working directory are
   compared after resolving symlinks.
4. **The machine default** — a person typing `brain` from somewhere that is not
   inside any workspace.

`BRAIN_WORKSPACE_ID` still validates the outcome: if the resolved workspace's
UUID disagrees with the launching one, the command fails instead of acting on the
wrong brain.

The launching workspace deliberately outranks the current directory. An agent
panel opened for `family` stays on `family` even while it reads files under
another root — otherwise a `cd` inside a session would silently retarget every
command it ran afterwards, and would disagree with the `BRAIN_WORKSPACE_ID` the
panel was launched with.

### Strict workspace selection for Brain's own children

Every child process Brain spawns for its own work carries
`BRAIN_REQUIRE_WORKSPACE=1`, and a process that sees it **refuses to run without
an explicit `-w`/`--workspace`**. A code path that builds a `brain …` command and
forgets the selector then fails loudly instead of quietly operating on whichever
workspace happens to be the default — which, in a two-workspace setup, silently
syncs or mutates the wrong brain.

It applies only to Brain-spawned children. Typing `brain sync` yourself still
uses the default workspace, which is the whole point of having one. The agent
panel is covered by a stronger mechanism already: it receives `BRAIN_WORKSPACE_ID`,
so an agent-issued command that resolves a different workspace fails on identity
rather than on a missing flag.

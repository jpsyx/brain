# Keybindings

The merged `brain` shell has **three main views** (tasks, brain-directory
search, and logs) and one app-level **brain panel** (the selected
agent PTY: Claude by default, Codex with `--codex` / `-cx`, OpenCode with
`--open-code` / `-oc`, or pi with `--pi` / `-pi`). The selected frontend owns
its PTY input translation.
See [glossary.md](glossary.md) for the vocabulary. Startup: the **tasks
view** is showing, the **brain panel is open** (on the right) but unfocused, so
`j`/`k` work immediately.

## The shortcut-parity invariant

**Every keyboard shortcut has a command-palette row.** ("Hotkey" and "shortcut"
mean the same thing here.) A key that performs an action always runs the same
`Command` its palette row runs (the event loop calls `App::execute_command` or
`App::execute_global_action` rather than reimplementing anything inline), so the
two can never drift apart, and a user whose terminal swallows a chord (see
*Kitty keyboard protocol* below) can always reach the same action from
`Ctrl+P`.

The table in `src/tasks/shortcuts/` records which command each binding runs,
and a guard test fails the build if the palette does not list it. The only
bindings exempt are the ones that are not commands at all: cursor movement
(`j`/`k`, `d`/`u`, `PgDn`/`PgUp`, `g`/`G`), panel scrolling (`Alt+U`/`Alt+D`),
`Esc` dismissing an error banner, and `Ctrl+P` itself. A second guard test pins
that exempt list.

The converse does not hold: plenty of commands are palette-only. The palette is
the parent set.

Keys are resolved in this precedence (see `tui/event_loop/run.rs`):

1. **Unconditional quit:** `Ctrl+Q` exits even while a modal is open.
2. **Modal overlays:** a captive modal (help, sync log, the command palette,
   the task or entry target picker, confirmation, brain input, session rename
   picker or input, link picker, or assignee filter) consumes the key before any
   panel accelerator.
3. **App-level accelerators:** Esc dismisses a pending error banner before
   reaching a panel. `Ctrl+X` closes the selected user session,
   `Ctrl+N` starts a new conversation, `Alt+S` opens help, `Alt+H/L` moves focus,
   `Alt+[/]` and occupied `Alt+digit` slots switch tabs, and `Alt+U/D` scrolls.
   Main-view focus enables `Ctrl+H/L`, `Ctrl+T/B`, and contextual palette,
   brain-input, and agenda actions. `Ctrl+M` selects Main from either panel.
4. **The brain panel:** when focused, keys forward to the selected agent as bytes.
5. **The active main view:** tasks, brain-search, or logs handlers consume the key.

## App-level (work in either main view)

| Key | Action | Notes |
| --- | --- | --- |
| `Esc` | Dismiss a pending error banner | Applies in Tasks, Brain Search, Logs, and the brain panel. An active modal keeps its own Esc behavior. Without a pending error, normal panel behavior applies. |
| `Ctrl+L` / `Ctrl+H` | Cycle the main view right / left | Cycles tasks, brain search, and logs. Main-panel focus only, so the brain panel keeps Claude's `Ctrl+H` (backspace) etc. when it has focus |
| `Ctrl+T` | Jump to the **tasks** view | Main-panel focus only |
| `Ctrl+B` | Jump to the **brain-directory** view | Main-panel focus only |
| `Alt+H` / `Alt+L` | Focus the **left** / **right** panel | Spatial: follows the layout when the brain panel is swapped sides. `Alt+H` from the brain panel is the reliable way back to the main view |
| `Alt+U` / `Alt+D` | Scroll the focused panel a half-page up / down | Brain panel scrolls its scrollback; the main view pages. Fires while the selected agent has focus or a filter is active. Also accepts macOS Option-produced equivalents when richer keyboard reporting surfaces those instead of Alt-modified ASCII |
| `Ctrl+M` | Select and focus the Main brain session | Launches Main if unavailable, using its saved conversation when the frontend can reopen it. Needs the kitty protocol to stay distinct from Enter |
| `Ctrl+N` | Start a new conversation in the selected user session | Runs the selected adapter's semantic new-session action (`/new` plus its native submit) in a live Manual or Skill tab. It preserves a Manual tab's identity and title. Receiver tabs ignore this action. |
| `Alt+[` / `Alt+]` | Cycle the brain-panel tab (previous / next): **main** session ↔ each open additional tab | Manual, skill-session, and receiver-run tabs share one stable insertion order, which also drives `Alt+1` / `Alt+<n>` slots and the rendered tab strip. At most one receiver-run tab is live in a workspace process, and its insertion never invokes selection. The reliable bracket switch resolves as Alt-modified brackets or the macOS Option smart-quote glyphs. On macOS layouts that send an Option-produced glyph (`¡`, `™`, `£`, …), that glyph addresses the same slot; an unoccupied slot selects nothing and the glyph remains ordinary panel input. From either panel |
| `Ctrl+X` | Close the selected Additional manual or skill-session tab | From either panel. Main is permanent and ignores this action; receiver runs are removed only by their lifecycle owner. Closing an Additional manual session removes its saved mapping, while shell shutdown preserves mappings. |
| `Alt+S` | Open the keyboard-shortcuts help modal | Replaces the old bare `?`; bound to `Alt+S` so a literal `?` still types into the brain-search filter. Distinct Meta sequence on every terminal |
| `Ctrl+P` | Open the global command palette | Main-panel focus only, from any main view; in the brain panel it stays the agent's own binding |
| `Ctrl+Q` | Unconditional quit | Intercepted before modals/panels; quits even from the brain panel or a modal. `0x11`, no kitty protocol needed. The palette's **Quit brain** row leaves through the same door |

**Panel focus vs. view switching** are two different axes: `Alt+H/L` move
*focus* between the main view and the brain panel; `Ctrl+H/L` change *which
main view* is shown. Both read as "left/right" but mean different things.

Main's saved resume candidate must retain its frontend evidence: Claude checks
its transcript and live claims, Codex checks its exact on-disk rollout, and
OpenCode checks the selected workspace's live root session. Missing evidence
starts fresh under the same Manual identity, without selecting another recent
conversation.

There is **one** command palette, shared by every main view. It offers
**Start new brain session**, **Rename session**, **Close a brain session**,
**Start a new conversation in this session**, **Show main brain session**,
configured skill starts, and stable Show rows for open manual and skill tabs.
Start immediately creates an Additional manual session named
`<workspace>-<three random lowercase letters>`. Receiver tabs have no Show row.
No per-session row adds a direct shortcut annotation.

The Rename and Close pickers accept Up/Down and `Ctrl+K`/`Ctrl+J` navigation.
Both place actionable rows first and disabled rows last. Rename advances only
from an Additional manual row; Main, Skill, and Receiver rows are marked `[not
renameable]`. Close acts only on an Additional manual or Skill row; Main and
Receiver rows are marked `[not closeable]`. Disabled rows are dimmed and
crossed out, with their bracketed annotation in yellow. The rename input is
prefilled and accepts printable single-line text, Backspace, and `Ctrl+U`
(clear). Enter validates and saves the trimmed title; invalid input remains
visible with an inline error. Esc and `Ctrl+C` cancel these modals. Panel
accelerators are captive while a picker or rename input is open; `Ctrl+Q`
retains unconditional quit.
Manual launch, persistence, and close errors remain in the shared banner until
Esc acknowledges them. Other keystrokes and view or focus changes preserve the
message, including input received before its first render.

## Tasks view

The tasks view is a vim-style modal list with tabbed sub-views. It is the
startup default.

### Normal mode

| Key | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Next / previous task (accepts a count prefix, e.g. `3j`) |
| `d` / `u` | Half-page down / up |
| `PgDn` / `PgUp` | Full page down / up |
| `g` / `G` (`Home` / `End`) | First / last task |
| `→` / `←` | Expand / collapse the highlighted entry's notes |
| `l` | Toggle the selected entry's notes (preview ↔ full markdown) |
| `Tab` / `Shift+Tab` | Cycle **sub-view** forward / backward (today → mit → past_due → week → habits → backlog → all) |
| `t` `m` `p` `w` `h` `b` `a` | Jump to sub-view (today/mit/past_due/week/habits/backlog/all). Bare letters only. `h` collapses notes instead when the highlighted entry's notes are expanded |
| `/` | Enter search mode (live fuzzy filter) |
| `r` | Reload `tasks.csv` + `habits.csv` |
| `Enter` | Open the task actions modal for the selected entry |
| `Ctrl+D` | Mark the selected task complete (confirm modal). `0x04`, no kitty protocol needed |
| `Ctrl+Backspace` | Remove the selected task (confirm modal) — tasks only. Bare Backspace is a no-op |
| `Ctrl+O` | Open the selected entry's links (Linear issue + notes URLs) |
| `Ctrl+Enter` | Open the task actions modal (mainly for search mode) |
| `Ctrl+P` | Open the global command palette (every command brain has) |
| `Ctrl+Shift+M` | Brain-input modal seeded with the selected task as context |
| `Ctrl+A` | Open today's agenda (offers to generate it when missing) |
| `q` / `Esc` | Quit (Esc clears an active filter first). Also `Ctrl+C` |

`Ctrl+P`, `Ctrl+A`, and `Ctrl+Shift+M` fire from any main view while the main
panel has focus; the task actions modal and the bare-letter keys are tasks-view
only. `Ctrl+Shift+M` with nothing highlighted asks which task, exactly as its
palette row does. Opening today's habits page in the browser is the palette's
**"Open habits in browser"** row (served by the bundled brain server; the old
`Ctrl+H` binding became the cycle-view accelerator).

### Search mode (`/` active)

| Key | Action |
| --- | --- |
| printable char | Append to the query |
| `Backspace` | Delete (empty query → exit search) |
| `Ctrl+U` | Clear the query (empty query → exit search) |
| `Enter` | Exit search mode, keep the filter |
| `Esc` / `Ctrl+C` | Cancel the filter, exit search (does not quit) |
| `Ctrl+Enter` | Open the task actions modal for the selected entry |
| other `Ctrl+<key>` | Falls through to the normal-mode shortcut |

## Brain-directory (search) view

An always-filtering fuzzy picker over the selected workspace's projects,
areas, resources, and archive directories. Every printable key edits the
query.

| Key | Action |
| --- | --- |
| printable char | Append to the query and refilter |
| `Backspace` / `Ctrl+U` / `Ctrl+W` | Delete char / clear / delete word |
| `↑` / `↓` (`Ctrl+K` / `Ctrl+J`) | Move selection |
| `PgUp` / `PgDn` / `Home` / `End` | Page / jump |
| `Enter` | Open the highlighted entry in place (text → editor tab, blob → system open, dir → Finder) — shell stays up |
| `Ctrl+Enter` | Reveal the entry in Finder |
| `Ctrl+G` | Create a PDF from the highlighted `.md` file (green confirm modal) |
| `Ctrl+D` | Delete the highlighted entry (red confirm modal → Trash) |
| `Ctrl+R` | Refresh the list (re-walk the current scope, keep the query) |
| `Ctrl+P` | Open the global command palette (the same one every view opens) |
| `Esc` / `Ctrl+C` | Quit the shell |

`Tab` / `Shift+Tab` do nothing here (no sub-views). Each direct key above
resolves the highlighted path and hands it to the same `EntryCommand` the
palette row runs, so the two can't drift; the PDF / delete confirm overlays are
captive while open.

The palette's rescope rows cover every bucket: **Search capture** (the
user-managed in-basket), **Search projects**, **Search areas**, **Search
resources**, and **Search archive**, plus **Global search**, which restores all
five. None of them has a direct keystroke, so none carries a gray `[…]` hint.
Choosing one brings the brain-directory view forward, since the rescope is
otherwise invisible.

## Modals

Shared across the app; a captive modal consumes all input.

- **Help** (`Alt+S`, or the palette's **Show keyboard shortcuts** row) — the
  `shortcuts::ALL` reference, grouped by surface (Navigation, Views, Task
  actions, Brain directory, Brain panel, Search, Global). Sized to the terminal
  (a ~10% gutter, 70–112 columns) so descriptions don't wrap to ribbons.
  `j/k`, `PgUp/PgDn`, `g`, `?`/`q`/`Esc` close.
- **Sync log** (palette: **Show sync status**) — tails the running sync's live
  transcript, re-read every frame. Says "No sync is running right now." when
  there is none; an earlier run's log is not shown. `j`/`k` scroll, `g` jumps to
  the start, `G` returns to following the tail, `PgUp`/`PgDn` page, `q`/`Esc`
  close. Captive over every modal except help.
- **Command palette** (`Ctrl+P`, any main view) — filterable; numbered rows;
  `Enter` runs, `Esc` closes. It takes at most three fifths of the terminal's
  height and scrolls inside that, keeping the selection visible and what's
  behind it as context; the footer shows your position (`31/55`) whenever the
  list is longer than the box.
  It is the **parent set of every command**: task commands, brain-directory
  commands, view switches, session commands, the workspace toggles
  (**Enable/Disable receiver**, **Disable/Enable daily triage alert**),
  **Sync brain now**, **Show sync status**, **Show keyboard shortcuts**, and
  **Quit brain** are all listed no matter which view is showing. The set never
  changes with app state — only each row's wording does:
  - A command whose target is already highlighted names it: *Mark T123 as
    complete*, *Delete 'plan.md'*.
  - A command whose target is missing reads generically: *Mark a task as
    complete*, *Delete a file or directory*. Running it opens a picker for the
    target first (see below), then does exactly what the named row would.
  The only rows a workspace can lack are the assignment controls (**Add task**,
  **Filter by assignee**, **Reassign**), which a single-member workspace has no
  use for. That is a workspace capability, not app state.
- **Task target picker** (a task command with nothing highlighted) — a
  filterable list of every task (and habit, when the command accepts one),
  matched on ID or name. `Enter` runs the pending command on the chosen row,
  `Esc` abandons it.
- **Entry target picker** (a brain-directory command with nothing highlighted)
  — the same fuzzy picker the brain-directory view uses, boxed as a modal. Type
  to filter, `↑`/`↓` or `Ctrl+K`/`Ctrl+J` to move, `Enter` to run the pending
  command on the chosen path, `Esc` to abandon. A chosen entry that can't
  satisfy the command (a directory for *Copy a file's path*, a non-markdown
  file for *Create a PDF*) is refused with a message rather than acted on.
- **Task actions** (`Enter` on a task): the palette's task rows, already bound
  to that entry, so their labels drop the ID the title carries. Unlike the
  global palette it hides commands the entry can't take — a habit shows no
  defer or remove row — because it is already committed to one row.
- **Confirm** — Yes/No (the daily-triage nudge adds **Skip**, which marks today's
  Morning Triage habit done deterministically in-process — no agent). `y`/`n`/`s`/`Esc`,
  `←`/`→`/`Tab` move, `Enter` resolves.

The receiver palette row has no direct key. Its dynamic label mutates the same
persistent selected-workspace `receiver_enabled` value as `brain receiver
start`, `brain receiver stop`, and startup `--with-receiver`. It never starts or
stops the shared process; live TUI leases own that lifetime. The CLI-only
receiver and server status probes are read-only and therefore have no palette
mutation counterpart. Their control inspection is also immutable: status never
expires a lease or advances shared-process state.

- **Confirm** — Yes/No (triage adds Skip). `y`/`n`/`Esc`, `←`/`→`/`Tab` move, `Enter` resolves.
- **Brain-input** (`Ctrl+Shift+M`) — compose a seeded message. `Alt+Enter` newline, `Enter` send.
- **Link picker** (`Ctrl+O`, ≥ 2 links) — numbered; digit opens, `Enter` opens highlighted.
- **Assignee filter** (shared-workspace palette row): numbered portable
  members plus **All assignees**; digit or `Enter` applies, `Esc` closes. The
  active member appears below the task-view heading, and task-view `Esc`
  clears it before quitting.

## Kitty keyboard protocol

`run_tui` requests `DISAMBIGUATE_ESCAPE_CODES`. With it, `Ctrl+M`/`Ctrl+Enter`
are distinct from `Enter`, `Ctrl+H`/`Ctrl+L` from Backspace/Tab-family, and
`Ctrl+Shift+M` reports its Shift. Without it (legacy Terminal.app):

- `Ctrl+M` / `Ctrl+Enter` / `Ctrl+Shift+M` collapse to `Enter` → use the palette.
- `Ctrl+H` collapses to Backspace, so **cycle-view-left is unavailable**; use `Ctrl+L` (right) or the palette. `Ctrl+T` / `Ctrl+B` / `Ctrl+L` have no aliasing.
- `Alt+S`, `Alt+U`, `Alt+D`, `Ctrl+D`, `Ctrl+A`, `Ctrl+Q`, `Ctrl+X`, `Ctrl+N` are all reliable (Meta sequence, macOS Option-glyph fallback, or control bytes with no aliasing).

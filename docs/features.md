# Features

`brain` is the central terminal dispatch for the second brain and task
system. Everything below is reachable from the persistent shell (bare
`brain`), as a subcommand, or from a command palette.

## The merged shell: two main views + one brain panel

Bare `brain` (and `brain tasks …`) opens a persistent shell with **two main
views** and one app-level **brain panel** (see [glossary.md](glossary.md)):

- **Tasks view** — the startup default. The task-management surface over
  `~/brain/tasks/{tasks,habits}.csv`: tabbed sub-views (`today`, `mit`,
  `past_due`, `week`, `habits`, `backlog`, `all`; `Tab`/`Shift+Tab` cycle
  them), vim navigation, notes expand/render, mark-complete / remove / defer /
  open-links, agenda (`Ctrl+A`), and the daily-triage startup nudge. The full
  key list is in [keybindings.md](keybindings.md).
- **Brain-directory (search) view** — the fuzzy search across projects,
  areas, resources, and archive (the picker described later in this doc);
  formerly what bare `brain` opened.
- **Brain panel** — a live, interactive `claude` session in an embedded PTY,
  open at startup and shared by both main views. It does not belong to either
  view: switching views leaves it open; closing it (`Ctrl+X`, or claude
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
starts focused on the search panel so a query can be typed immediately; the
brain panel is still spawned at startup with the resumed conversation ready
one `Alt+`-switch away. `Alt+U` / `Alt+D` scroll the focused panel a half-page
up / down
(the brain panel by half its visible rows, the search panel by a page of its
match list) — a keyboard-only alternative to the wheel that fires even while
Claude has focus or the filter is being typed.

**Closing vs quitting.** Exiting claude (e.g. `Ctrl-C` to end the turn, then
`Ctrl-C` again to exit claude) **closes the brain panel** — search goes
full-width and the shell keeps running. It does *not* quit `brain`. To
**re-open** the panel, run **Message brain** (`Ctrl-M`, or the palette row —
which only appears while the panel is closed); it resumes your latest
session. To **quit `brain`** entirely, press `Esc` or `Ctrl-c` from the
**search** panel.

**Start a new session.** `Ctrl+N` starts a fresh Claude conversation in the
brain panel: it sends `/new` + Enter to the running `claude` for you. It fires
from either panel while the panel is open (no need to focus the brain panel
first). When the panel is closed, `Ctrl+N` keeps its search meaning (move the
selection down).

**Session resume.** On startup the brain panel resumes your **most recent
Claude session** — the continuous conversation picks up where it left off.
If you type `/new` (or `/clear`) inside claude — or press `Ctrl+N` — that
fresh session becomes the one brain resumes next time. Running `brain` in a second terminal does
**not** reuse the session a live `brain` already holds (no tangled threads):
it resumes the next-most-recent free session, or starts fresh. If the
session it would resume has no transcript yet (you opened brain last time but
never sent a message), it can't be resumed — brain starts a fresh chat and
says so in the status line. See [integrations.md](integrations.md) and
[data-model.md](data-model.md) for the lock-and-recency model.

**Swap the layout.** The palette's "Move brain panel to the left/right"
command flips which side the brain panel sits on; the choice is persisted
(`~/.cache/brain/state.db`), so it sticks across runs. `Alt+H`/`Alt+L`
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
   closed**. In the one-shot picker it instead cd's into `~/brain` and opens
   claude.
2. **Open tasks** `[^T]` — run the `tasks` TUI (task management, agenda, triage).
3. **Go to brain root directory** `[^B]` — cd into the configured root.
4. **Search projects** — rescope search to `~/brain/projects`.
5. **Search areas** — rescope search to `~/brain/areas`.
6. **Search resources** — rescope search to `~/brain/resources`.
7. **Search archive** — rescope search to `~/brain/archive` (retired material).
8. **Global search** — search across projects, areas, resources, and archive.
9. **Move brain panel to the left / right** — swap the layout (label names
   the direction the panel would move; persistent shell only).
- **Delete '<file>'** `[^D]` — move the highlighted entry (file **or**
  directory) to the Trash. **Shown whenever something is highlighted**, and it
  **trails** the list (never default-selected) so a stray `Enter` on open
  can't delete; the label is elided with the same threshold as the PDF row.
  See "Delete an entry" below.

In the persistent shell, the search rows rescope the left panel **in
place**; "Go to root" and "Open tasks" deliberately leave brain (handing a
plan to the parent shell). The keystrokes (`Ctrl-g`, `Ctrl-d`, `Ctrl-m`,
`Ctrl-t`, `Ctrl-b`) also fire directly without opening the palette first —
`Ctrl-g` and `Ctrl-d` open a confirmation modal (see below); the rest run
their action. `Ctrl-R` **refreshes** the search list (re-walks the current
scope, keeping the query); the list also auto-refreshes after a PDF is
created or an entry is deleted, so the change shows without a manual refresh.

The palette is a filterable text input: typing narrows the rows. Each
row's matchable text includes its 1-based number, so you can type a digit
(`6`), any word from the label (`message`), or several words (`search
projects`) and the list narrows to the hits. Navigate the filtered list
with ↑/↓, `Ctrl-k`/`Ctrl-j`, or `Ctrl-p`/`Ctrl-n`;
`Backspace`/`Ctrl-u`/`Ctrl-w` edit the query; `Enter` runs the highlighted
row (and exits the picker into that action); `Esc` or `Ctrl-c` closes the
overlay and returns to the underlying search (no cd, no claude, no error).

## Subcommands

| Command | Aliases | No-arg behavior | With-query behavior |
| --- | --- | --- | --- |
| `brain pr [q]` | `project`, `projects` | cd into `~/brain/projects` | picker scoped to projects, seeded with `q` |
| `brain ar [q]` | `area`, `areas` | cd into `~/brain/areas` | picker scoped to areas |
| `brain re [q]` | `resource`, `resources` | cd into `~/brain/resources` | picker scoped to resources |
| `brain s [q]` | `search` | picker across all buckets (empty) | picker across all buckets, seeded with `q` |
| `brain cd` | — | cd into the brain root | — |
| `brain msg <prompt>` | — | cd into `~/brain`, open claude empty | cd + open claude with `<prompt>` |
| `brain tasks` | — | run the `tasks` TUI | — |
| `brain config` | — | list config (see below) | `get`/`set` subcommands |

Bare positional input with no matching subcommand becomes a global search:
`brain rust borrow` is equivalent to `brain s rust borrow`.

### `brain config`

Reads and writes brain's persistent config (`~/.config/brain/config.json`):

- `brain config list` (or bare `brain config`) — aligned table of every
  variable, its effective value, and its description.
- `brain config get <name>` — the effective value of one variable.
- `brain config set <name>=<value>` — set and persist a variable (unknown
  names rejected).

`config` runs before the `markdown-to-pdf` prerequisite gate, so it always
works even when that tool is missing. See [config.md](config.md) for the schema
and the prerequisite/auto-discovery rules.

### Prerequisite: `markdown-to-pdf`

Every command except `brain config` fails fast with a red `❌` error if the
`markdown-to-pdf` command can't be resolved (it's needed for "Create PDF").
Its path is auto-discovered on first run and stored as `markdown_to_pdf_path`;
see [config.md](config.md).

## The fuzzy picker

The search panel of the persistent shell, and the whole screen for the
one-shot search subcommands. It collects entries under the relevant bucket
roots and renders a filterable, grouped list. The matching, navigation, and
rendering are identical in both; what differs is what `Enter` does.

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
  - `Enter` → **open directly**.
    - In the **persistent shell**: text-like files open in a **new iTerm2
      tab** (cd'd to the file's directory, then `$VISUAL`/`$EDITOR`/`nvim`);
      everything else hands off to the system `open`; a directory reveals in
      Finder. The brain shell stays open in all cases.
    - In a **one-shot picker**: text files become an `edit=` directive (run
      in the current terminal), blobs an `open=`, dirs a Finder reveal; the
      parent shell `cd`s into the file's directory.
  - `Ctrl-Enter` → **reveal in Finder**. Files resolve to their parent
    directory.
- **Command palette**: `Ctrl-p` opens the top-level command palette (the
  menu) as a modal overlay for any action `brain` can run; `Esc` closes it
  back to the picker.
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
search list refreshes so the entry disappears. In the persistent shell this
happens **in place** and the shell stays up; in a one-shot picker the entry is
dropped from the list and the picker stays open. The label elides a long
filename with the same threshold as the "Create PDF" row.

## Open tasks

`brain tasks`, palette item 2, or `Ctrl-t`, hands off to the user's `tasks` CLI: the
ratatui task-management TUI backed by `~/brain/tasks/{tasks,habits}.csv`
(today / MIT / past-due / week / habits views, agenda, triage). `brain`
doesn't reimplement any of that; it emits a `tasks=1` directive and the
zsh wrapper runs the `tasks` function (sourced alongside `brain` in the
user's rc). This is what makes `brain` a true *dispatch*: task work lives
in `tasks`, knowledge work lives in `brain`, and `brain` is the one door
into both. See [integrations.md](integrations.md).

## Help and version

`brain --help` / `brain -h` print the clap-generated usage (with the
long-form command descriptions and the TUI key summary). `brain --version`
prints the crate version. Both flow through the wrapper as passthrough
output.

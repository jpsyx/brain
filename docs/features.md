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
   closed**.
2. **Open tasks** `[^T]` — switch to the tasks main view (task management,
   agenda, triage), in-process.
3. **Search projects** — rescope search to `~/brain/projects`.
4. **Search areas** — rescope search to `~/brain/areas`.
5. **Search resources** — rescope search to `~/brain/resources`.
6. **Search archive** — rescope search to `~/brain/archive` (retired material).
7. **Global search** — search across projects, areas, resources, and archive.
8. **Move brain panel to the left / right** — swap the layout (label names
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

There are only two subcommands; everything else lives inside the persistent
shell. Bare `brain` (no subcommand) opens the shell on the tasks view.

| Command | Behavior |
| --- | --- |
| `brain` | Open the persistent shell on the tasks view (the startup default). |
| `brain tasks [view/date/query] [flags]` | Open the shell on the given tasks view/selector/search. |
| `brain tasks --no-tui …` | Print the resolved task list as plain text (no TUI). |
| `brain tasks complete <id>` | Mark a task complete (`mark_done.py`), no TUI. |
| `brain tasks doctor` | Run the state/hook health check, no TUI. |
| `brain tasks search <q>` | Open the shell with an initial search over all tasks. |
| `brain config [list\|get\|set]` | Read or change persistent config (see below). |
| `brain personalize [show\|get\|set\|edit]` | Read or change your personalization (identity + tag styles). Bare `brain personalize` runs first-run onboarding if nothing is set, else shows current values (see below). |
| `brain skills sync [--root <dir>]` | Render + install the bundled skills into the agent registry (`~/.agents/skills`) and fan out to the frontends (Claude, Codex, OpenCode, Cursor). `--root` installs under a sandbox dir instead of your real setup (see below). |

`brain tasks mark <id> [as] done` is rewritten to `brain tasks complete <id>`
before clap parses it.

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

### `brain personalize`

Reads and writes your **personalization** — content *about you*, stored beside
the config store at `~/.config/brain/personalization.json` (just another brain
config, under `$HOME` rather than inside the brain root).

- `brain personalize` (bare) — first-run onboarding if nothing is set yet
  (a short, skippable prompt for your name, role, and who you work for),
  otherwise prints your current values (same as `show`).
- `brain personalize show` — a stable, keyed block (`name:` / `role:` /
  `works_for:`) that brain skills read at runtime to learn who they're
  assisting.
- `brain personalize get <field>` — one field (`name`, `role`, `works_for`).
- `brain personalize set <field>=<value>` — set and persist a field.
- `brain personalize edit` — open the raw JSON in `$EDITOR` (this is how you
  edit **tag styles**).

**Tag styles.** The task renderer's tag → emoji+label mapping is personalization.
The binary ships only a tiny universal default set (`mit`, `personal`, `work`);
any other tag renders as its raw name until you add a style under `tag_styles`
in the personalization JSON. So the public binary carries no personal taxonomy.

**Every mutation re-renders skills.** `personalize set`/`edit`, first-run
onboarding, and `config set` all trigger a skill re-render so the installed
skills never drift from your values. (The render pipeline itself lands in a
later sub-project; the trigger is wired now.)

Like `config`, `personalize` runs before the `markdown-to-pdf` prerequisite
gate, so it always works. See [config.md](config.md) and
[data-model.md](data-model.md) for the store layout and schema.

### `brain skills`

Manages the **bundled brain skills** — the skills that ship with brain and
install into the shared agent registry so they work in *any* Claude (or Codex,
OpenCode, Cursor) session, not just inside brain.

- `brain skills sync` — render each bundled skill and install it: write a built
  copy, link `~/.agents/skills/<name>` at it, then link each frontend's skills
  dir at the registry entry. Idempotent; re-run any time.
- `brain skills sync --root <dir>` — install everything under a **sandbox** dir
  instead of your real `~/.agents`/frontend dirs. Used for testing so a run
  never disturbs your live setup.

The skills are embedded in the binary, so a fresh clone needs no extra files.
Installing is also triggered automatically after a `config`/`personalize` change
when `skills_auto_sync` is `true` (default `false` while the pipeline is being
rolled out). Bundled today: `article-summarizer`, `triage`,
`brain-knowledge-capture`, `second-brain`, and `contacts` (more land as
sub-project B migrates them in). See [config.md](config.md) and the sub-project
B spec.

**Customizing skills without forking.** Two mechanisms, both stored with your
brain (synced, never committed to the repo):

- **Extensions** — personalize a *bundled* skill without a new skill. Put a
  `<root>/.config/extensions/<skill>.md` file with `[hook]` sections; the sync
  injects each hook's text at the skill's matching `<!-- brain:ext hook -->`
  marker in the **built copy** (the repo skill is never touched). Content with no
  matching marker is appended as a "Personal extensions" section, so nothing is
  lost. This is how, e.g., the bundled `triage` skill declares
  `triage:daily-open` / `triage:daily-linear` / `triage:weekly-inboxes` /
  `triage:weekly-linear` hooks so a personal extension can bolt an email pass,
  an issue-tracker reconcile, and a cloud in-basket onto the generic core.
- **Plugins** — whole skills you own, in `<root>/.config/plugins/<name>/`. The
  sync installs them alongside the bundled cores, into the same registry and
  frontends.

### Prerequisite: `markdown-to-pdf`

Every command except `brain config` fails fast with a red `❌` error if the
`markdown-to-pdf` command can't be resolved (it's needed for "Create PDF").
Its path is auto-discovered on first run and stored as `markdown_to_pdf_path`;
see [config.md](config.md).

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
search list refreshes so the entry disappears. This happens **in place** and
the shell stays up. The label elides a long filename with the same threshold
as the "Create PDF" row.

## Open tasks

`Ctrl-t` (or palette item "Open tasks") switches to the **tasks main view**,
the ratatui task-management surface backed by `~/brain/tasks/{tasks,habits}.csv`
(today / MIT / past-due / week / habits views, agenda, triage). It is the
startup default and `brain tasks` opens straight onto it. The tasks view is
in-process — a main view of the same shell, not a separate binary — so the
switch is instant and the brain panel stays open beside it. See
[integrations.md](integrations.md) for the tasks-view shell-outs
(`mark_done.py`, agenda / habits).

## Help and version

`brain --help` / `brain -h` print the clap-generated usage (with the
long-form command descriptions and the TUI key summary). `brain --version`
prints the crate version. Both are printed by clap straight to stdout.

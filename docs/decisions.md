# Decisions

The "why" behind `brain`'s non-obvious choices. Architecture is in
[architecture.md](architecture.md); this file is the rationale an agent
needs before second-guessing a design.

## Why tasks and brain were merged into one CLI

They went hand-in-hand and both already embedded the *same* kind of `claude`
brain panel anchored at `~/brain`, so running two separate shells (each with
its own panel, session DB, `SessionStart` hook, and env namespace) was
duplicative and meant the two panels couldn't share a conversation. Merging
gives one shell with one app-level brain panel shared across a **tasks view**
and a **brain-directory view**.

Decisions taken during the merge (see the conversation that produced it):

- **Tasks view is the startup default**, brain panel open but unfocused so
  `j`/`k` work immediately.
- **Two switching axes, deliberately distinct.** `Ctrl+H/L` (cycle) and
  `Ctrl+T`/`Ctrl+B` (jump) switch *which main view* shows; `Alt+H/L` move
  *panel focus*. View-switch chords are intercepted only when the main panel
  has focus, so the brain panel keeps Claude's readline chords (`Ctrl+H` =
  backspace, etc.) when focused.
- **`Alt+S` (not bare `?`) opens help**, so a literal `?` still types into the
  always-filtering brain-search view. It's a Meta sequence, reliable on every
  terminal.
- **Displaced bindings.** `Ctrl+B` (was brain's "go to root") is repurposed to
  the brain-view jump and go-to-root was **dropped** (redundant now the view
  is one keystroke away in-app). `Ctrl+H` (was tasks' "open habits page") is
  repurposed to cycle-left; opening the habits page moved to the palette.
- **One session namespace.** The separate `BRAIN_*` / `TASKS_*` namespaces and
  DBs existed only to stop two shells adopting each other's sessions; with one
  panel there is one hook, one DB (`brain_sessions`), one `BRAIN_*` namespace.
- **`tasks` utilities nest under `brain tasks …`** (`complete`, `doctor`,
  `search`, `--no-tui`); the old `tasks` binary/command and its `tasks=1` plan
  handoff are gone (cold-turkey, per the user).
- **The tasks tui was the merge base**, not brain's: it was the richer,
  default surface, so the merged `App` is the ported tasks `App` extended with
  a `MainView` axis and brain's `picker` embedded as the second view. brain's
  shared `session`/`state`/`pty_pane`/`plan` (near-identical ports) were kept
  as the single copy.

## Why `brain` is a "central dispatch", not a single-purpose tool

The user lives in the terminal and wants **one command** to reach
everything around `~/brain`: jump to a note, search across PARA buckets,
start a claude conversation rooted in the brain, or open the task TUI.
Rather than memorize `brain`, `tasks`, `agenda`, `cl`, etc. as separate
entry points, `brain` is the front door that routes to the right tool.
That's why bare `brain` drops straight into global search (the common
case) with the full **command palette** of actions one `Ctrl-p` away, and
why "Open tasks" exists as a first-class palette item even though the
actual work lives in a different binary. New top-level capabilities should
be added as a palette row + a subcommand, not as a separate command the
user has to discover.

## Why bare `brain` is global search, with the palette behind `Ctrl-p`

The single most common thing the user wants is to find a note, so bare
`brain` opens global search directly rather than making them pick "Global
search" out of a menu first. The menu didn't go away — it became the
**command palette**, reachable with `Ctrl-p` from any picker, so every
other action (cd to root, per-bucket search, message brain, open tasks)
is still one keystroke from the search box. `Ctrl-p` is the palette hotkey
(matching the `tasks` TUI), which is why it is no longer an up-motion
alias in the picker (up is `↑` / `Ctrl-k`).

## Why the palette is a modal overlay, not its own screen

The palette is drawn as a modal **overlay inside the picker's event loop**
(`menu::draw_modal` over the picker, `menu::MenuApp` driven by the picker's
`handle_key`), rather than a separate full-screen TUI the way it started.
The reason is `Esc`: a separate screen would have to *exit* on `Esc`,
dropping the user all the way back to the shell and losing the search they
were in. As an overlay, `Esc` just closes the box and the picker is still
right there underneath — the same back-out-of-a-modal behavior the `tasks`
TUI has. This is why `menu.rs` has no `run()`/event loop of its own; it
exposes pure state (`MenuApp`, `handle_key`) plus `draw_modal`, and the
picker owns the loop. A confirmed row leaves the picker via
`Outcome::Choice`, which `main` routes through the same `dispatch` the old
flow used.

## Why archive is browsable now (it wasn't before)

Archive (`~/brain/archive`) is retired PARA material, and originally `brain`
left it out of every search on purpose. In practice the user still needs to
dig things back out of the archive, so it's now a first-class bucket:
"Search archive" is its own palette row and the `Archive` bucket is part of
global search (and bare `brain`). It sorts **last** in every grouped result
so live Projects/Areas/Resources stay on top and retired material doesn't
crowd the common case. There's deliberately no `brain ar`-style subcommand
for it — the palette row and global search cover the need without adding a
fourth bucket verb to the CLI surface.

## Why the palette's top rows carry direct keystrokes

The three actions the user reaches for most — message brain, open tasks, go
to brain root — sit at the top of the palette and also fire directly from
any picker via `Ctrl-m` / `Ctrl-t` / `Ctrl-b`, so the common cases don't
even require opening the palette. The keystrokes are surfaced as dim `[…]`
hints on their palette rows (the `tasks` convention, via
`menu::shortcut_for`) so they're discoverable without cluttering the layout.
`Ctrl-m` shares Enter's byte (`0x0D`), so it depends on the kitty protocol
we already push to stay distinct; without the protocol it degrades to a
plain `Enter` (open the selection), the same safe fallback as `Ctrl-Enter`.

## Why `Enter` opens and `Ctrl-Enter` reveals (the swap)

Opening the file is the action the user wants most of the time, so it gets
the unmodified `Enter`; revealing the containing directory in Finder is
the rarer case and takes the `Ctrl-Enter` chord. A directory match has no
file to open, so `Enter` on one falls back to revealing the directory
(identical to `Ctrl-Enter`), which keeps `Enter` from ever being a no-op.

## Why the binary doesn't `cd`, call `cl`, or run `tasks` itself

These effects must happen in the **parent shell**:

- `cd` mutates the caller's working directory; a child process can't.
- `cl` is a zsh alias and `tasks` is a zsh function — neither is a binary
  on `PATH`, so the child can't exec them and have them behave like the
  user's configured versions.

So the binary prints a small **plan** (`cd=`, `claude=`, `tasks=`, …) to
stdout and the `brain` zsh wrapper executes it in the parent shell. This
keeps the Rust side pure and testable (it just decides *what* should
happen) and confines shell coupling to one readable wrapper. See
[integrations.md](integrations.md) for the protocol.

## Why the TUI renders to `/dev/tty`, not stdout

The wrapper captures the binary's **stdout** to read the plan. If the TUI
also drew to stdout, the captured plan would be full of escape codes and
frame data. Rendering to `/dev/tty` (opened directly) keeps stdout clean
for the plan while the interactive UI still reaches the real terminal.
crossterm's raw-mode toggles and event reader operate on the controlling
terminal regardless, so input is unaffected.

## Why we push the kitty protocol unconditionally (and avoid the probe)

Distinguishing `Ctrl-Enter` (reveal in Finder) from `Enter` (open file)
needs the terminal keyboard-enhancement protocol. We push
`DISAMBIGUATE_ESCAPE_CODES` on entry without checking support first:
unsupported terminals ignore the escape, and the matching pop is then a
no-op, so nothing is left in a bad state. We deliberately avoid
`supports_keyboard_enhancement()` because its `DA1 + CSI ? u` probe can
race teardown and leak bytes (`[?0u...[?...c`) into the parent shell on
slower terminals. The degradation is safe: without the protocol,
`Ctrl-Enter` just behaves like `Enter`.

## Why slug separators are stripped before fuzzy matching

Brain slugs look like `ann-afloat`, `2024_q3_review`, `rust.borrow`. With
nucleo's substring atoms, a query word like `afloat` wouldn't match
`ann-afloat` because the dash breaks the contiguous run. We normalize each
display string by dropping `-`, `_`, `.` and match against that, then map
the highlight indices back to the original bytes. Net effect: `afloat`,
`annafloat`, and `ann afloat` all find `ann-afloat`, and the highlight
still lands on the right characters. See [data-model.md](data-model.md).

## Why both `tasks.csv` work and brain notes route through `brain`

Task management is a big, separate domain with its own CSVs, recurrence
rules, and TUI (the `tasks` CLI). We did **not** reimplement it inside
`brain`; we hand off. `brain` owns discovery and routing; `tasks` owns
task state. The seam is the `tasks=1` directive. This keeps each codebase
small and lets the task TUI evolve independently while staying one
keystroke away.

## Why the pure/impure split (and the `lib.rs`)

Every decision worth testing is pulled into a pure function:
`parse_config_root`, `expand_tilde_with_home`, `is_textlike`,
`finder_target`, `handle_key`, the `App` matching/navigation, the render
helpers, and the `plan::*_to` writers. The thin shells that touch
`/dev/tty`, `$HOME`, the exe path, or `std::process::Command` stay
untested by design. `lib.rs` re-exports the modules so integration tests
can link them; the binary declares the same modules privately. This mirrors
the sibling `tasks` project so an agent moving between them finds the same
shape.

## Why bare `brain` is a persistent two-panel shell with an always-on claude

The user wanted `claude` rooted in `~/brain` to be *always present and
continuous* — not launched per question and torn down. So bare `brain` is no
longer a fire-once picker; it's a persistent shell with fuzzy search on one
side and a live `claude` PTY (the "brain panel") on the other, mirroring the
`tasks` TUI's embedded brain pane. The two coexist because finding a note and
thinking with claude are complementary, not modal. We start focused on the
brain panel so the resumed conversation is immediately typable; `Alt+H` /
`Alt+L` switch panels (spatial, so they follow a layout swap).

## Why claude exiting closes the panel instead of quitting the shell

Exiting claude (Ctrl-C, Ctrl-C) is a frequent gesture — you end a chat
without meaning to leave `brain`. So when the `claude` child dies the event
loop **closes the panel** (drops the PTY, search goes full-width) rather than
quitting; the closing Ctrl-C is forwarded to claude and never seen as a quit,
and the auto-close needs no extra keystroke. Quitting `brain` is a separate,
deliberate gesture: `Esc` / `Ctrl-c` from the **search** panel. Re-opening is
**Message brain** (`Ctrl-M` or the palette), which resumes your latest
session — so the panel is closeable and re-openable, not a one-shot.

Closing the panel **releases** the session lock (it's no longer being driven)
so the re-open goes through the same recency+claim path as startup — which is
also why "Message brain" appears in the palette only while the panel is
closed: there's nothing to open when it's already up (and `Alt+L` focuses it).

## Why opening a file spawns a new iTerm2 tab instead of replacing the shell

In the persistent shell the whole point is that the brain panel never goes
away. The old `edit=` directive ran `$EDITOR` *in the current terminal*,
which would tear down the TUI to edit a note. Instead the running TUI spawns
a **new iTerm2 tab** (`osascript`) with `cd <dir> && $EDITOR <file>` for text
files, and `open <file>` for everything else (which launches its own app).
Either way the brain shell stays up. iTerm2 is the user's terminal, so we
drive it directly; on any other terminal the editor path falls back to
`open`.

## Why SQLite (not a JSON file) for session + layout state

The state is written by *multiple* processes that can race: several `brain`
shells, plus the Claude SessionStart hook (a separate Python process) firing
on every session start/resume/`/clear`. A JSON file would need hand-rolled
locking to stay consistent; SQLite in WAL mode gives concurrent readers + a
single writer with no busy-storms for free, and the `tasks` sibling already
established the pattern. The cost is `rusqlite` (`bundled`), accepted for the
concurrency guarantee.

## Why the lock + recency resume model (the multi-terminal answer)

Two goals tension: *always resume your latest conversation*, but *never put
two terminals on the same thread* (which would interleave into a tangle).
The resolution: each running shell **locks** its session to its PID; on
startup a shell resumes the most-recently-active **free** session (or starts
fresh if none is free) and releases the lock on exit. One terminal always
resumes its last conversation; a second can't grab the one the first holds,
so it takes the next-free session or a fresh one. Crashes don't strand a
session — dead-PID locks are reaped (`kill -0`) on the next startup.

## Why a SessionStart hook (and deliberately not a Stop hook)

brain can choose a session id up front (`--session-id`), but if the user
types `/new` (or `/clear`) mid-run, Claude may rotate to an id brain never
saw — and that fresh conversation is the one they'd want to resume next
time. A **SessionStart** hook fires on every start/resume/clear/compact with
the live `session_id` (keyed to the shell via `BRAIN_INSTANCE_ID` /
`BRAIN_PID` env), so brain always learns the current id — robust whether or
not `/clear` rotates it. We do **not** add a `Stop` hook (the `tasks`
project's "last assistant message" mechanism): brain-panel sessions are
continuous conversations, not discrete runs, so there's no per-run
completion to capture.

## Why we verify a transcript exists before resuming a session

`claude --resume <id>` only works if a transcript `<id>.jsonl` exists in the
project dir — and Claude writes that file only once a message is exchanged.
So a brain session you open and close *without chatting* leaves a DB row
with no transcript; blindly `--resume`-ing it later produces "couldn't find
session with ID …". brain therefore checks the transcript exists on disk
before resuming and skips candidates that don't, falling back to the next
valid one (or a fresh chat). This is also why brain forces the PTY's cwd to
`<brain_root>` *and* prefixes the command with `cd <root>`: every session is
scoped to the same project dir, so the existence check and `--resume` always
look in the same place. When the fallback to a fresh chat is caused by a
missing transcript, we surface it in the status line rather than silently —
the user asked to know when their conversation didn't carry over.

## Why the brain panel uses bare `claude`, not the `cl` alias

The one-shot `msg` path honors the user's `cl` alias. The brain panel
instead spawns bare `claude` because it must control the `--resume` /
`--session-id` flag to drive the resume model — an alias injecting its own
flags would fight that. Same reasoning as the `tasks` queue.

## Why we disable alternate scroll (and motion) reporting for the mouse

`EnableMouseCapture` turns on more than we want. We immediately trim it with
a raw `\x1b[?1002l\x1b[?1003l\x1b[?1007l` (see
`tui::disable_mouse_motion_reporting`):

- `1002`/`1003` (button-drag + any-event **motion**) off so ⌘-hover / ⌘-click
  still reach iTerm2's native link / Semantic-History handler; we only need
  button + wheel events.
- `1007` (xterm **alternate scroll**) off because it is the subtle reason the
  wheel appeared dead in the brain shell. On the alternate screen, iTerm2
  (default) and xterm translate the wheel into **arrow keys** instead of mouse
  events. The brain shell opens **focused on the claude panel**, so those
  arrows were forwarded straight into `claude` and *neither* panel scrolled.
  Disabling alternate scroll forces real wheel mouse events, which
  `handle_mouse` routes to whichever panel the cursor is over, independent of
  which panel has keyboard focus. (The `tasks` sibling only seemed unaffected
  because it opens focused on its list panel, where stray arrow keys move the
  selection and look like scrolling.)

Both are best-effort DECRST writes: a terminal that doesn't speak them just
ignores the escape, so there's no teardown to undo.

### Update: the wheel is unreliable in practice, so we lean on `Alt+U`/`Alt+D`

The reasoning above assumed that after trimming to **button + wheel** (plain
`1000` reporting, motion off), iTerm2 would still deliver wheel events to the
app. In practice that assumption does **not** hold on the user's iTerm2: with
motion reporting off, the scroll wheel produces *no* mouse events at all, so
neither panel scrolls with the wheel. The two mouse concerns are effectively
in tension on this terminal:

- **Wheel to the app** appears to require `1002`/`1003` (motion) reporting to
  be *on* — plain `1000` alone does not emit wheel events here.
- **`1002`/`1003` off** is what the original decision chose so ⌘-hover /
  ⌘-click keep reaching iTerm2's native Semantic-History handler.

We deliberately did **not** re-enable motion reporting to chase the wheel:

1. We can't confirm (without an interactive terminal probe) that ⌘-click
   survives motion reporting on the user's exact iTerm2 version, and silently
   breaking ⌘-click would be worse than a dead wheel.
2. Any escape-sequence answer is terminal-specific and fragile; these shells
   should not depend on a particular terminal's wheel semantics.

Instead, **`Alt+U` / `Alt+D` are the supported way to scroll** (half-page up /
down of the focused panel; see [keybindings.md](keybindings.md)). They are
handled as ordinary key events, intercepted before forwarding to `claude`, so
they work in every terminal, in every panel, even while Claude has focus or
the search filter is being typed — with **zero** dependency on mouse
reporting. The `1007` (alternate-scroll) DECRST is still worth keeping: it is
cheap, harmless where unsupported, and keeps the wheel from turning into stray
arrow keys for terminals where the wheel *does* reach the app. The `tasks` and
`dif` siblings make the same call and document it inline in their
`keybindings.md` mouse sections (neither keeps a decisions log).

## Why "Create PDF" is `Ctrl-G` and a contextual, leading palette row

The action only makes sense for a markdown file, so both its palette row and
its shortcut are **gated on a `.md` selection** — the row is absent otherwise,
and `Ctrl-G` is a no-op. It's `Ctrl-G` ("generate"), *not* `Ctrl-M`: `Ctrl-M`
is already "Message brain" (and shares Enter's byte), and shadowing it on the
common case of a highlighted note would be worse than a free letter. The row
**leads** the palette (before "Message brain") when present so opening the
palette on a markdown file lands on it by default — the fast path is
`Ctrl-p` → `Enter`. The confirmation modal fires **only** from the `Ctrl-G`
shortcut, not the palette row: picking a row is already a deliberate
confirmation, whereas a bare keystroke is easy to hit by accident.

## Why we delete any existing PDF before converting

The user's `markdown-to-pdf` never overwrites in non-interactive mode — it
writes a `-vN` variant instead (a safety default for scripted callers). But
"Create PDF" promises a PDF named exactly like the source (`plan.md` →
`plan.pdf`), and we open that exact path afterward. So `create_pdf` removes any
pre-existing same-name PDF first, guaranteeing the converter writes the exact
name and that we open the file we just produced. This is a regenerate action on
a derived artifact, so replacing the previous output is the expected behavior,
not data loss.

## Why "Delete" trashes (not `rm`), defaults to No, and trails the palette

Delete is the one destructive action `brain` performs, so every choice around
it leans safe:

- **Trash, not `rm`.** `move_to_trash` asks Finder to `delete` the item, which
  lands it in the Trash exactly like a user dragging it there — recoverable via
  `Put Back`. A raw `rm` would make a fat-finger unrecoverable, which is the
  wrong default for a note system. We reuse the OS Trash rather than inventing
  our own "deleted" area (mirrors how "Create PDF" reuses the user's converter).
- **Red modal, default No.** Both the `Ctrl-D` shortcut *and* the palette row
  route through the confirmation modal (unlike "Create PDF", whose palette row
  skips it) — there is no non-confirmed path to a delete. The modal is red and
  defaults to **No**, so a stray `Enter` cancels; deleting takes a deliberate
  `y` or a toggle to Yes. (Contrast PDF, a constructive action, which is green
  and defaults to Yes.)
- **Trails the palette.** The "Delete '…'" row is appended **last**, never the
  default-selected top row, so `Ctrl-p` → `Enter` can't delete by reflex. The
  "Create PDF" row, being safe and the likely intent on a note, leads instead.
- **`Ctrl-D`, search-panel only.** Bound in the search panel's key handler, not
  intercepted globally, so it never sends EOF (`Ctrl-D`) to `claude` when the
  brain panel is focused. `Ctrl-R` (refresh) is scoped the same way to avoid
  claude's reverse-search.

## Why the search list auto-refreshes (and `Ctrl-R` exists)

The picker walks the tree once at open. Actions that change the tree from
inside `brain` — creating a PDF, deleting an entry — would otherwise leave a
stale list until the next scope switch. So those actions call `App::refresh`
(re-walk the current `scope`, keep the query via `reload_entries`), and
`Ctrl-R` exposes the same refresh manually for changes made outside `brain`
(a file added in another terminal, an editor save). Refresh keeps the query
where a scope switch (`set_entries`) clears it, because a refresh is "same
view, newer data" rather than "new view".

## Why `markdown-to-pdf` is a discovered, configurable command

`markdown-to-pdf` is often installed as a shell **function** (an autoloaded
wrapper), which a child process can't invoke, so the binary needs a concrete
executable to spawn. Rather than shell out to an interactive `zsh -ic` on every
conversion (slow, sources the whole rc, risks leaking output onto our
`/dev/tty`), `brain` stores the tool's path as a config variable
(`markdown_to_pdf_path`) and spawns it directly with `<file.md> --out`.

The path is not hardcoded (the repo is public). On first run it is
**auto-discovered** (PATH, then conventional bin dirs, then a one-shot login
shell that resolves an autoloaded function to the script it wraps) and
persisted. A missing or invalid path is a hard, fail-fast error pointing at
`brain config set markdown_to_pdf_path=…`. See `settings.rs`.

## Why `linear_workspace` is a slug, not a full URL

The Linear link config is the **workspace slug** (e.g. `acme`), not the whole
`https://linear.app/<slug>/issue/` prefix. The slug is the only part that
varies per user; brain owns the URL shape, so a user configures the minimum and
can't get the surrounding format wrong. `Config::linear_base_url` interpolates
it, and an empty slug simply omits Linear links.

## Why config lives in `~/.config/brain/`, not the repo

The store is machine-local and writable (auto-discovery persists into it), and
the repo is public — so shipping a real `config.json` in the checkout would
both leak machine-specific values and fight the auto-write. Config therefore
lives at `~/.config/brain/config.json` (XDG-respecting), managed by
`brain config`, and never tracked.

## Why `Ctrl-N` sends `/new` instead of being forwarded to claude

Starting a fresh conversation is a frequent gesture, and typing `/new` by hand
each time is friction. `Ctrl-N` is intercepted before the brain-panel key
forwarding (like `Alt+U`/`Alt+D`) and types `/new` into the PTY, so it works
from either panel without first focusing the brain panel. We only intercept it
**while the panel is open** — there's nothing to send to otherwise — which
conveniently leaves `Ctrl-N`'s search meaning (move-down) intact when the panel
is closed and search is full-width. A brand-new `--session-id` isn't used
because `/new` is what makes Claude rotate its own id, which the SessionStart
hook then records as the session to resume next time (the same path
`/new`-typed-by-hand already takes).

The submitting `Return` is **deferred a couple of event-loop ticks**
(`advance_submit_countdown` / `App::tick_new_submit`), not appended to the
`/new` burst: claude coalesces a chunk of bytes ending in `\r` into a single
paste, where the trailing `CR` lands as a literal newline and leaves `/new`
sitting unsent. Sending the Enter as a distinct keystroke a beat later makes
claude actually submit it. This mirrors the `tasks` sibling's
`pending_brain_submit` mechanism (learned there first).

## Why no comments-by-default and no decision log in code

Per the user's house style, new code gets a comment only when the *why* is
non-obvious; the function name + these docs carry the *what*. This repo is
not under git, so there's no PR review, no `.difit/` log, and no changelog
file — `docs/` is the durable record.

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
  shared `session`/`state`/`pty_pane` (near-identical ports) were kept
  as the single copy.

## Why `brain` is a "central dispatch", not a single-purpose tool

The user lives in the terminal and wants **one command** to reach
everything around `~/brain`: manage tasks, jump to a note, search across PARA
buckets, or think with a claude session rooted in the brain. Rather than
memorize several separate entry points, `brain` is the front door: a single
persistent shell with two main views (tasks and brain-directory search) and an
always-on brain panel. New top-level capabilities should be added as a main
view, a palette row, or a keybinding inside that shell, not as a separate
command (or a shell-mutating one-shot subcommand) the user has to discover.

## Why the command palette lives behind `Ctrl-p`

Every non-default action (per-bucket search, message brain, open tasks, move
the panel, create a PDF, delete) lives in the **command palette**, reachable
with `Ctrl-p` from the search view, so it is one keystroke from the search box
without cluttering the screen. `Ctrl-p` is the palette hotkey (matching the
`tasks` TUI), which is why it is no longer an up-motion alias in the picker (up
is `↑` / `Ctrl-k`).

## Why the palette is a modal overlay, not its own screen

The palette is drawn as a modal **overlay inside the picker's event loop**
(`menu::draw_modal` over the picker, `menu::MenuApp` driven by the picker's
`handle_key`), rather than a separate full-screen TUI the way it started.
The reason is `Esc`: a separate screen would have to *exit* on `Esc`,
dropping the user all the way back to the shell and losing the search they
were in. As an overlay, `Esc` just closes the box and the picker is still
right there underneath — the same back-out-of-a-modal behavior the `tasks`
TUI has. This is why `menu/` has no `run()`/event loop of its own; it
exposes pure state (`MenuApp`, `handle_key`) plus `draw_modal`, and the
search view owns the loop. A confirmed row returns a `Choice`, which
`tui/search_view.rs` runs in place (rescope, message brain, open tasks,
PDF/delete) without leaving the shell.

## Why archive is browsable now (it wasn't before)

Archive (`~/brain/archive`) is retired PARA material, and originally `brain`
left it out of every search on purpose. In practice the user still needs to
dig things back out of the archive, so it's now a first-class bucket:
"Search archive" is its own palette row and the `Archive` bucket is part of
global search. It sorts **last** in every grouped result so live
Projects/Areas/Resources stay on top and retired material doesn't crowd the
common case. Rescoping to any single bucket (Archive included) is a palette
row in the search view, never a CLI subcommand.

## Why the palette's top rows carry direct keystrokes

The two actions the user reaches for most — message brain, open tasks — sit at
the top of the palette and also fire directly from the search view via
`Ctrl-m` / `Ctrl-t`, so the common cases don't even require opening the
palette. The keystrokes are surfaced as dim `[…]` hints on their palette rows
(the `tasks` convention, via `menu::shortcut_for`) so they're discoverable
without cluttering the layout. `Ctrl-m` shares Enter's byte (`0x0D`), so it
depends on the kitty protocol we already push to stay distinct; without the
protocol it degrades to a plain `Enter` (open the selection), the same safe
fallback as `Ctrl-Enter`.

## Why `Enter` opens and `Ctrl-Enter` reveals (the swap)

Opening the file is the action the user wants most of the time, so it gets
the unmodified `Enter`; revealing the containing directory in Finder is
the rarer case and takes the `Ctrl-Enter` chord. A directory match has no
file to open, so `Enter` on one falls back to revealing the directory
(identical to `Ctrl-Enter`), which keeps `Enter` from ever being a no-op.

## Why `brain` is a pure TUI binary (no plan, no wrapper)

`brain` used to have shell-mutating one-shot subcommands (`cd`, `msg`, the
per-bucket search verbs). Those effects had to happen in the **parent shell**
(a child process can't `cd` the caller or exec a zsh alias/function), so the
binary printed a small `key=value` **plan** to stdout and a `brain` zsh wrapper
executed it. That whole mechanism is **gone**. Every capability now lives
*inside* the persistent shell, which performs its own file-open, Finder-reveal,
PDF, trash, and `claude`-launch actions by spawning processes; nothing needs to
mutate the parent shell. So there is no plan protocol and no wrapper: `run.sh`
just builds the binary when the sources change and `exec`s it directly. The
`claude` launch that once relied on the `cl` alias is now driven by the
configurable `claude_cmd`. See [integrations.md](integrations.md).

## Why the TUI renders to `/dev/tty`, not stdout

The binary's **stdout** is reserved for `brain config` output (a table or a
single value) plus clap's help/errors, so a caller can pipe or capture it
cleanly. If the TUI also drew to stdout, that output would be full of escape
codes and frame data. Rendering to `/dev/tty` (opened directly) keeps stdout
clean while the interactive UI still reaches the real terminal. crossterm's
raw-mode toggles and event reader operate on the controlling terminal
regardless, so input is unaffected.

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

Task management is a big domain with its own CSVs, recurrence rules, and TUI.
Since the tasks↔brain merge it is an **in-process main view** of `brain`
(`crate::tasks`), not a separate binary: `brain` reads the CSVs directly, and
`Ctrl-T` (or `brain tasks`) switches to the tasks view instantly with the brain
panel still beside it. The tasks logic stays in its own `src/tasks/` namespace
so it keeps its shape, but there is no cross-binary handoff (and no `tasks=1`
directive) anymore.

## Why the pure/impure split (and the `lib.rs`)

Every decision worth testing is pulled into a pure function:
`parse_config_root`, `expand_tilde_with_home`, `is_textlike`,
`finder_target`, `handle_key`, the `App` matching/navigation, the render
helpers, and `session::build_claude_command`. The thin shells that touch
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
away. Running `$EDITOR` *in the current terminal* would tear down the TUI to
edit a note. Instead the running TUI spawns a **new iTerm2 tab** (`osascript`)
with `cd <dir> && $EDITOR <file>` for text files, and `open <file>` for
everything else (which launches its own app).
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

## Why the brain panel launches a configurable `claude_cmd`

The brain panel must control the `--resume` / `--session-id` flag to drive the
resume model, so it can't defer to a shell alias that might inject its own
flags. Rather than hardcode bare `claude`, the launch command is the
configurable `claude_cmd` (`config::Config::claude_command`, default `claude
--dangerously-skip-permissions` — what the user's old `cl` alias resolved to).
`session::build_claude_command` splices that command in verbatim and then
appends brain's own `--resume` / `--session-id`, so a user can point the panel
at a different wrapper or add flags without brain losing control of the resume
flag, and brain never depends on a shell alias.

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
`/dev/tty`), `brain` stores the tool's path as an env variable
(`markdown_to_pdf_path`, in brain env — see "Why brain env split off from
brain config" below) and spawns it directly with `<file.md> --out`.

The path is not hardcoded (the repo is public). On first run it is
**auto-discovered** (PATH, then conventional bin dirs, then a one-shot login
shell that resolves an autoloaded function to the script it wraps) and
persisted. A missing or invalid path is a hard, fail-fast error pointing at
`brain env set markdown_to_pdf_path=…`. See `settings/markdown_pdf.rs`.

## Why `linear_workspace` is a slug, not a full URL

The Linear link config is the **workspace slug** (e.g. `acme`), not the whole
`https://linear.app/<slug>/issue/` prefix. The slug is the only part that
varies per user; brain owns the URL shape, so a user configures the minimum and
can't get the surrounding format wrong. `Config::linear_base_url` interpolates
it, and an empty slug simply omits Linear links.

## Why config lives inside the brain root (`<brain-root>/.config/`), not `~/.config/brain`

Everything brain persists — `config.json`, `personalization.json`, skill
`extensions/`, `plugins/` — lives in `<brain-root>/.config/`. The design went
through three positions and this is the resting one:

1. **A/original:** machine-local `config.json` at `~/.config/brain`, but
   personalization *inside* the brain root so it synced with the brain.
2. **Mid-project:** unify *everything* under `~/.config/brain` and sync that dir
   externally (via jpsyx-configs `home/`).
3. **Final (here):** unify everything *inside the brain root*.

Position 2 died on a concrete footgun. jpsyx syncs `$HOME` paths by symlinking
them at a **regenerated mirror** (`home-dist/`, wiped and rebuilt every sync). A
tool that *writes its config at runtime* (brain does: `config set`, `personalize
set`, onboarding, `resync_skills`) writing through such a symlink lands its bytes
in the gitignored mirror, which is then wiped on the next build — the write is
lost and never committed. brain can't dodge it the way the jpsyx-native
`email-triage` skill does (writing the repo *source* directly), because brain
must stay generic and **never know about jpsyx-configs**.

So instead of routing brain's runtime writes through any dotfiles mirror, we put
its config *inside the brain*. The brain root is already the user's synced,
portable content; brain's own state rides along with it for free (whatever syncs
the brain — sub-project C, Backblaze, etc. — syncs the config), and **jpsyx has
nothing to do with brain config at all**. The repo stays generic (no tracked
`config.json`), and brain still writes only under `$HOME`.

### The one exception: `root`

The brain-root *location* can't be stored inside the brain root — you'd need the
root to find the setting that tells you the root (circular). So `root` is
resolved outside the config system: the path in `~/.config/brain-root` (a
one-line file, tilde-expanded) if present, else the default `~/brain`. It is the
**only** machine-local piece of brain state, is **not** a `brain config` variable
(there is no `brain config set root`), and is edited by hand or tracked in
dotfiles. Because brain only ever *reads* it, a dotfiles tool can safely
mirror-symlink it — the runtime-write footgun above doesn't apply to a read-only
pointer. Pablo tracks it in jpsyx at `home/.config/brain-root` containing
`~/brain`.

### `markdown_to_pdf_path` and syncing a machine-specific value (historical; superseded)

At the time of this decision, `config.json` synced across machines and
`markdown_to_pdf_path` rode along inside it despite being machine-specific (the
binary sits at a different path per host); the startup gate compensated with a
self-heal (a stored path missing/not-executable on *this* machine triggers
re-discovery and re-persist before failing). **C1 (see "Why brain env split off
from brain config" below) removes the need for that compromise**:
`markdown_to_pdf_path` now lives in brain env, which never syncs, so there is
no synced-but-stale value to heal from in the first place. The startup gate
still re-discovers on an invalid path (a fresh machine that has never run
`brain` there yet has nothing to heal from), but the sync-races-a-machine-local-value
tension this subsection originally described no longer exists.

## Why brain env split off from brain config (C1) — and why it partially reverses "config lives inside the brain root"

Sub-project C (cross-machine sync of `~/brain` via Backblaze B2) makes the
brain directory itself sync across machines. That flips the risk the previous
decision was optimizing for: once the brain dir is what syncs, anything
machine-specific sitting inside `<brain-root>/.config/` — a discovered
`markdown_to_pdf_path`, and especially B2 credentials — would sync too, leaking
secrets and wrong-on-another-host paths onto every machine. So C1 splits
config into two stores by **lifecycle**, not just location:

- **brain env** (`~/.config/brain/env.json`, machine-local, `brain env` CLI) —
  anything that would be *wrong* if copied to another machine: `root`,
  `markdown_to_pdf_path`, and the Backblaze `sync` block (bucket, credentials,
  trigger flags). Lives at a fixed XDG-style path **outside** the brain root,
  so it is structurally exempt from whatever syncs the brain directory.
- **brain config** (`<brain-root>/.config/config.json`, `brain config` CLI) —
  everything that's *right* on every machine: `linear_workspace`,
  `daily_triage_name_pattern`, `day_rollover_hour`, `agenda_dir`,
  `calendar_id`, `claude_cmd`, `skills_auto_sync`. Unchanged in shape and
  location; it keeps riding the brain-dir sync exactly as before.

**The rule of thumb:** *wrong-if-synced ⇒ brain env; right-on-every-machine ⇒
brain config.* Personalization (`personalization.json`, `extensions/`,
`plugins/`) is unaffected — it's content *about you*, correct everywhere, so it
stays a third store inside the brain root alongside `config.json`.

This is a **partial reversal** of "Why config lives inside the brain root"
(above), which deliberately unified *everything* — including the
then-machine-specific `markdown_to_pdf_path` — inside the brain root to dodge
the jpsyx mirror-write footgun. That reversal is now correct because C makes
the *opposite* failure mode dominant: a synced brain dir means anything
machine-local placed inside it leaks across every machine on the next sync,
whereas the jpsyx mirror-write footgun the original decision was avoiding is
merely inconvenient (a lost local edit) rather than a wrong path or a leaked
secret landing somewhere it shouldn't.

Splitting off brain env also **retires the circularity argument** that used to
justify treating `root` as unstorable: `root` couldn't live *inside* the brain
root because you'd need the root to find the setting that names the root. Once
brain env lives at a fixed path outside the brain root, that circularity is
gone — `root` becomes a normal (if machine-local) env variable, and the legacy
`~/.config/brain-root` pointer becomes a back-compat read, auto-migrated into
`env.json`'s `root` key on first run.

**The residual jpsyx mirror-write footgun, now on `env.json`.** `env.json` is
runtime-mutable (`brain env set`, and the `markdown_to_pdf_path` self-heal), so
the same footgun the original decision dodged for `config.json` now applies to
it: if a dotfiles tool (e.g. jpsyx) mirror-*symlinks* `env.json` at a
regenerated mirror, a runtime write lands in the mirror and is lost on the next
rebuild. Brain does not solve this — it stays generic and has no jpsyx
awareness by design. The fix, if a user wants `env.json` to persist across
machines via a private dotfiles repo, is **jpsyx-side**: seed/copy the file
into place rather than symlinking it (or re-commit after changes), the same way
jpsyx already special-cases the read-only `~/.config/brain-root` pointer (safe
to symlink because brain only ever reads it — `env.json` is not read-only, so
the same trick doesn't apply). See the brain-sync design spec §12 for the full
record.

## C2 — `brain sync`'s rclone transport: env-var creds, `--max-delete` over `--check-access`, bisync over a custom merge

C1 shipped only a parse-only `SyncConfig`; C2 is the first phase that actually
moves bytes. The choices below are the ones a future phase (a triggers/watch
phase, or a conflict-resolve UI) needs to keep, not accidentally "simplify."

**Why credentials are `RCLONE_CONFIG_*` env vars, never a persisted
`rclone.conf`, and never on argv.** The brain-env `sync` block already holds
the B2 key id/app key as plaintext (machine-local, never synced — see the C1
decision above), so `brain sync` doesn't need a *second* secret store; it just
needs to hand rclone the same secret at invocation time. `src/sync/remote.rs`
builds an rclone remote (named `BRAIN`) entirely as env vars
(`RCLONE_CONFIG_BRAIN_TYPE`/`_ACCOUNT`/`_KEY`) passed to the child process,
with the argv carrying only `BRAIN:<bucket>/<path>` — no secret in it. Two
things this avoids: (1) a persisted `~/.config/rclone/rclone.conf` that would
be a second, harder-to-audit copy of the same credential, sitting outside
brain's own config lifecycle; (2) secrets on the process argv, which any other
user on the machine can read via `ps`. Rebuilding the remote from brain env on
every invocation costs nothing (it's a handful of string ops) and keeps the
credential's *only* copy where `brain env`/`brain sync setup` already manage
it.

**Why `--max-delete` and not `--check-access`.** rclone offers `--check-access`
as a symmetry guard (both sides must show matching `RCLONE_TEST` marker files,
or the run aborts) and `--max-delete` as a blast-radius guard (abort if a run
would delete more than a configured percent of files). We only use the
latter. `--check-access` requires brain to *create and manage* those marker
files on both sides — a whole feature C2 doesn't build — so turning it on
today would make every single run abort. `--max-delete` needs no such
bookkeeping: it just counts what bisync is about to delete against
`sync.max_delete_percent` (default 50) and refuses the run (without
propagating any deletes) if that threshold is exceeded. It's the cheaper guard
that already covers the failure mode we actually care about (a wiped or
mostly-empty side quietly deleting everything on the other machine); a future
phase can add `--check-access` on top once it owns the marker-file lifecycle,
but it isn't a prerequisite for a safe first sync.

**Why `rclone bisync` and not a from-scratch merge.** The brain root isn't
just markdown notes — it also holds `tasks.csv`/`habits.csv`, which don't
merge line-by-line the way prose does. Rather than write and maintain a
CSV-aware three-way merge, C2 leans on `rclone bisync`, which already
implements correct bidirectional sync semantics: it tracks each side's prior
listing so it can distinguish "changed here" from "deleted there" (the
half of bidirectional sync that's easy to get wrong — naively diffing two
current snapshots can't tell an edit from a delete-then-recreate), and it
ships conflict handling and delete-propagation guards brain would otherwise
have to reinvent. A CSV-specific merge strategy, if ever needed, is a
narrower problem to solve later on top of this transport, not a reason to
avoid it now.

**Why a same-file conflict is "keep both," not "pick a winner and discard."**
`--conflict-resolve` (`newer` for a bare `brain sync`, `path1`/`path2` for
`--push`/`--pull`) decides which copy rclone treats as the winner for *this
run*, but `--conflict-loser pathname` (not the default `num`) tells it to keep
the loser too, under a suffix, rather than delete it. Silently discarding a
same-file edit on a personal knowledge base is worse than a little clutter —
the loser might be the copy the user actually wanted. brain's post-pass then
renames the marker to a human-readable `name (conflict <host> <date>).ext`
(see [data-model.md](data-model.md)) so resolving it later is a normal
file-manager task, not spelunking for `__brainconflict__` files.

**Why there is no silent auto-resync.** When bisync aborts (the
`--max-delete` guard, or rclone's own "prior listings missing" guard), C2
deliberately does not retry with `--resync` on its own — `--resync` makes one
side unconditionally overwrite the other's listing, and blindly doing that
after an abort could paper over exactly the kind of change (a wiped
directory, a botched previous run) the guard exists to catch. Instead the
abort is surfaced (`Outcome::Aborted`, with a message pointing at `brain sync
init` or `brain sync --resync`) and the human decides. This mirrors the
project's broader pattern of surfacing rather than auto-healing anything
that touches data loss (contrast the auto-healed `markdown_to_pdf_path`,
which only ever *rediscovers a tool path* — never anyone's data).

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

## Why personalization is just another brain config (in the brain root)

Personalization (name, role, who you work for, tag styles, namespaces) is
content *about you* that should be identical on every machine. It is stored beside
`config.json` in the brain config dir (`<brain-root>/.config/personalization.json`,
`settings::config_dir()`) — just another brain config, riding along when the
brain dir syncs. See the config-location decision above for why everything brain
persists lives inside the brain root (and the `root`-pointer exception).

## Why tag styles (and identity) are personalization, not hardcoded

The public repo must carry no personal taxonomy. The task renderer's tag →
emoji+label map used to be a hardcoded `match` full of one user's tags
(`ceo`, `aa`, `mit`, …). Now the binary ships only a tiny universal default set
(`mit`, `personal`, `work`) with a raw-name fallback, and every other tag is a
user override in `tag_styles`. Same for identity: a skill's generic *rule*
("act as a personal assistant") stays in the skill; the personalized *who* it
serves ("a CEO at Avandar") is personalization the skill looks up. This is the
hybrid model — identity is a runtime lookup (`brain personalize show`), so it
changes instantly without a rebuild, while structural per-user variation is left
to the skill render pipeline.

## Why mutations call a `resync_skills()` seam (currently a no-op)

Any `config set` / `personalize set` / onboarding change should keep the
installed skills consistent with the user's values, so every mutation path calls
a single `skills::resync_skills()` hook. The real render/install pipeline is a
later sub-project; wiring the seam now (as a no-op) means that sub-project only
fills in the body, without hunting down every mutation. It must never fail a
mutation — a `config set` succeeds even if a future render step errors.

## Why the renderer resolves tags via a process cache, not threaded state

`type_label` is on a hot render path with two call sites whose callers fan out
widely. Rather than thread `&TagStyles` through every render signature, the
user's styles load once into a process cell (`personalization::runtime`) at
startup. The *decision* logic (`TagStyles::label`) stays a pure, unit-tested
function; the cell is only the data supply. Until it is initialized (i.e. in
unit tests) it falls back to the generic defaults, so tests never see the dev
machine's personalization and stay hermetic. The running TUI reads styles at
startup; changing personalization takes effect on the next launch.

## Why bundled skills are embedded in the binary (`include_dir`)

brain is meant to be cloned and used by anyone, and its skills must work in *any*
agent session, not just inside the repo. Embedding the `skills/` dir into the
binary with `include_dir` makes it self-contained: `brain skills sync` writes the
skills out wherever they're needed, so a user who `cargo install`s brain (or
moves the binary) still gets them. `include_str!` can't carry a skill's multiple
files (SKILL.md + scripts), which is why the one dependency is justified.

## Why the skill install is a two-hop link (registry → built), matching jpsyx

`brain skills sync` writes a built copy, links `~/.agents/skills/<name>` at it,
then links each frontend's skills dir at that registry entry. This mirrors the
existing jpsyx fan-out exactly, so brain-owned skills sit in the same shared
registry every frontend already reads — and so brain and jpsyx can coexist on
Pablo's machines once the B4 bridge stops jpsyx from pruning brain-owned entries.
The link *targets* are a pure function (`layout::link_ops`), unit-tested; the FS
shell (`install`) stays thin.

## Why the skill sync is gated off (`skills_auto_sync`) during rollout

`resync_skills()` is wired into every config/personalize mutation, but a mutation
must not silently rewrite the user's live agent registry while the pipeline is
still being built (sub-projects B1–B3) — on Pablo's machine that registry is
still jpsyx-owned, and dual management would collide. So the auto-sync is gated
behind a config flag defaulting to `false`; the pipeline is exercised only via
`brain skills sync --root <sandbox>` until the B4 cutover flips the flag and
fixes jpsyx's prune. Safety-first: the default can't disturb a live setup.

**Post-B4:** the flag now defaults to `true` — the rollout is over, the registry
is brain-owned, and jpsyx no longer fights brain for it, so a mutation *should*
re-render the live registry (program invariant #5). Setting it `false` reverts to
sync-only-on-demand via explicit `brain skills sync`.

## Why extensions inject at named hooks (not append-only, not runtime lookup)

A user must be able to personalize a bundled skill without forking it — and
sometimes that means running something *at the start* of the skill (Pablo's
`triage` calls email-triage first), which a trailing "append" can't express and a
pure runtime lookup can't order reliably. So base skills declare named markers
(`<!-- brain:ext hook -->`) and the extension file supplies content per hook,
substituted in place. Content with no matching marker still lands in a trailing
"Personal extensions" section, so nothing the user wrote is silently dropped. A
skill with no extension renders unchanged (markers are stripped).

## Why extensions render a new built copy, never the repo/plugin source

The repo must stay 100% generic and a plugin is the user's own artifact; neither
should be mutated by personalization. So injection happens only when writing the
*built* copy that Claude/Codex read (`render` → `install`), leaving the source
pristine. This is the structural half of the A hybrid model (identity stays a
runtime lookup; behavior changes are rendered), and it keeps `git status` on the
repo clean no matter how heavily a user personalizes.

## Why the bundled `triage` skill drops email/Linear/Notion into extension hooks

Migrating `triage` (B3) forced a line between generic triage logic and one
person's tool stack. The rule we settled on: a *generic workflow* stays in the
core skill (past-due grouping, at-risk scan, chronic-ignore sweep, the
scratch-inbox sweep, the monthly backlog review, the AskUserQuestion protocol);
an *identity fact* becomes a runtime `brain personalize show` lookup ("busy CEO"
→ role/works_for); and a *specific external tool or private URL* moves to the
user's extension. So the whole email-triage-first pass, the Superhuman
reply-reconcile, the Linear reconcile/mirror-in/grooming pass (it hardcodes an
owner email — which personalization has no field for — plus an `AVA-###` prefix
and a `/linear-pm` dependency), and the private Notion In-Basket URL all live in
Pablo's `triage` extension, not the repo. The core declares four hooks
(`triage:daily-open`, `triage:daily-linear`, `triage:weekly-inboxes`,
`triage:weekly-linear`) at the exact points those passes ran, so the rendered
copy reproduces the original flow byte-for-behavior while the repo stays generic.
A guard test (`bundled_skills_carry_no_personal_data`) fails the build if any
bundled skill grows a personal token, so this line can't silently erode.

Cross-skill script calls (todo's `mark_done.py`, `find_chronic_ignored.py`, …)
standardized on the install-registry path `~/.agents/skills/todo/scripts/<name>.py`
rather than the old `~/global-skills/...` (jpsyx) or `~/.claude/skills/...`
(one-frontend) forms: that path is frontend-agnostic and is exactly where
`brain skills sync` installs the `todo` skill, so it resolves for any cloner.

## Why second-brain split into a lean core + a `/contacts` skill + a `zotero-sync` plugin

The original `second-brain` skill was 1200+ lines that fused three concerns:
generic PARA knowledge management, a full Zotero reference-manager integration,
and a local contacts book. Migrating it (B3) separated them along ownership
lines:

- **Core `second-brain`** keeps only generic PARA (buckets, decision flow,
  naming, IPs, project lifecycle, add-resource, extract-IP, sync, CSV tooling).
  The "how to summarize" step delegates to `/article-summarizer`; Zotero-specific
  commands are gone.
- **`/contacts`** is now its **own bundled skill** (generic CSV book +
  `contacts.py`), so it loads only when the user actually asks about a person —
  second-brain just points at it. Its Notion "Our People" fallback is a personal
  extension (`contacts:fallback`).
- **`zotero-sync`** is a personal *plugin* (not bundled): every Zotero command,
  the richer additions table, reading-state retrieval, and the `zotero-*.md`
  references. It builds on core second-brain + `/article-summarizer`.

**Namespaces became runtime config, not an extension hook.** An earlier plan put
the user's project namespaces (`work`/`personal`/…) behind a `second-brain`
extension hook, but namespaces (and the task-tag set) are better modeled as
first-class personalization surfaced in onboarding and editable via
`brain config set namespaces|tags`. So core second-brain reads the configured set
at runtime (`brain personalize show`) with generic example namespaces in the
prose, and declares no namespaces hook — only `company-context` (load a company
profile note) and `reference-manager` (hand off to a reference-manager plugin).

## Why the `todo` migration split off Linear + a whole personal scheduling layer

`todo` was the largest, most-entangled skill (1880 lines) and carried *two*
deeply-woven personal subsystems on top of the generic task core. The split:

- **Generic core** keeps the task system (schema, commands, agenda ordering,
  habits, chunked tasks, backlog, sync), including the `linear_issue` column as
  **inert external-issue-link plumbing** (kept named `linear_issue` so the live
  tasks.csv + `set_linear_issue.py`/`list_linked_tasks.py` scripts don't churn)
  — but nothing in core contacts an external service.
- **Linear → a `linear-sync` plugin** (not a single-file extension, because its
  `linear-link.md` playbook is a reference file — same reasoning as
  `zotero-sync`). Core exposes a `todo:linear` hook + a `todo:linear-backlog`
  hook; the personal `todo` extension fills them by pointing at `/linear-sync`.
  The `triage` extension's `linear-link.md` refs were repointed to the plugin.
- **The agenda's personal scheduling layer** — Google-Calendar busy-block pull,
  the work-hours cutoff + late-work streak, and Pablo's specific daily anchors
  (plus his work-vs-personal `task_type` taxonomy) — moved to `todo:calendar`,
  `todo:cutoff`, and `todo:anchors` hooks. Core agenda is calendar-optional and
  anchor-agnostic: it orders tasks/habits and writes the PDF, pulling busy
  blocks only if `calendar_id` is set and applying a partition/cutoff only if an
  extension defines one.
- Two config vars back this: `agenda_dir` (default `~/Downloads`, so the PDF
  destination isn't hardcoded) and `calendar_id` (empty = no calendar
  integration). Scripts read `BRAIN_AGENDA_DIR`/`MARKDOWN_TO_PDF` from the env
  with sane fallbacks instead of a hardcoded jpsyx module path.

The guard test gained `jpsyx` as a forbidden token so the private dotfiles-repo
path can never re-enter a bundled skill. Note the guard catches identity/paths,
not every personal detail (it wouldn't flag "Walk Luna"); depersonalizing a
skill still needs human judgment for personal-but-not-identifying content.

## Why no comments-by-default and no decision log in code

Per the user's house style, new code gets a comment only when the *why* is
non-obvious; the function name + these docs carry the *what*. This repo is
not under git, so there's no PR review, no `.difit/` log, and no changelog
file — `docs/` is the durable record.

## B4 — the jpsyx bridge + live cutover (ownership boundary, prune-safety, rollback)

The B1–B3 pipeline was proven only in a sandbox; B4 is the one phase allowed to
touch the live agent registry. The cutover flips Pablo's six migrated skills
(`article-summarizer`, `brain-knowledge-capture`, `contacts`, `second-brain`,
`todo`, `triage`) plus his two plugins (`zotero-sync`, `linear-sync`) from
jpsyx-owned to brain-owned, and makes jpsyx delegate to `brain skills sync`
without ever pruning what brain owns.

**The ownership boundary is the link target, not a manifest file.** A registry
or frontend skill link is *brain-owned* iff it (transitively) resolves under
brain's built dir (`$XDG_DATA_HOME/brain/skills` or `~/.local/share/brain/skills`).
jpsyx-owned links resolve into `~/global-skills` or `~/.agents/skills`. This
falls out of the two systems' existing designs and needs no new file:

- jpsyx's fan-out only ever creates/repoints/prunes links whose target
  `points_into` its own sources (`~/.agents/skills`, `~/global-skills`). A
  brain-owned registry entry (`~/.agents/skills/todo → ~/.local/share/brain/…`)
  points into *neither*, so jpsyx's `prune_into_sources` leaves it alone and its
  aggregation records it as a harmless "conflict" (foreign symlink) rather than
  clobbering it. jpsyx's `sync::prune`/`sweep` only touch dangling links into the
  *mirror* (`home-dist/`), which brain links never are. So every existing jpsyx
  prune path already spares brain-owned links *by construction*.
- brain's `install::sync` `remove_existing` cleanly replaces the old jpsyx-owned
  symlink at each name, so the cutover is a plain re-link, not a conflict.

B4 makes this boundary **explicit and defended** rather than merely emergent:
jpsyx gains a `brain` step that (1) invokes `brain skills sync` before the
fan-out so the registry is brain-populated first, and (2) teaches the fan-out to
recognize brain's built dir as a protected foreign source (a brain-owned name is
never aggregated-over or pruned), backed by a regression test that runs a full
jpsyx sync and asserts brain-owned links survive.

**Cutover steps (executed on Pablo's machine, this phase):**
1. Flip `skills_auto_sync`'s default to `true` (rollout is over; mutations should
   re-render live, per program invariant #5).
2. Snapshot the live `~/.agents/skills` + every frontend skills dir to the
   scratchpad; write a deterministic rollback script *before* mutating anything.
3. Run `brain skills sync` for real: installs the 6 skills + 2 plugins into the
   registry (replacing the jpsyx-owned links) and fans them out to the frontends.
4. In jpsyx: add the `brain skills sync` delegation + brain-ownership prune
   guard; remove the 4 migrated skills (`todo`, `second-brain`, `triage`,
   `brain-knowledge-capture`) from `home/global-skills/` so jpsyx stops owning
   them. (`article-summarizer`/`contacts` were never in jpsyx; `email-triage`,
   `habits`, and `zotero-article-summary` stay jpsyx-owned — the first two were
   never migrated, and `zotero-article-summary` is left in place as a no-regression
   default until Pablo confirms the `zotero-sync` plugin fully supersedes it.)
5. Persist brain's config across machines. **Resolved not by jpsyx but by moving
   the config into the brain root** (`<brain-root>/.config/`), so it syncs with
   the brain and jpsyx never touches it — see "Why config lives inside the brain
   root" above. This sidesteps the mirror-clobber footgun entirely (jpsyx would
   have symlinked `~/.config/brain` at the regenerated `home-dist/` mirror and
   lost brain's runtime writes on the next sync). The one machine-local remnant,
   the `root` pointer (`~/.config/brain-root`), *is* jpsyx-tracked — safely,
   because brain only reads it.

**Rollback:** `scratchpad/b4-snapshot/ROLLBACK.sh` removes brain's built dir and
brain-owned links, then re-runs `jpsyx sync` to restore the jpsyx-owned registry
(the migrated skills must still exist under `home/global-skills` in the jpsyx
checkout, or be restored with `git checkout -- home/global-skills` first).

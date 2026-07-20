# AGENTS.md

You are an AI agent working in `~/src/jpsyx/brain/`. This file is the
single entry point for anything an agent needs to know about the project.
**Read it before changing code.** (Other agent tools — Codex, opencode,
Cursor — also resolve `AGENTS.md`; `CLAUDE.md` is a symlink to this file
so Claude Code picks it up too.)

## What this project is

`brain` is the user's **central terminal dispatch** and the **single CLI**
for both their second brain and their task system. (The standalone `tasks`
binary was merged in; it no longer exists.) It's a small Rust CLI
(clap + ratatui) that browses `~/brain` (PARA) and manages
`~/brain/tasks/{tasks,habits}.csv`.

Bare `brain` (and `brain tasks …`) opens a **persistent shell** (`tui/`) with
**two main views** — the **tasks view** (task management, agenda, triage; the
startup default) and the **brain-directory search view** (fuzzy-pick over
`~/brain`) — plus one app-level **brain panel** (an interactive `claude`
session in a PTY that resumes your latest conversation, open at startup and
shared by both views). Switch views with `Ctrl+L`/`Ctrl+H` (cycle) or
`Ctrl+T`/`Ctrl+B` (jump). Read [docs/glossary.md](docs/glossary.md) first for
the main-view / sub-view / brain-panel vocabulary.

The one-shot subcommands (`pr|ar|re|s|cd|msg`, `brain <freeform>`) and the
tasks utilities (`brain tasks {complete|doctor|search|--no-tui}`) stay
short-lived. Because some effects must happen in the *parent shell* (cd, the
`cl` alias), a one-shot binary prints a small **plan** to stdout and a zsh
wrapper (`./brain`) executes it; the persistent shell emits no plan (it just
returns on quit). The persistent shell keeps state (`~/.cache/brain/state.db`,
table `brain_sessions`) to resume the right Claude session (lock + recency)
and remember the panel layout, fed by a single Claude `SessionStart` hook
(`scripts/claude_session_start_hook.py`, keyed on `BRAIN_*`).

For a deeper map, see [docs/architecture.md](docs/architecture.md).

## The docs/ contract

**Whenever you add a feature, change a keybinding, alter the plan
protocol, change root resolution, or change the module shape, update the
relevant file under `docs/` in the same change.**

The docs are the source-of-truth for *what* `brain` does and *why*. Code
is the source-of-truth for *how*. They must agree on *what*.

| If you change… | Update… |
| --- | --- |
| A plain-English term ↔ code mapping (view, sub-view, panel, …) | `docs/glossary.md` |
| Module list, data flow, plan protocol shape, main-view routing | `docs/architecture.md` |
| User-visible behavior (main views, menu items, subcommands, picker/tasks behavior) | `docs/features.md` |
| `Bucket` / `Entry`, the picker match model, the `Task`/sub-view model | `docs/data-model.md` |
| A **tasks-view** keybinding | `docs/keybindings.md`, the `src/tasks/shortcuts.rs` table (footer + help modal), `compact_footer_line` in `src/tasks/render/chrome.rs`, **and** (if it's also a palette / task-action row) `shortcut_for` in `src/tui/palette.rs` |
| A **main-view-switch** or app-level keybinding (`Ctrl+H/L/T/B`, `Alt+?`) | `docs/keybindings.md`, the pure classifiers in `src/main_view.rs`, and the Global rows in `src/tasks/shortcuts.rs` |
| A **brain-search-view** keybinding or menu row | `docs/keybindings.md`, `src/menu.rs` (`items` + `shortcut_for`), `src/tui/search_view.rs` |
| The plan protocol or the `cl`/`open`/`edit` path | `docs/integrations.md` **and** the `brain` wrapper's `case` |
| The SessionStart hook, state DB schema, or `BRAIN_*` env | `docs/integrations.md`, `scripts/claude_session_start_hook.py`, `scripts/install_hook.sh`, `src/state.rs` |
| `config.json` schema or root resolution | `docs/config.md` (fields split across `src/config.rs` + `src/paths.rs`) |
| Testing strategy, what we test vs. skip | `docs/testing.md` |
| A non-obvious design choice | `docs/decisions.md` |

If a change spans categories, update all the relevant docs. Do not defer.

## Red/Green TDD — the iron law

**No production code lands without a failing test written first.**

1. **RED.** Write the smallest test for the next behavior. Run it. Watch
   it fail. A test you never saw fail proves nothing.
2. **GREEN.** Write the simplest code that turns *that* test green. Don't
   add behavior the red test doesn't demand; don't widen the test surface
   between red and green.
3. **REFACTOR.** Clean up with the bar green; re-run to stay green.

Then the next red. When fixing a bug: first a failing test that reproduces
it, *then* the fix. Push every decision worth testing into a **pure
function** (the picker's matching, menu `handle_key`, `is_textlike`,
config parsing, the `plan::*_to` writers) and test that, rather than
mocking the terminal or the filesystem. See
[docs/testing.md](docs/testing.md) for the full strategy and layout.

## Build, run, test

```sh
# rebuild + run via the zsh wrapper (auto-rebuilds when src/ changes)
brain

# manual rebuild
( cd ~/src/jpsyx/brain && cargo build --release )

# headless smoke test of the plan output (no TUI on these paths)
./target/release/brain tasks   # → prints `tasks=1`
./target/release/brain cd      # → prints `cd=<root>`

# the full test suite — runs in well under a second
( cd ~/src/jpsyx/brain && cargo test --release )

# one module's unit tests / one integration file
cargo test --release picker::
cargo test --release --test entry_collect

# lint clean (pedantic + nursery are on)
cargo clippy --release --all-targets
```

The `brain` zsh function at `~/src/jpsyx/brain/brain` is the user's entry
point. They never type `cargo run`.

## Quick orientation for new agents

1. Read [docs/README.md](docs/README.md) — the index.
2. Read [docs/architecture.md](docs/architecture.md) end-to-end.
3. Scan [docs/features.md](docs/features.md) for what the user can do.
4. Open the file you actually need to touch.
5. RED test → GREEN code → update docs, in the same change.

## House rules (project-specific)

- **No `unsafe`.** `[lints.rust] unsafe_code = "forbid"` enforces it.
- **Keep clippy clean.** `pedantic` + `nursery` are on at `warn`; don't
  add new warnings.
- **The binary's stdout is only the plan.** Never `println!` diagnostics
  — they'd be parsed as directives or echoed. Diagnostics go to stderr;
  the TUI goes to `/dev/tty`.
- **Don't add dependencies casually.** The set is small on purpose. If you
  need one, justify it in `docs/architecture.md`.
- **Follow the pure/impure split.** New decision logic goes in a pure
  function (testable); the `/dev/tty` and `Command` shells stay thin.
- **Keep the dimmed shortcut annotation in sync with the binding.** Every
  command palette row that has a direct keystroke shows it as a gray `[…]`
  hint, driven by `shortcut_for` in `src/menu.rs`. Whenever you add or
  change a keybinding for an action that also appears in the palette,
  update `shortcut_for` in the same change so the gray hint matches the
  real binding. If I tell you a new (or changed) action's shortcut is
  `[abc]`, register `[abc]` in `shortcut_for` as part of the work. Don't
  make me ask for it.
- **Comments only when the *why* is non-obvious.** The function name and
  the docs cover the *what*.
- **This repo is not under git.** No PR review, no decision-log file, no
  `.difit/` directory. `docs/` is the durable record.

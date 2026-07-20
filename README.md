# brain

**`brain` is the central terminal dispatch for the user's second brain and
task system.** It's the one command to reach everything you do from the
terminal around `~/brain`: cd between PARA buckets, fuzzy-pick a note
across them, start a claude conversation rooted in the brain, or open the
task-management TUI. Bare `brain` opens a menu of all of it.

Anything brain-related or task-related — notes, projects, areas,
resources, tasks, habits, agenda, triage — goes through `brain`. It
doesn't reimplement those tools; it routes you to the right one.

## Why a dispatch?

The user lives in the terminal and would rather type one command than
remember a dozen. So `brain` is the front door, and it hands off to
specialized tools: the `tasks` CLI for task management, the `cl` alias for
claude, and Finder / `$EDITOR` for files. Adding a capability means adding
a menu row and a subcommand, not another command to memorize.

## Usage

```sh
brain                 # interactive menu of every action
brain s rust borrow   # fuzzy-pick across all buckets, seeded with a query
brain pr              # cd into ~/brain/projects
brain pr afloat       # fuzzy-pick inside projects, seeded with "afloat"
brain ar / brain re   # areas / resources (same no-arg cd, with-query pick)
brain cd              # cd into the brain root
brain msg "draft the Q3 plan"   # open claude in ~/brain with this prompt
brain tasks           # open the tasks TUI (task management, agenda, triage)
```

In the fuzzy picker: type to filter, `↑/↓` (or `Ctrl-k`/`Ctrl-j`/`Ctrl-n`; `Ctrl-p` opens the palette) to
move, `Enter` to reveal in Finder, `Ctrl-Enter` to open the file
(text → `$EDITOR`, otherwise the system default app), `Esc` to quit.
Full key tables: [docs/keybindings.md](docs/keybindings.md).

## How it works (one paragraph)

`brain` is a Rust binary plus a thin zsh wrapper. Effects that must happen
in the *parent shell* (cd, the `cl` alias, the `tasks` function) can't be
done by a child process, so the binary prints a small **plan**
(`cd=…`, `claude=…`, `tasks=1`, …) to stdout and the wrapper executes it.
The interactive UI renders to `/dev/tty` so stdout stays clean for the
plan. Details: [docs/architecture.md](docs/architecture.md) and
[docs/integrations.md](docs/integrations.md).

## Configuration

Settings live in `~/.config/brain/config.json`, managed with `brain config`:

```sh
brain config list                 # every variable, value, and description
brain config set root=~/brain     # point brain at a different brain directory
```

`root` defaults to `$HOME/brain`. Note: `markdown-to-pdf` is a prerequisite
(auto-discovered on first run). See [docs/config.md](docs/config.md).

## Developing

```sh
( cd path/to/brain && cargo test --release )       # full suite (<1s)
( cd path/to/brain && cargo clippy --release --all-targets )
```

We follow **red/green TDD**: no production code without a failing test
first. Read [AGENTS.md](AGENTS.md) and [docs/](docs/README.md) before
changing code — and update the docs in the same change.

## Docs

- [docs/README.md](docs/README.md) — index and read order
- [docs/architecture.md](docs/architecture.md) — modules, plan protocol, data flow
- [docs/features.md](docs/features.md) — every menu item and subcommand
- [docs/data-model.md](docs/data-model.md) — buckets, entries, fuzzy matching
- [docs/keybindings.md](docs/keybindings.md) — menu + picker key tables
- [docs/integrations.md](docs/integrations.md) — the wrapper, `tasks`, claude
- [docs/config.md](docs/config.md) — the config store, `brain config`, and root resolution
- [docs/testing.md](docs/testing.md) — TDD doctrine and test layout
- [docs/decisions.md](docs/decisions.md) — the "why" behind the design

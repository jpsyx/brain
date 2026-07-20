# brain

**`brain` is the central terminal dispatch for the user's second brain and
task system.** It's the one command to reach everything you do from the
terminal around `~/brain`: manage tasks, fuzzy-pick a note across the PARA
buckets, or think alongside a `claude` session rooted in the brain. Bare
`brain` opens a persistent shell with all of it.

Anything brain-related or task-related — notes, projects, areas,
resources, tasks, habits, agenda, triage — goes through `brain`.

## Why a dispatch?

The user lives in the terminal and would rather type one command than
remember a dozen. So `brain` is the front door: a persistent shell with two
main views (tasks and brain-directory search) and an always-on `claude`
brain panel, plus Finder / `$EDITOR` handoffs for files. Adding a capability
means adding a palette row or a keybinding, not another command to memorize.

## Usage

```sh
brain                 # persistent shell, tasks view (the startup default)
brain tasks           # same shell, launched on the tasks view explicitly
brain tasks today --no-tui        # print today's tasks, no TUI
brain tasks complete t123         # mark a task complete
brain tasks doctor                # health check
brain config          # read/change persistent config
```

Inside the shell: `Ctrl-L`/`Ctrl-H` cycle main views, `Ctrl-T`/`Ctrl-B`
jump to the tasks / brain-search view, `Ctrl-P` opens the command palette,
and `Alt-S` shows help. In the brain-search view: type to filter, `↑/↓`
(or `Ctrl-k`/`Ctrl-j`) to move, `Enter` to open the file (text → a new
iTerm2 tab / `$EDITOR`, otherwise the system default app), `Ctrl-Enter` to
reveal in Finder. Full key tables: [docs/keybindings.md](docs/keybindings.md).

## How it works (one paragraph)

`brain` is a single Rust binary. It has no shell-mutating one-shot
commands, so it needs no wrapper: `run.sh` builds it when the sources change
and `exec`s it directly, forwarding args. Everything the user does happens
inside the persistent TUI, which renders to `/dev/tty` and performs its own
file-open, Finder-reveal, and `claude`-launch actions by spawning
processes. The binary's stdout carries only `brain config` output plus
clap's help/errors. Details: [docs/architecture.md](docs/architecture.md)
and [docs/integrations.md](docs/integrations.md).

## Configuration

Settings live in `~/.config/brain/config.json`, managed with `brain config`:

```sh
brain config list                 # every variable, value, and description
brain config set root=~/brain     # point brain at a different brain directory
```

`root` defaults to `$HOME/brain`, and the persistent shell honors it. Note:
`markdown-to-pdf` is a prerequisite (auto-discovered on first run), and
`claude_cmd` sets the command the brain panel launches (default `claude
--dangerously-skip-permissions`). See [docs/config.md](docs/config.md).

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
- [docs/architecture.md](docs/architecture.md) — modules, routing, data flow
- [docs/features.md](docs/features.md) — every main view, palette row, and subcommand
- [docs/data-model.md](docs/data-model.md) — buckets, entries, fuzzy matching
- [docs/keybindings.md](docs/keybindings.md) — app / tasks / search key tables
- [docs/integrations.md](docs/integrations.md) — `run.sh`, claude, the hook / state DB
- [docs/config.md](docs/config.md) — the config store, `brain config`, and root resolution
- [docs/testing.md](docs/testing.md) — TDD doctrine and test layout
- [docs/decisions.md](docs/decisions.md) — the "why" behind the design

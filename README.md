# brain

**`brain` is the central terminal dispatch for the user's second brain and
task system.** It's the one command to reach everything in the selected
workspace: manage tasks, fuzzy-pick a note across the PARA buckets, or think
alongside an agent session rooted in that workspace. Bare `brain` opens the
default workspace in a persistent shell with all of it.

Anything brain-related or task-related — notes, projects, areas,
resources, tasks, habits, agenda, triage — goes through `brain`.

# Installation

`brain` isn't published to npm or crates.io — you build it from source. It's a
single Rust binary, so all you need is a [Rust toolchain](https://rustup.rs) and
a clone:

```sh
git clone https://github.com/jpsyx/brain.git
cd brain
```

Then pick one of two ways to run it.

**A. Install it on your `PATH`** (simplest — gives you a global `brain`):

```sh
cargo install --path .     # builds release, installs `brain` into ~/.cargo/bin
brain                      # run it (ensure ~/.cargo/bin is on your PATH)
```

Re-run `cargo install --path .` after a `git pull` to update.

**B. Run through `run.sh`** (auto-rebuilds whenever the sources change):

`run.sh` builds the binary on first run (and whenever the `src/` is newer than
the binary), then `exec`s it and forwards your arguments — so it always runs the
current code, no manual rebuild after a `git pull`:

```sh
./run.sh                          # builds if needed, then runs `brain`
./run.sh tasks today --no-tui     # any args are forwarded
```

For a global command that stays current, point a shell function at it in your
`~/.zshrc` / `~/.bashrc` (adjust the path to your clone):

```sh
brain() { ~/src/brain/run.sh "$@"; }
```

Either way, the first run [sets brain up](#1-setup): a short onboarding prompt and
installing the bundled skills. Then read the [User manual](#user-manual) below.

## Why a dispatch?

The user lives in the terminal and would rather type one command than
remember a dozen. So `brain` is the front door: a persistent shell with two
main views (tasks and brain-directory search) and an always-on agent
brain panel, plus Finder / `$EDITOR` handoffs for files. Adding a capability
means adding a palette row or a keybinding, not another command to memorize.

## Usage

```sh
brain                 # persistent shell, tasks view (Claude brain panel)
brain --codex         # same shell, with Codex in the brain panel
brain -cx            # short alias for --codex
brain tasks           # same shell, launched on the tasks view explicitly
brain tasks today --no-tui        # print today's tasks, no TUI
brain tasks complete t123         # mark a task complete
brain tasks doctor                # health check
brain config          # read/change persistent config
brain sync -b family  # run a command in another registered workspace
brain workspace list  # show registered workspaces, aliases, and the default
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
file-open, Finder-reveal, and agent-launch actions by spawning
processes. The binary's stdout carries only `brain config` output plus
clap's help/errors. Details: [docs/architecture.md](docs/architecture.md)
and [docs/integrations.md](docs/integrations.md).

---

# User manual

Everything below is what a person setting up their own `brain` needs: where it
keeps its state, how to configure and personalize it, and how the bundled skills
work and how to make them yours without forking the repo.

## 1. Setup

**Prerequisites**

- A Rust toolchain (only to build; `run.sh` builds on first run and when sources change).
- [`markdown-to-pdf`](#the-markdown-to-pdf-prerequisite) on your `PATH` — brain
  uses it to turn notes/agendas into PDFs. Auto-discovered on first run.
- The `claude` CLI for the default brain panel, or the `codex` CLI if you run
  `brain --codex` / `brain -cx`.

**Register a workspace**

Your "brain" is a [PARA](https://fortelabs.com/blog/para/) directory —
`projects/`, `areas/`, `resources/`, `archive/`, plus `tasks/`. The first
workspace becomes the default. Create a new root, or attach an existing synced
root:

```sh
brain workspace create --name brain --root ~/brain
brain workspace attach ~/family
```

Run any workspace-scoped command with `--brain <name-or-alias>` or `-b`; omit
the selector to use the default. `brain workspace default <name>` changes only
which workspace an omitted selector chooses. It does not change access mode.

**First run**

Run `brain`. If you've never personalized it, a short, skippable prompt asks for
your name, role, and who you work for, then installs the bundled skills. That's it
— brain works fully even if you skip every prompt.

## 2. Where brain keeps things

brain splits what it persists into **two stores**, by lifecycle:

- **brain config** — a dir inside each workspace root, `<brain-root>/.config/`
  (e.g. `~/brain/.config/`). Holds everything that's *right* on every machine.
  Because it lives **inside that workspace**, it travels with it: whatever
  syncs the workspace root across machines syncs these too. Nothing external (no
  dotfiles tool) is involved.

  | Path | What it is |
  | --- | --- |
  | `config.json` | portable runtime settings ([`brain config`](#3-configuration)) |
  | `personalization.json` | who you are ([`brain personalize`](#4-personalize-brain)) |
  | `extensions/<skill>.md` | your tweaks to a bundled skill ([hooks](#6-extend-a-skill-with-hooks)) |
  | `plugins/<name>/` | your own whole skills ([plugins](#7-add-a-whole-skill-plugins)) |

- **brain env and workspace registry** — one fixed, machine-local file,
  `~/.config/brain/env.json`. Schema v2 stores the default workspace and a
  record keyed by each canonical workspace name. Each record contains its root,
  aliases, stable workspace ID, local user ID, receiver state, and fully siloed
  machine-local env values. The file lives **outside** every workspace root, so
  it never syncs with workspace content.

  | Field | What it is |
  | --- | --- |
  | `root` | registry-owned path for this workspace; visible through `brain env get root`, but read-only there |
  | `markdown_to_pdf_path` | path to the `markdown-to-pdf` binary on this machine |
  | `claude_cmd` | command used to launch Claude on this machine |
  | `codex_cmd` | command used to launch Codex on this machine |
  | `sync` | Backblaze B2 sync config (bucket, credentials, trigger flags) |

  The rule of thumb: **wrong if synced → brain env; right everywhere → brain
  config.**

**Workspace roots and selectors.** Roots are structural registry fields, not
writable env variables. Use workspace commands to register roots and manage
names, aliases, or the default:

```sh
brain workspace create --name family --root ~/family
brain workspace alias add family fam
brain workspace default family
brain sync -b fam
```

A legacy `~/.config/brain-root` one-line pointer file is still read for
back-compat and is automatically, idempotently folded into the first schema-v2
workspace during migration. After migration, `brain workspace create` and
`brain workspace attach` are the supported ways to register roots.

## 3. Configuration

`config.json` is managed with `brain config` (hand-editing is fine too):

```sh
brain config list                        # every variable, value, description
brain config get calendar_id             # one value
brain config set calendar_id=me@work.com # set + persist (re-renders skills)
brain env set claude_cmd='claude --dangerously-skip-permissions'
```

| Variable | Default | Meaning |
| --- | --- | --- |
| `linear_workspace` | *(unset)* | Linear workspace slug; builds `https://linear.app/<slug>/issue/` for the task "open link" action. |
| `daily_triage_name_pattern` | `Morning Triage` | Regex on habit names that gates the startup triage nudge. Empty disables it. |
| `day_rollover_hour` | `6` | Hour (0–23) the "logical day" rolls over for the triage re-check. |
| `agenda_dir` | `~/Downloads` | Where the generated daily-agenda PDF is written. |
| `calendar_id` | *(empty)* | Calendar to pull busy blocks from when building the agenda. Empty = no calendar. |
| `skills_auto_sync` | `true` | When true, every `config`/`personalize` change re-renders + reinstalls your skills. Set false to sync only via `brain skills sync`. |

Names normalize (`-`→`_`, lower-cased), so `Linear-Workspace` works. Workspace
roots are registry-owned; agent launch commands are machine-local env values.

### `brain env`: machine-local values

`brain env` operates on the selected workspace's machine-local env map inside
`~/.config/brain/env.json`. The selected root is shown as a read-only virtual
value; structural registry fields cannot be changed through `brain env set`:

```sh
brain env list                 # every env variable, value, description
brain env get root             # selected workspace root (read-only)
brain env set markdown_to_pdf_path=/path/to/markdown-to-pdf
brain env set claude_cmd='claude --dangerously-skip-permissions'
brain env set codex_cmd='codex --model gpt-5'
brain env get root -b family
```

| Variable | Default | Meaning |
| --- | --- | --- |
| `root` | selected workspace root | Registry-owned and read-only through `brain env`. |
| `markdown_to_pdf_path` | *(auto-discovered)* | Path to the `markdown-to-pdf` command on this machine. |
| `claude_cmd` | `claude --dangerously-skip-permissions` | Command the Claude brain panel launches on this machine. |
| `codex_cmd` | `codex` | Command the Codex brain panel launches on this machine. |

### The `markdown-to-pdf` prerequisite

brain shells out to a `markdown-to-pdf` command. On first run it auto-discovers
one (your `PATH`, then `~/.local/bin`, `/usr/local/bin`, `/opt/homebrew/bin`,
`~/bin`, then your login shell) and remembers it **in brain env**, since it's a
machine-specific path. If the stored path is missing on *this* machine (e.g.
you set it up on another Mac), brain re-discovers automatically. Only if
nothing is found does it print a red error; fix it with
`brain env set markdown_to_pdf_path=/path/to/markdown-to-pdf`.

## 4. Personalize brain

Personalization is content *about you* that skills read to act as your assistant.
Manage it with `brain personalize`:

```sh
brain personalize                 # onboarding if unset, else shows your profile
brain personalize show            # the stable block skills read at runtime
brain personalize set role="CEO"
brain personalize edit            # open personalization.json in $EDITOR
```

| Field | Meaning |
| --- | --- |
| `name` | your display name |
| `role` | who you are ("CEO", "software engineer", "PhD student") |
| `works_for` | your org, "myself", or empty |
| `namespaces` | your project namespaces (e.g. `work`, `personal`) |
| `tag_styles` | how task tags render (label + emoji) |

`namespaces` and `tags` are edited with an interactive checklist:

```sh
brain config set namespaces       # toggle/add your project namespaces
brain config set tags             # toggle/add your task tags + styles
```

Skills don't bake in your identity — they call `brain personalize show` at
runtime. So updating your profile updates every skill's behavior at once, and the
brain repo itself stays 100% generic (no personal data committed anywhere).

## 5. Skills

brain ships a set of **generic, agent-ready skills** compiled into the binary.
`brain skills sync` renders them (injecting your [extensions](#6-extend-a-skill-with-hooks))
and installs them into the shared agent-skill registry (`~/.agents/skills/`),
fanning out to every installed frontend — Claude, Codex, OpenCode, Cursor — so
they work in *any* session, not just brain's own panel.

```sh
brain skills sync                 # render + install all skills (+ your plugins)
brain skills sync --root /tmp/sbx # install into a sandbox dir (for trying things)
```

With `skills_auto_sync` on (the default), this also runs automatically after any
`brain config` / `brain personalize` change, so your installed skills never drift
from your settings.

**Bundled skills** (all generic; your machine renders them with your personal
touches):

| Skill | Hooks it offers |
| --- | --- |
| `article-summarizer` | *(none — fully generic)* |
| `brain-knowledge-capture` | *(none)* |
| `contacts` | `contacts:fallback` |
| `second-brain` | `second-brain:company-context`, `second-brain:reference-manager` |
| `todo` | `todo:linear`, `todo:linear-backlog`, `todo:calendar`, `todo:cutoff`, `todo:anchors` |
| `triage` | `triage:daily-open`, `triage:daily-linear`, `triage:weekly-inboxes`, `triage:weekly-linear` |

There are **two ways to make skills yours without forking**: *extensions* (tweak a
bundled skill) and *plugins* (add a whole new skill). Both are stored with your
brain and never committed to the repo.

## 6. Extend a skill with hooks

An **extension** injects your own content into a bundled skill at named points
the skill declares — without changing a whole skill or touching the repo. The
injection happens only in the *installed copy* that agents read; the bundled
source is never modified.

A bundled skill marks its extension points with HTML-comment markers:

```markdown
<!-- brain:ext todo:calendar -->
```

You supply the content in `<brain-root>/.config/extensions/<skill>.md`, as
`[hook-name]` sections:

```markdown
# ~/brain/.config/extensions/todo.md

[todo:calendar]
When building the agenda, pull busy blocks from my Google Calendar and leave
those slots free.

[todo:anchors]
Always anchor 7:00am "Walk the dog" and 6:00pm "Gym" into the day.
```

On the next sync, each `[hook]`'s content replaces the matching marker in the
installed `todo` skill. Rules:

- Text **before the first `[hook]`**, and any hook that doesn't match a marker,
  is appended under a trailing **"## Personal extensions"** section — so nothing
  you write is ever silently dropped.
- A marker with no matching hook is removed, leaving the skill clean.
- The available hooks per skill are in the [table above](#5-skills); the
  authoritative list is the repo's `skills/<name>/SKILL.md` (grep for
  `brain:ext`).

## 7. Add a whole skill (plugins)

A **plugin** is a complete skill you own, installed alongside the bundled ones by
the same pipeline. Drop it at `<brain-root>/.config/plugins/<name>/`:

```
~/brain/.config/plugins/my-skill/
  SKILL.md          # required — the skill itself
  scripts/…         # optional supporting files
```

Run `brain skills sync` (or just change any config, with auto-sync on) and it
installs into `~/.agents/skills/my-skill` and fans out to every frontend, exactly
like a bundled skill. This is how you keep private/company-specific skills (e.g. a
Linear or Zotero integration) without putting them in the public repo.

## 8. Syncing across machines

Because each workspace's **brain config** (`config.json`,
`personalization.json`, `extensions/`, `plugins/`) lives under that root's
`.config/`, it rides along with whatever syncs the workspace. The schema-v2
registry and **brain env** (`~/.config/brain/env.json`) deliberately do not. The
registry contains machine-local roots, and each workspace env can contain local
binary paths or credentials.

On a new machine:

1. get the workspace root onto the new machine (however you sync it),
2. run `brain workspace attach ~/wherever` to register that existing root,
3. run `brain` (or select it with `-b`) to install skills from its synced
   extensions and plugins.

The machine registry should remain local. Portable configuration travels inside
each workspace; machine-local roots, receiver state, agent commands, and
credentials do not.

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

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
- The `claude` CLI, if you want the brain panel (an in-TUI Claude session).

**Point brain at your brain**

Your "brain" is a [PARA](https://fortelabs.com/blog/para/) directory —
`projects/`, `areas/`, `resources/`, `archive/`, plus `tasks/`. By default brain
uses `~/brain`. To keep it elsewhere, write the path into `~/.config/brain-root`
(see [Where brain keeps things](#2-where-brain-keeps-things)).

**First run**

Run `brain`. If you've never personalized it, a short, skippable prompt asks for
your name, role, and who you work for, then installs the bundled skills. That's it
— brain works fully even if you skip every prompt.

## 2. Where brain keeps things

Everything brain persists lives in **one dir inside your brain root**,
`<brain-root>/.config/` (e.g. `~/brain/.config/`):

| Path | What it is |
| --- | --- |
| `config.json` | runtime settings ([`brain config`](#3-configuration)) |
| `personalization.json` | who you are ([`brain personalize`](#4-personalize-brain)) |
| `extensions/<skill>.md` | your tweaks to a bundled skill ([hooks](#6-extend-a-skill-with-hooks)) |
| `plugins/<name>/` | your own whole skills ([plugins](#7-add-a-whole-skill-plugins)) |

Because this dir lives **inside your brain**, it travels with it: whatever syncs
your `~/brain` across machines syncs your config, personalization, and
customizations too. Nothing external (no dotfiles tool) is involved.

**The one exception — `root`.** The location of the brain root can't be stored
*inside* the brain root (you'd need to know the root to find the setting that
tells you the root). So it's resolved separately, and is **not** a `brain config`
value:

1. the path in `~/.config/brain-root` (a one-line file, `~` allowed), if present;
2. otherwise the default `~/brain`.

`~/.config/brain-root` is the *only* machine-local piece of brain state. Edit it
by hand:

```sh
echo '~/notes/brain' > ~/.config/brain-root   # keep your brain somewhere else
```

## 3. Configuration

`config.json` is managed with `brain config` (hand-editing is fine too):

```sh
brain config list                        # every variable, value, description
brain config get calendar_id             # one value
brain config set calendar_id=me@work.com # set + persist (re-renders skills)
brain config set claude-cmd              # bare name → prompts interactively
```

| Variable | Default | Meaning |
| --- | --- | --- |
| `linear_workspace` | *(unset)* | Linear workspace slug; builds `https://linear.app/<slug>/issue/` for the task "open link" action. |
| `markdown_to_pdf_path` | *(auto-discovered)* | Path to the `markdown-to-pdf` command. Self-heals if a synced value is wrong on this machine. |
| `daily_triage_name_pattern` | `Morning Triage` | Regex on habit names that gates the startup triage nudge. Empty disables it. |
| `day_rollover_hour` | `6` | Hour (0–23) the "logical day" rolls over for the triage re-check. |
| `agenda_dir` | `~/Downloads` | Where the generated daily-agenda PDF is written. |
| `calendar_id` | *(empty)* | Calendar to pull busy blocks from when building the agenda. Empty = no calendar. |
| `claude_cmd` | `claude --dangerously-skip-permissions` | Command the brain panel launches; brain appends `--resume`/`--session-id`. |
| `skills_auto_sync` | `true` | When true, every `config`/`personalize` change re-renders + reinstalls your skills. Set false to sync only via `brain skills sync`. |

Names normalize (`-`→`_`, lower-cased), so `Linear-Workspace` works. `root` is
**not** here — see [above](#2-where-brain-keeps-things).

### The `markdown-to-pdf` prerequisite

brain shells out to a `markdown-to-pdf` command. On first run it auto-discovers
one (your `PATH`, then `~/.local/bin`, `/usr/local/bin`, `/opt/homebrew/bin`,
`~/bin`, then your login shell) and remembers it. If the stored path is missing
on *this* machine (e.g. your `config.json` synced from another Mac), brain
re-discovers automatically. Only if nothing is found does it print a red error;
fix it with `brain config set markdown_to_pdf_path=/path/to/markdown-to-pdf`.

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

Because your config, personalization, extensions, and plugins all live under
`~/brain/.config/`, they ride along with whatever syncs your brain directory
(cloud drive, git, etc.). On a new machine:

1. get your `~/brain` there (however you sync it),
2. set `~/.config/brain-root` if your brain isn't at `~/brain`,
3. run `brain` — it installs the skills from your synced extensions/plugins.

Machine-specific values that happen to sync (like `markdown_to_pdf_path`)
self-heal on the machine that needs them.

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

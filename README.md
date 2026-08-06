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
remember a dozen. So `brain` is the front door: a persistent shell with three
main views (tasks, brain-directory search, and logs) and a live agent
brain panel, plus Finder / `$EDITOR` handoffs for files. Adding a capability
means adding a palette row or a keybinding, not another command to memorize.

## Usage

```sh
brain                 # persistent shell, tasks view (Claude brain panel)
brain --codex         # same shell, with Codex in the brain panel
brain -cx            # short alias for --codex
brain --open-code     # select the OpenCode stub; exits unsupported before startup
brain -oc             # short alias for --open-code
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

`brain` is a single Rust binary with a persistent TUI and short-lived command
families. It has no shell-mutating one-shot commands, so it needs no wrapper:
`run.sh` builds it when the sources change and `exec`s it directly, forwarding
args. The TUI renders to `/dev/tty` and performs its own file-open,
Finder-reveal, and agent-launch actions by spawning processes. The intentional
stdout families are `config/env/version`,
`workspace list`, explicit plain-task output, and help. `--verbose` mirrors logs
to stdout for non-TUI commands. Clap errors and diagnostics go to stderr. The
TUI renders to `/dev/tty`. Details: [docs/architecture.md](docs/architecture.md)
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

OpenCode is selectable with `--open-code` / `-oc`, but it is currently a
fail-fast stub. Brain does not run an `opencode` process, inspect OpenCode
sessions, install OpenCode hooks, or deliver OpenCode receiver responses.

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
the selector to use the default. The global selector works before or after a
subcommand, so `brain -b family sync` and `brain sync -b family` select the same
workspace. Names and aliases are trimmed and lower-cased, then must match
`[a-z0-9][a-z0-9_-]*`.

The first workspace becomes the default. Later creates and attaches preserve
that choice. `brain workspace default <name>` changes only where future
commands with no selector route. Changing the default workspace never changes
access mode, workspace identity, root, local user, receiver enablement, or env.

The complete implemented management surface is:

```sh
brain workspace list
brain workspace create --name family --root ~/family
brain workspace attach ~/shared-brain
brain workspace rename family household
brain workspace alias add household fam
brain workspace alias remove household fam
brain workspace default household
brain workspace remove household
brain workspace repair -b brain --manifest --local-user-id primary-user
brain workspace migrate -b brain --acknowledge-all-machines-updated
brain sync -b fam                  # aliases work for ordinary commands
```

Omit a management value to use the guided `/dev/tty` prompt. `attach` adopts
the stable identity in the existing root. `rename` preserves that identity and
updates the default name when necessary. `remove` detaches only the
machine-registry record; it never deletes the root or its contents.

**First run**

Run `brain`. If the selected workspace has no required machine-local user ID,
the readiness prompt asks for it and then continues the original command. The
separate personalization prompt for name, role, and organization remains
skippable. Brain then installs the bundled skills.

## 2. Where brain keeps things

Brain silos each workspace's persisted state, configuration, and runtime
artifacts. One machine registry says which workspaces this binary can select;
portable files stay inside their root, and runtime files use the stable
workspace UUID rather than a name or default.

> **Security boundary:** `workspace_only` is advisory prompt enforcement plus
> best-effort capability filtering, easy to bypass, and not tenant isolation.
> Use separate operating-system accounts, machines, VMs, or containers for real
> isolation.

| Boundary | Location | What belongs there | Portable? |
| --- | --- | --- | --- |
| Workspace-owned data | `<workspace-root>/` | Notes, tasks, skills customizations, and `.config/{workspace.json,users.json,config.json,personalization.json,extensions/,plugins/}` | Yes, it travels with that workspace |
| Machine registry | `$XDG_CONFIG_HOME/brain/env.json` (fallback `~/.config/brain/env.json`) | Schema, canonical default, and each workspace's UUID, machine-local root, aliases, `local_user_id`, `receiver_enabled`, and siloed free-form `env` | No |
| Workspace runtime/cache | `~/.cache/brain/workspaces/<workspace-uuid>/` | `state.db`, `tui.lock`, `inbox/`, `responses/`, and `sync/` locks, journal, current state, workdir, and CSV baselines | No |
| Shared infrastructure | Machine server PID/control files and the current shared triage signal | Narrow process coordination only; habits payloads are selected by request UUID | No |

Active run logs remain under `/tmp` through `logging.rs`. The literal read-only
`brain server status` and `brain receiver status -b <workspace>` probes are the
exception: they create no run log, skill render, render stamp, config repair,
or server state. Receiver status obtains process and exact-workspace lease facts
from one generation-bound control response. Both status requests use immutable
lease projections and never prune or advance lifecycle state. Registration,
heartbeat, enablement, unregister, ingress lookup, and routing availability
opportunistically discard expired leases. The watchdog provides periodic
expiry and guarantees final crashed-lease shutdown when no traffic arrives.
`WorkspacePaths::logs_dir` is reserved and unused; it is not the destination
for current diagnostic logs.

### Shared server and receiver lifetime

Brain runs at most one machine-wide HTTP process, and only while one or more
workspace TUIs are live. A ready TUI binds its UUID-local job socket, wins or
joins the shared-process election, registers its workspace lease, and renews
that lease with heartbeats. An orderly final close unregisters and stops the
process immediately. A crashed final TUI stops renewing, so the watchdog
removes its lease and stops the process after the five-second TTL.

Every public route begins with the portable opaque ingress in
`/w/<ingress>/...`. Brain resolves that ingress to an enabled, live lease before
it selects a root, provider credential, portable user, prompt, log scope, or job
socket. An accepted message is acknowledged only after one in-memory enqueue
in that exact TUI. There is no durable inbound queue, replay worker, detached
agent, manual server start/kill/restart command, or always-on responder.

Habits and triage completion are local-only actions, not public ingress
surfaces. Their `/local/<exact-live-lease>/w/<ingress>/...` URLs carry the
ephemeral lease capability accepted by that TUI, so a peer workspace lease
cannot read or mutate the selected workspace.

If every TUI is closed, no server exists and an inbound text receives no Brain
response. If another workspace TUI keeps the shared process alive but the
target workspace is disabled, closed, expired, full, or unreachable, the
sender receives one unavailable response and the message is discarded.

The portable manifest is
`<workspace-root>/.config/workspace.json`. It contains the stable workspace UUID,
receiver ingress UUID, manifest schema, and minimum compatible Brain version.
It is strict and create-only: Brain refuses unknown fields, incompatible
versions, or an identity mismatch, and never silently replaces an existing
identity. A second machine can therefore attach the synced root without
inventing a different UUID.

The sole registry has this strict schema-v2 shape:

```json
{
  "schema_version": 2,
  "default_workspace": "brain",
  "workspaces": {
    "brain": {
      "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
      "root": "/Users/example/brain",
      "aliases": ["personal"],
      "local_user_id": "primary-user",
      "receiver_enabled": false,
      "env": {}
    }
  }
}
```

Portable people live separately in `<workspace-root>/.config/users.json`.
Their lower-case kebab IDs identify people, so the same person may select the
same ID on multiple computers. `local_user_id` selects that person only for
local work on the current machine; an authenticated inbound phone or email
mapping overrides it for that request. There is no separate device, owner,
creator, or audit-history identity.

Task and habit rows use `assigned_to` for that portable person and immutable
`task_uuid` values for merge identity. Readers temporarily accept the legacy
`assignee` heading, but every write emits `assigned_to`. `T###` and `H###`
remain mutable display IDs: UUID-distinct rows survive a two-machine collision,
then reconcile labels and relationships deterministically.

Records never inherit or merge env values. The rule of thumb is: **wrong if
synced means brain env; right everywhere means brain config.** `root` is
registry-owned and read-only through `brain env`; local executable paths,
frontend commands, provider credentials, and sync transport settings live in
the selected record's `env` object.

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

Migration uses a legacy flat `root`, then that read-only pointer, then
`~/brain` only as compatibility inputs. Existing installs become one default
canonical workspace without losing machine env; an existing portable manifest
supplies its identity. A valid schema-v2 registry remains byte-for-byte
unchanged. Ordinary selected-workspace startup and selected
`brain workspace repair` validate or seed only that selected root's portable
access mode; they never inspect another registered root. `brain workspace
list` and an explicit whole-registry migration check validate or seed every
registered root. Fresh ordinary or repair startup synthesizes the compatible
`brain` workspace before readiness repair, while a first explicit create or
attach establishes exactly the requested workspace. Interactive ordinary
commands ask for missing required setup and continue; headless commands print
exact `brain workspace repair` instructions.

### Agent access and workspace boundaries

The foundation currently isolates workspace selection, portable stores, and
UUID-scoped runtime paths. Env, config, personalization, state, TUI, tasks,
reindex, sync, and Brain-owned children all receive one immutable selected
`CommandContext` / `WorkspaceContext`. Selection happens once. Ordinary runtime
code does not reopen the registry or consult a global root. Detached Brain
children carry the canonical `--brain` name, and child integrations receive
`BRAIN_WORKSPACE_ID`, `BRAIN_WORKSPACE`, `BRAIN_ROOT`, and `BRAIN_ACTOR_ID`.

`workspace_only` is advisory prompt enforcement plus best-effort capability
filtering, easy to bypass, and not tenant isolation. It is intended only to
reduce accidents and naive cross-workspace leakage among trusted users. Real
adversarial or sensitive isolation requires an external OS, VM, machine, or
container boundary.

Brain implements this advisory mode with trusted frontend instructions,
selected-workspace capability filtering, a minimal child environment, and the
selected workspace root as the child working directory. Claude and Codex keep
using the user's shared frontend login; Brain does not create a separate
credential identity for a workspace. `brain skills status` distinguishes
strictly selected capabilities from advisory-only and unavailable ones instead
of treating logical selection as proof of enforcement. The migrated/default
workspace remains unrestricted unless its portable access policy explicitly
configures it otherwise. Changing the default workspace never changes access
mode.

The `AgentController` facade and advisory access controls are active. TUI and
receiver callers use semantic launch, type, submit, queue, new-session,
completion, transcript, terminal, and shutdown operations. Claude and Codex
adapters translate those operations into their own commands, input sequences,
resume rules, and hook behavior. OpenCode is represented only by a
constructible fail-fast adapter; every operation returns unsupported before a
PTY or child process is touched. Functional OpenCode sessions remain a later
phase. The shared receiver lease, ingress, authentication, forwarding, and
delivery lifecycle is active.

Task-schema activation is available only through the explicit, selected
`brain workspace migrate` command. It checks compatibility, portable-user
mappings, remote identity, and the all-machines acknowledgement before it
creates a UUID-scoped journal or mutates portable data. When sync is configured,
the final legacy semantic sync completes first. Brain then keeps an exact
machine-local backup, resumes after the last verified step on retry, migrates
task identity, reconciles triage, rebuilds derived data, and verifies the whole
workspace before removing the active journal. The retained backup and recovery
commands are reported on failure. Ordinary startup, readiness, and sync never
activate this migration.

Cloud sync is optional per workspace and reads only that workspace's machine
record. Every remote-writing path first validates the remote portable manifest
against the selected UUID. A mismatch, malformed manifest, incompatible schema,
or unreadable manifest fails closed. Setup can initialize an empty remote. A
nonempty manifestless remote requires explicit interactive confirmation or
`--adopt-workspace-id <exact-selected-uuid>`; `--yes` alone is never ownership
authority. Locks, journals, current state, rclone workdirs, semantic CSV
baselines, freshness, and watcher state are all derived from the workspace UUID,
so different workspaces may sync concurrently while one workspace stays
serialized.

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
| `enable_triage_habits` | `true` | Maintain protected daily and weekly triage chains. Setting `false` transactionally purges every managed occurrence and derived reference while leaving manual `/triage` available. |
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
brain env set opencode_cmd='opencode' # reserved; the stub never executes it
brain env get root -b family
```

| Variable | Default | Meaning |
| --- | --- | --- |
| `root` | selected workspace root | Registry-owned and read-only through `brain env`. |
| `markdown_to_pdf_path` | *(auto-discovered)* | Path to the `markdown-to-pdf` command on this machine. |
| `claude_cmd` | `claude --dangerously-skip-permissions` | Command the Claude brain panel launches on this machine. |
| `codex_cmd` | `codex` | Command the Codex brain panel launches on this machine. |
| `opencode_cmd` | `opencode` | Reserved OpenCode command value. The selectable stub never executes it. |

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
| `triage` | `triage:daily-open`, `triage:daily-subagents`, `triage:daily-linear`, `triage:daily-merge`, `triage:daily-required-outputs`, `triage:weekly-inboxes`, `triage:weekly-linear` |

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

The bundled `todo` skill also exposes `todo:agenda-after-build`. It is a
generic, no-op-by-default seam for caller-supplied post-build steps. An
extension that uses it must supply its own content and paths at runtime; the
bundled skill does not discover private artifacts or assume a particular
external service.

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

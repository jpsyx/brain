# Architecture

`brain` is a small Rust CLI that browses one selected PARA-organized workspace
(projects / areas / resources) and acts as the single terminal entry point for
the user's knowledge and task workflows.

As of the tasks↔brain merge, `brain` is the single CLI for both the second
brain and the task system; the standalone `tasks` binary is gone. Its two
execution surfaces are a persistent TUI and short-lived command families:

- **Bare `brain`** (and interactive `brain tasks …` routes) opens a
  **persistent shell**
  (`tui/`) with **three main views**: the **tasks view** (task management,
  agenda, triage; the startup default) and the **brain-directory search view**
  (fuzzy-pick over the selected root), and the **logs view** (scrollable
  diagnostics), plus one app-level **brain panel** (an
  interactive agent session in a PTY). You switch main views with
  `Ctrl+L`/`Ctrl+H` (cycle) or `Ctrl+T`/`Ctrl+B` (jump); the brain panel
  persists across a switch and closing it makes the main view full-width. The
  process owns the terminal until you quit and keeps a little SQLite state so
  it resumes the right Claude session and remembers the panel layout. See
  [glossary.md](glossary.md) for the main-view / sub-view / panel vocabulary.
- **Short-lived command families** cover non-TUI task utilities, config, env,
  workspace, portable users, sync, personalization, skills, server/receiver,
  habits, checks, and reindexing. They mutate or report through their focused handlers, then
  exit without opening the persistent shell. `brain tasks complete`,
  `brain tasks doctor`, and `brain tasks --no-tui` are short-lived;
  `brain tasks search` opens the persistent TUI.

There are **no** shell-mutating one-shot commands: no `cd`, `msg`, or
per-bucket search subcommand, and no freeform note search. Interactive
file-open, Finder-reveal, PDF, trash, and agent-launch actions happen inside
the persistent shell by spawning processes. The binary therefore needs no
parent-shell cooperation, wrapper, or plan protocol.

## One binary, run directly

```
user types `brain …`
  └─→ run.sh
       ├─ rebuilds the binary if any src/*.rs (or Cargo.toml) is newer
       │    (cargo build --release; build chatter → stderr)
       └─ exec target/release/brain "$@"   (forwards every argument)

the binary:
  ├─ every run → writes a timestamped `/tmp` log; `--verbose` mirrors logs to stdout
  ├─ help / version → print and exit without opening the TUI
  ├─ tasks complete / doctor / --no-tui → mutation, health check, or plain output
  ├─ tasks search → opens the persistent TUI with a custom task search
  ├─ config / env / workspace / user → selected configuration or registry management
  ├─ sync / personalization / skills → focused setup, reporting, or mutation
  ├─ server / receiver / habits / checks / reindexing → focused handlers
  └─ bare brain and other interactive task routes → persistent TUI on /dev/tty
```

Help and version exit without opening the TUI. After these explicit exits,
bare `brain` and interactive task routes open the persistent TUI.

The intentional stdout families are `config/env/version`, `workspace list`,
explicit plain-task output, and help. `--verbose` mirrors logs to stdout for
non-TUI commands. Clap errors and diagnostics go to stderr. The TUI renders to
`/dev/tty`; TUI runs keep stdout quiet and expose the log through the tasks
command palette. The binary
opens files, cds its own PTY, launches `claude`, and reveals in Finder itself,
from inside the running shell. See [decisions.md](decisions.md) for why Brain
needs no wrapper or plan protocol, and [integrations.md](integrations.md) for
the launch/handoff detail.

## High-level data flow (inside the binary)

```
argv
 └─→ Cli::parse                          (cli/)
      ├─→ help / -v / --version / Cmd::Version
      │    └─→ print and exit before workspace bootstrap or command gates
      ├─→ logging::init                  (timestamped `/tmp` log; stdout mirror with `--verbose`)
      ├─→ workspace::bootstrap            (explicit per-invocation policy)
      │    ├─ context-free/internal → no registry, root, or prompt
      │    ├─ create/attach/remove/repair → registry capability only
      │    └─ ordinary command → migrate only without a valid v2 registry,
      │         select once, validate readiness, repair interactively,
      │         return CommandContext
      └─→ command::dispatch::run          (focused handlers under command/)
           ├─→ configuration / sync / server / workspace / reindex handlers
           └─→ settings::ensure_markdown_to_pdf (tasks/TUI prerequisite gate)
           ├─ no subcommand ─────────→ tasks_launch(default view) → tui::run_tui (MERGED SHELL, tasks view)
           └─ Cmd::Tasks(rest)       ─→ TasksCli::parse_from(rest) → tasks_launch:
                                          complete → complete::run (native CSV completion)
                                          doctor   → doctor::run_doctor
                                          --no-tui → plain::print_plain
                                          search   → CustomSearch → tui::run_tui
                                          else     → tui::run_tui (MERGED SHELL)

tui::run_tui(command_context, view, cli, …) (the persistent shell)
 ├─→ command_context.workspace.root()       (immutable selected root snapshot)
 ├─→ build_search(brain_root)                (entry::collect over all buckets → picker::App)
 └─→ App event loop (tasks view + search view + agent PTY)
       ├─ agent::SessionStore: reap dead locks, scoped resume / claim or register
       ├─ App owns one AgentController per live main/triage panel
       │    ├─ access::AccessPolicy snapshots trusted portable mode/root/actor
       │    ├─ agent::{ClaudeFrontend,CodexFrontend} translate semantic operations
       │    ├─ agent::OpenCodeFrontend rejects every operation before transport access
       │    └─ PtyPane clears inherited env, spawns the complete spec, and carries bytes
       ├─ Ctrl+L/H cycle views, Ctrl+T/B jump; Alt+H/L switch panel focus
       ├─ Ctrl+P opens a command palette (tasks: tui::palette; search: menu::MenuApp; status and log actions open the logs view)
       ├─ Enter on a file opens it in place (open_target spawners) — shell stays up
       └─ quit → the loop just returns (no plan, no wrapper handoff)
```

The task and brain-search views share the pure picker logic (`picker::App` matching /
navigation, rendered via `picker::draw_into`) and the `menu` palette in the
search view. The search panel lives in a bordered half of the shell alongside
the live brain panel; opening a file or rescoping a bucket happens in place
and the shell stays up.

## Multi-workspace foundation boundary

The foundation separates workspace payload and runtime state along these
boundaries:

| Owner | Location | Examples |
| --- | --- | --- |
| Portable workspace | `<workspace-root>/` | Notes, tasks, `.config/workspace.json`, `.config/users.json`, config, personalization, extensions, and plugins |
| Machine registry | `$XDG_CONFIG_HOME/brain/env.json` (fallback `~/.config/brain/env.json`) | Schema-v2 default plus each canonical record's UUID, root, aliases, local user, receiver switch, and siloed env object |
| Workspace runtime | `~/.cache/brain/workspaces/<workspace-uuid>/` | State DB, TUI lock, portable-user transaction lock, inbox, responses, capability artifacts, and sync lock/journal/current state/baselines |
| Shared infrastructure | `~/.cache/brain/server/` plus the transitional triage-signal path | Generation-tagged process coordination and an infrastructure-only log, never a default workspace payload path |

One bootstrap resolves an immutable `CommandContext` / `WorkspaceContext`.
Env, config, personalization, state, TUI, tasks, reindex, sync, and child
integrations consume that selection or a path derived from it. Ordinary
runtime code does not reopen the registry or resolve a global root. Detached
Brain children carry `--brain <canonical-name>`; integrations receive
`BRAIN_WORKSPACE_ID`, `BRAIN_WORKSPACE`, `BRAIN_ROOT`, `BRAIN_ACTOR_ID`, and
`BRAIN_CHANNEL`, with agent-session values added separately.

Active run logs remain under `/tmp` through `logging.rs`.
`WorkspacePaths::logs_dir` is reserved and unused; current diagnostic logs do
not use that UUID-scoped path.

The frontend-neutral `agent` facade, concrete Claude/Codex adapters, PTY
transport, main and triage controller ownership, receiver controller dispatch,
advisory portable access modes, and a fail-fast OpenCode selection stub now
exist. Functional OpenCode sessions, coordinated task-schema activation, and
the final shared receiver lease lifecycle remain later phases.
`workspace_only` is easy-to-bypass prompt guidance plus capability filtering,
not a security or isolation boundary. It reduces accidents and naive leakage
among trusted users; adversarial or sensitive workloads require an external
OS, VM, machine, or container boundary. Changing the machine default never
changes portable access mode. The controller accepts a workspace-only launch
only when its capability plan has the same access mode and selected workspace
UUID; unrestricted launches carry no plan and do not parse capability
configuration. Those are the only accepted access contexts. Unrestricted with
any plan, workspace-only without a plan, and mismatched workspace-only plans
all fail before frontend or transport work.

## Modules

### `main.rs`
The binary entry point is intentionally thin: parse, context-free version exit,
logging, one workspace bootstrap, one dispatch call, and one top-level error
boundary. It links the library modules instead of declaring a duplicate module
tree. `command/dispatch.rs` owns the exhaustive `Cmd` routing, while focused
`command/{configuration,tasks,sync,server,workspace,users,reindex}` modules own the
existing handlers. `command/server/` further separates receiver setup, HTTP
server lifecycle, and habits dispatch. Receiver command ownership is reflected
on disk: `receiver/mod.rs` owns dispatch and provider setup, while
`receiver/hooks.rs` owns workspace-sensitive Claude/Codex hook installation;
its focused installer tests live in the owned `receiver/hooks/tests.rs`
submodule.

### `cli/`
The clap derive surface, split by command family. `mod.rs` keeps the parser
entry, public re-exports, and the small top-level `Cmd`; `global`,
`configuration`, `tasks`, `sync`, `server`, `workspace`, and `users` own their focused
arguments. All former `crate::cli::*` type paths remain stable. `Cli` owns the
global flags, including raw `--brain/-b <workspace>` selection, plus one
optional `Cmd`. The shared real/test parser normalization extracts that selector
before Clap's delegated `tasks` tail can capture it, including after a task
positional; `--` stops extraction. Clap retains the exact raw selector, and
registry-aware dispatch resolves it case-insensitively as a canonical name or
alias. Bare `brain` remains equivalent to `brain tasks`.

### `logging.rs`
Per-run logging. `logging::init` always creates a timestamped
`/tmp/<rfc3339-nanos>.log` file, and `--verbose` mirrors log
lines to stdout for non-TUI commands, and prints the final log path at process
exit. Before the persistent shell takes over `/dev/tty`, `main.rs` disables the
stdout mirror; the TUI keeps the log path in `App` and offers receiver and brain
log actions in the command palette that switch the main panel to a log view.
the tasks command palette. Command handlers and thin IO shells call
`logging::log` at phase boundaries: dispatch, config/env/personalize actions,
task CSV loads and writes, sync/rclone work, server lifecycle probes, doctor
checks, and skill installation.

### `paths.rs`
Legacy single-root resolution only. `brain_root()` / `brain_root_path()` retain
the pre-migration flat-root, read-only pointer, and `~/brain` fallback boundary
needed to construct the first schema-v2 record. Ordinary commands do not use
them to select a workspace; bootstrap supplies an immutable
`WorkspaceContext::root()`. The IO-free compatibility pieces
(`parse_brain_root_file`, `expand_tilde_with_home`) remain unit-testable without
a real `$HOME` or pointer file. See [config.md](config.md).

### `workspace/`
The typed, selection-independent workspace foundation. `id` owns the immutable
UUID newtype, `name` validates canonical lower-case slugs, `context` owns an
already-resolved root and the machine's local user ID, and `paths` derives every
machine-local runtime path from the immutable UUID. `registry/` owns the
versioned machine registry, split by responsibility into `model` (schema and
validated mutations), `validate` (pure whole-registry invariants), `select`
(borrowed canonical/default/alias resolution), `store` (the fixed registry
path, lock-owning transactions, and same-directory atomic replacement), `lock`
(the bounded SQLite transaction lock on the stable adjacent database),
and `migrate` (the one-time flat-env conversion and exact-byte backup).
`bootstrap_policy` classifies every invocation before workspace IO, while
`bootstrap` executes that policy. Context-free and hidden internal-server
routes cannot prompt. Create, attach, remove, and repair first run the
`command::preflight` prompt-and-validation stage; only a complete request may
trigger legacy migration and receive a registry capability. Ordinary commands
inspect the fixed registry path and enter legacy migration only when it is not
already valid schema v2; a valid registry avoids all legacy root/config lookup.
They then require a ready selected workspace. `readiness` is
the pure manifest/portable-membership/local-user decision,
`manifest` owns strict portable identity parsing, and successful bootstrap
returns one immutable `CommandContext` containing an `Arc<WorkspaceContext>`,
the request's resolved local `ActorContext`, and the registry store. Interactive repair uses injected `BufRead`/`Write`,
persists under the registry transaction, reloads, and continues the originally
requested command. Root-local stores take the context, machine-env writes also
take its exact `RegistryStore`, and the TUI retains the same `Arc` for watcher,
receiver, session, rendering, state, response, and sync paths. Brain-owned
children receive the typed workspace/actor integration environment.
Detached workspace-owned sync children additionally carry
`--brain <canonical-name>`. The detached shared-server child is the deliberate
exception: it owns only machine-shared lifecycle/control state and resolves
request payloads by workspace UUID, so it has no selected `--brain` argument.

### `actor/`

The immutable effective person for one request lineage. `resolve` uses the
machine's portable local user for interactive work, or an enabled normalized
phone/email identity after provider authentication. `context` carries the
validated person ID, display name, and initiating channel through queueing,
session lookup, hooks, task-agent prompts, completion, and response delivery.
When readiness admits a legacy workspace with no portable user file,
`local_actor` uses its exact lower-case kebab legacy local ID as an interactive
compatibility actor and writes nothing; the inactive portable migration remains
inactive. Readiness rejects every nonblank ID that the `UserId` parser would
reject, so actor bootstrap cannot discover a weaker legacy acceptance rule.

### `agent/`

The frontend-neutral agent boundary. `controller` owns the semantic
`AgentController` facade and the transport trait, so callers can type, submit,
queue, start sessions, launch, inspect completion and transcripts, snapshot,
and shut down without constructing frontend keystrokes. `frontend` defines the
frontend trait plus complete launch request and launch spec types; `input`,
`session`, and `hooks` own the shared input, validated session-plan, completion,
and hook metadata values. `session` owns the canonical `AgentKind` identity,
frontend-neutral `SessionStore`, immutable `SessionScope`, and durable
`CompletionStatus`;
the crate-level `session.rs` re-exports it and keeps adapter-backed command/env
wrappers for compatibility callers and pure tests. `claude` and `codex` own launch
syntax, input sequences, completion, transcript, and session-lifecycle rules.
`PtyPane` implements `AgentTransport`. The main panel and ephemeral triage tab
are both stored as `Option<AgentController>`; keyboard, receiver, draw, scroll,
close, and event-loop code call controller semantics and never construct
frontend keystrokes. The controller also owns the short delayed queue action
that keeps injected text separate from its final frontend input.
`opencode` supplies a constructible frontend whose lifecycle, input, session,
completion, and response operations all return typed unsupported errors before
transport access.
`LaunchRequest::HookMetadata` is trusted input that adapters merge into the
explicit child environment. The plan-mandated `LaunchSpec::hooks` slot is
currently reserved and empty; `PtyPane` does not consume a second hook channel.

### `access/`

Portable access policy. `mode` owns the two stable config values; `prompt`
builds the trusted advisory text and deliberately naive literal outside-root
warning; `capabilities` snapshots mode plus prompt; `skills` resolves portable
logical names against the selected machine record; `enforcement` models honest
frontend evidence and levels; `artifact` owns symlink-safe validation and
removal below the selected workspace's trusted UUID cache root; `mcp` owns the
machine schema plus frontend runtime translation; `store` strictly
loads portable config, preserves unrelated keys, and
publishes mode changes through a synced same-directory atomic replacement. It
also validates or seeds the selected record before readiness, a new record
before publication, and every record when listing or explicitly migrating the
whole registry. Main, receiver, resumed, fresh, and triage launches all construct policy
from the selected workspace, resolved actor, and already-loaded portable
`Config`. Main and triage launch paths attach the same resolved capability plan.
The controller validates the plan's mode and credential provenance before a
frontend can render artifacts or reach the transport. Unrestricted launch
assembly bypasses portable and machine capability parsing, preserving normal
frontend pass-through even when unused capability data is malformed. TUI setup
implements the same distinction before `App` construction: access mode and live
settings remain strict, but unrestricted mode does not deserialize the unused
logical capability lists.
Inbound prompt text is not an input to policy construction.
The main-panel launch path also resolves the capability plan and adapter-owned
response identity before claiming a free resumable session. A later controller
launch failure releases the instance claim and clears the attempted response
identity.

### `users/`

The strict schema-1 portable people registry at
`<workspace-root>/.config/users.json`. `id` validates exact lower-case kebab
person IDs; `normalize` canonicalizes unambiguous phone numbers and
case-normalizes email addresses without provider-specific rewriting; `model`
and `validate` reject unknown schema fields and ambiguous enabled identities;
`store` publishes canonical JSON through a same-directory atomic replacement;
`transaction` coordinates grouped registry and assignment changes with a
portable recovery journal plus a workspace UUID-scoped machine lock; and
`command` owns pure add/update/remove mutations plus the inactive legacy
conversion proposal. A pending grouped transaction restores the old generation
before the next portable-user load. The selected machine record's
`local_user_id` must name one member when this portable file exists. It
identifies a person, not a device, owner, creator, or authorization principal.
The same ID may be selected on multiple machines for that same person; there is
no cross-machine identity split or audit identity.

`command/` owns the workspace CLI: `mutate` turns collected values into pure,
validated registry-only decisions and owns registry-only mutations;
`preflight` applies and validates registry-only prompt answers before migration;
`provision` owns create/attach filesystem and persistence transactions; `list`
renders deterministic themed output; `prompt` collects omitted human values
from `/dev/tty`; and `mod.rs` stays focused on dispatch and re-exports.
Root normalization is pure:
it resolves a relative input against an explicit current-directory base and
removes lexical `.` / `..` components without requiring the path to exist or
canonicalizing symlinks. A relative root requires an absolute injected base;
otherwise context construction returns a typed error rather than storing a
relative root.

The sole machine-global registry is `$XDG_CONFIG_HOME/brain/env.json`, falling
back to `~/.config/brain/env.json`. Its schema version is exactly `2`; every
canonical workspace key owns one complete, siloed `WorkspaceRecord` (UUID,
root, aliases, local user identity, receiver switch, and machine environment
map). Selection borrows exactly that record and never copies or merges another
workspace's environment. Whole-registry validation rejects ambiguous selectors,
duplicate UUIDs, a missing default, and exact or ancestor/descendant root
overlap after absolute lexical normalization. `RegistryStore::transaction`
acquires the interprocess lock before any load and holds it through mutation,
whole-candidate validation, atomic persistence, live-value replacement, and
create failure reporting. Lock timeout errors retain the lock path, owner PID when
available, and wait duration. The stable adjacent lock database is never
unlinked; the existing `rusqlite` dependency holds `BEGIN IMMEDIATE` for the
transaction lifetime. The lock database remains zero-length: it has no schema
or data writes and uses `journal_mode=OFF`, so locking needs neither
initialization nor journal files. This also lets registry persistence report
its own permissions failure in a read-only parent. SQLite releases its OS lock
on normal close or process exit. A stable owner sidecar supplies the diagnostic
PID. Atomic replacement remains a separate persistence guarantee.

When bootstrap policy permits registry access, `registry::migrate` checks this
fixed file before ordinary command dispatch. A valid schema-v2 registry is
returned without any write. Otherwise
it converts the legacy flat object into exactly one default record, resolving
the root from flat `root`, then the read-only legacy pointer, then
`<home>/brain`; the result is tilde-expanded and lexically normalized without
requiring the directory to exist. The new record receives no aliases,
an empty local-user placeholder, a de-duplicated receiver switch, and all other
machine-local flat values inside its `env`. It creates the root and the first
matching portable manifest before persisting the registry. If that root already
contains a valid portable manifest, migration adopts its workspace UUID and
receiver-ingress UUID without changing its bytes. Only a missing manifest
causes migration to generate those identities. Access policy is
deliberately not machine-local; readiness remains incomplete until this
machine's local user ID is supplied.

Before replacing an existing flat file, migration creates an adjacent
exact-byte `env.json.legacy-backup` (or the first free numeric suffix), then
uses the atomic registry store. Re-running sees schema v2 and preserves both the
UUID and registry bytes without another backup. Registry-only create and attach
collect and validate their complete request before classifying the machine or
migrating. An existing
`env.json`, legacy `$XDG_CONFIG_HOME/brain-root` pointer, or `<home>/brain`
directory is legacy-install evidence and is migrated first. Only a genuinely
fresh machine with none of those sources lets the requested create/attach
become the first record. Workspace commands then load schema v2 explicitly and
never project through the compatibility view. `workspace create` creates only
its requested root after validating the complete registry candidate. It
tracks every missing root-directory component it creates. If later directory
creation or registry persistence fails, Brain never deletes those paths:
safe Rust 1.85 path APIs cannot atomically couple ownership verification with
deletion. A composed error keeps the original provisioning or persistence
error as its source and lists only the paths this invocation created, deepest
first, for manual inspection and cleanup. An `AlreadyExists` result is an
ownership race, not successful provisioning and is never added to that list.
`attach` requires an existing root with a strict compatible manifest and adopts
its UUID. Invalid manifests, duplicate UUIDs, and already-registered or
overlapping roots fail without changing registry bytes or root contents. Rename,
aliases, default changes, and removal use `RegistryStore` transactions. Adding
an alias already present on the same record is a typed error rather than a
successful no-op. Removal detaches only the record. Bootstrap resolves
`--brain` once into `CommandContext`; env writes revalidate canonical name plus
UUID under the registry transaction, and all ordinary config, personalization,
task, reindex, sync, receiver, and TUI paths consume that same selected context.
Legacy root helpers remain only inside the tested one-time migration boundary.

Deserialization has a single trusted boundary: JSON first enters a private raw
schema DTO, then conversion runs the same pure whole-registry validator used by
mutations. Public `Deserialize<MachineRegistry>` uses that conversion, so a
successfully deserialized value is fully valid. `RegistryStore` parses the raw
DTO itself so structural JSON failures retain operation and path context while
domain failures retain their typed `RegistryError` variants. Storage failures
likewise retain their operation, primary and related paths, IO error kind, and
message.

After selection, bootstrap constructs one `WorkspaceContext` before passing it
to ordinary commands. Context fields are private and accessors expose
only immutable views, so callers cannot desynchronize the workspace UUID from
its UUID-derived runtime paths.

### `settings/`
The persistent config store (`<brain-root>/.config/config.json`) and the
`brain config` command. Owns the raw JSON read/modify/write, the declared-
variable schema, get/set/list (with the aligned, colored `config list` table),
and the `markdown-to-pdf` prerequisite: auto-discovery (PATH → conventional bin
dirs → login-shell resolution of a function wrapper), validation, and the
fail-fast red-`❌` startup gate. Pure decision helpers (schema resolution, table
layout, message wording, shell-output parsing) are unit-tested; the IO shells
are thin. Split into `store` (JSON IO), `schema` (`VARS`/`Resolved`), `vars`
(get/set/resolve), `render` (the `config list` table), and `markdown_pdf` (the
prerequisite). See [config.md](config.md).

### `personalization/`
The personalization store — content *about you* at
`<brain-root>/.config/personalization.json` (beside the config store in
`settings::config_dir()`; it is just another brain config, inside the brain root
so it travels with the brain). Split into `model` (the `Personalization` schema +
parse), `store` (path resolution in the brain config dir + load/save), `tags`
(the `TagStyle`/`TagStyles` model, the
generic defaults `mit`/`personal`/`work`, and pure label resolution with
raw-name fallback), `runtime` (explicit selected-workspace style loading for
the TUI's retained `App` state), `command` (the
`brain personalize` show/get/set/edit logic — pure helpers + thin IO), and
`onboarding` (the skippable first-run prompt). The task renderer's
`type_label` delegates here, so the public binary carries no personal taxonomy.
See [config.md](config.md) and [data-model.md](data-model.md).

### `skills/`
The brain skill pipeline (sub-project B): render the bundled skills and install
them into the shared agent registry (`~/.agents/skills`), fanning out to each
frontend (Claude, **Codex**, OpenCode, Cursor). Split into `model` (the shared `Skill`/`SkillFile` type), `embed` (the
`include_dir!`-embedded `skills/` dir → bundled `Skill`s), `plugin` (whole user
skills discovered from `<root>/.config/plugins/<name>/`), `extension` (parse a
`<root>/.config/extensions/<skill>.md` into named `[hook]` sections + catch-all,
and `apply` it to a base `SKILL.md` at `<!-- brain:ext hook -->` markers,
producing a *new built copy* only — never the repo/plugin source; unmatched
content lands in a trailing "Personal extensions" section), `render` (base skill
→ installable files, injecting the extension into `SKILL.md`), `layout` (the
built dir + registry + frontend dirs, and the pure `link_ops` target
computation), `install` (collect bundled + plugins, write built + create the
two-hop symlinks; thin FS shell over `link_ops`), and `command`
(`brain skills sync [--root <sandbox>]`; `format_sync_plan` prints the built
dir, registry, frontend count, and extension/plugin sources before the FS shell
runs; `brain skills status` reports capability selection and enforcement).
For workspace-only launches, `layout` and `install` also render selected skills
under the workspace UUID and actor cache without creating registry or frontend
links. A machine skill is read only from its exact configured absolute
directory; the source directory, `SKILL.md`, and every descendant must be real
files or directories rather than symlinks. `resync_skills()` (the A seam) runs the pipeline, gated by
`skills_auto_sync` (**default `true`** since the B4 cutover) so a mutation
re-renders the live registry; set the flag `false` to manage skills only via
explicit `brain skills sync`. jpsyx delegates to `brain skills sync` and never
prunes brain-owned links (they resolve into brain's built dir, outside jpsyx's
sources). See the B spec under `docs/superpowers/specs/`.

**Version-stamped auto-resync.** So a version bump ships its *skill* changes the
way it ships *code* changes (immediately, no manual step), `bootstrap` calls
`skills::resync_on_version_change()` for every ready-workspace invocation. It
compares `env!("CARGO_PKG_VERSION")` to a per-workspace render stamp
(`state` DB `meta('skills_synced_version')`) and, when they differ, runs the
same pipeline once, then re-stamps (`needs_resync` is the pure decision). It is
LLM-free, gated by the same `skills_auto_sync` flag, never fails the invocation,
and is a no-op once stamped, so `--help`/`--version` (no workspace), the
internal hook/server, and registry-only maintenance never trigger it, and a
fork with no extensions renders identically. Every authoritative render path
(the version-resync, the mutation `resync_skills`, and a real `brain skills
sync`) writes the stamp so none re-fires redundantly; a `--root` sandbox sync
leaves no stamp.

The bundled `todo` workflow declares `todo:agenda-after-build` as a generic
no-op hook. If an installed extension adds a post-build step, that runtime step
owns every input and output path and passes optional markdown to the generic
helper explicitly. Core does not discover extension artifacts or external
service state.

### `entry.rs`
`Bucket` (Projects / Areas / Resources / Archive; declaration order =
display order, Archive last) and `Entry` (absolute selected-workspace `path`,
home-abbreviated `display`, `bucket`).
`collect()` walks each root with `walkdir`, skips hidden files
(`.`-prefixed) and the root itself, and tags every entry with its bucket.
Missing roots are silently skipped.

### `picker/`
The ratatui fuzzy picker. The `App` type lives in `picker/mod.rs` (so every
submodule reaches its private fields); the impl is split into `haystack`
(match preprocessing), `filter` (constructors + `refilter` + grouping), `nav`
(query edits + cursor + scroll), `selection` (highlighted-entry accessors +
palette/confirm openers), and `view` (`draw_into`). `App` **owns** its entries (so the persistent
shell can `set_entries` to rescope a bucket in place), precomputed
`HaystackBuf`s, the query, the current matches, and the interleaved
header/match `display_rows`. `refilter()` runs nucleo substring matching,
sorts matches by bucket then score then walk order, and rebuilds the
section-grouped rows. Navigation (`move_up`/`down`, `page_*`,
`ensure_visible`) keeps the cursor and its section header on screen.
Rendering is delegated to `render.rs` and exposed as `draw_into(f, app,
area)` so `tui`'s embedded search panel paints it. `App` also holds an
optional `palette: Option<menu::MenuApp>` overlay and an optional
`confirm: Option<confirm::Confirm>` overlay (routed before the palette) that
serves both the "Create PDF" and "Delete" confirmations. The `App` is driven
key-by-key by the search view (`tui/search_view.rs`): `Enter` opens the
selection in place (a directory falls back to a Finder reveal), `Ctrl-Enter`
reveals, `Ctrl-G` confirms a markdown→PDF conversion, `Ctrl-D` confirms a
trash, and a confirmed palette row runs its action. Every action happens in
place; the shell never tears down on a selection. On `Accept`, Delete trashes
the path and `drop_path`s the entry (`reload_entries` keeps the query), and
the picker stays open.

### `menu/`
Split into `labels` (contextual-row elision), `model` (`Choice`/`Targets`/the
row list/`shortcut_for`), `filter` (the substring matcher), `app` (`MenuApp` +
`handle_key`), and `view` (`draw_modal`).
The command palette (the top-level menu). It has **no screen of its own**:
the host opens it with `Ctrl-p`, drives its pure `MenuApp` + `handle_key`,
and paints it with `draw_modal` as a centered overlay. `Choice` enumerates
the rows; the row list is built per-open by `items(side, include_msg,
pdf_target, delete_target)` because the rows are contextual: the **layout
toggle** has a dynamic label (`layout_choice_label`: "Move brain panel to the
left" / "...right"), the **"Create PDF for '…'"** row (label via
`create_pdf_label`, which elides a long filename) leads the list only when
`pdf_target` is a highlighted `.md` filename, the **"Delete '…'"** row (label
via `delete_label`, which shares `create_pdf_label`'s ellipsis threshold via
`truncate_label_filename`/`LABEL_MAX_FILENAME`) **trails** the list when
`delete_target` is a highlighted entry of any kind (trailing, so a destructive
action is never the default-selected row), and the "Message brain" row is
dropped when `include_msg` is false (the persistent
shell hides it while the panel is open, shows it to re-open once closed).
`MenuApp::new(side, include_msg, pdf_target, delete_target)` owns the filtered
view; `filter_indices` /
`item_matches` are **pure** matchers (each row's matchable text includes its
1-based number), and `handle_key` is a **pure** key handler (returns
`Continue`/`Confirm`/`Cancel`). `Cancel` (Esc) tells the host to drop the
overlay, not to exit. In the persistent shell `Msg` opens/focuses the brain
panel and `ToggleLayout` swaps which side it sits on.

### `confirm.rs`
The shared yes/no confirmation modal. Like `menu`, it has **no screen of its
own**: the picker holds a `Confirm { path, kind, yes }` in its state, the host
drives its pure `handle_key` (returns `Continue`/`Cancel`/`Accept`), and paints
it with `draw_modal` as a centered overlay. `ConfirmKind` selects the flavor:
**Pdf** (green, defaults to Yes; opened by `Ctrl-G` on a `.md` file) and
**Delete** (red, defaults to **No** because it's destructive; opened by
`Ctrl-D` on any entry). The pure `accent`/`title`/`question` helpers key off
`kind`; on `Accept` the host converts (Pdf) or trashes (Delete) in place. The
key handling, the kind-keyed chrome, and the button styling are unit-tested.

### `render.rs`
Pure functions that build styled ratatui `Line`s for the picker (header,
input, separator, section header, entry with coalesced highlight spans,
empty state, footer) plus the Tokyo-Night palette constants. No state,
no IO — every function maps inputs to a `Line`, which is why they're
cheap to unit test.

### `open_target.rs`
Pure decisions about acting on a picked path: `is_textlike` (extension
allowlist; extensionless files count as text) and `finder_target` (a file
reveals its parent dir, a directory reveals itself). It also holds the
new-tab opener: pure builders (`edit_shell_command`,
`iterm_new_tab_applescript`) plus thin impure spawners (`open_in_editor_tab`,
`open_with_system`) the persistent shell uses to open files without tearing
itself down — text → a new iTerm2 tab, everything else → system `open`.
The PDF path lives here too: pure `is_markdown` (strictly `.md`) and
`pdf_output_path` (colocated, same stem, `.pdf`), plus the impure `create_pdf`
(drop any existing same-name PDF, then shell out to the user's
`markdown-to-pdf` script) that the "Create PDF" command calls. The delete path
mirrors this: pure `trash_applescript` (a Finder `delete POSIX file` line,
path escaped) plus the impure `move_to_trash` that shells out to `osascript`,
so the "Delete" command performs a recoverable, user-style trash rather than
an `rm`.

### `main_view.rs`
The app-level main-view axis: the `MainView` enum (`Tasks` / `BrainSearch`),
`MainView::step` cycling, and the pure key-classifiers `ctrl_cycles_view`
(`Ctrl+H`/`Ctrl+L`), `ctrl_jumps_view` (`Ctrl+T`/`Ctrl+B`), and
`alt_opens_help` (`Alt+S`). Pure and unit-tested; the merged `tui` App applies
the results.

### `config.rs`
Typed view of the runtime knobs, deserialized from the shared config store
(see `settings/`). Fields: `daily_triage_name_pattern`, `linear_workspace`,
and `day_rollover_hour`; `linear_base_url()` interpolates the workspace slug
into the full issue-URL prefix. Missing file/fields fall back to defaults, and
keys read elsewhere (e.g. `agenda_dir`, `skills_auto_sync`, or brain-env values
in the separate `env.json`) are ignored here.

### `sync/`
`brain sync`: manual, bidirectional cross-machine sync of the brain root to a
private Backblaze B2 bucket via `rclone bisync`, dispatched in
`src/command/sync.rs`
**before** the `markdown-to-pdf` prerequisite gate (like `config`/`env`/
`personalize`/`skills`). The data flow per run is **build → run → post-pass →
verify → journal**: `config` (`SyncConfig`, parsed from the brain-env `sync`
block) feeds `remote::build_remote` (the B2 remote as `RCLONE_CONFIG_*` env
vars, never on argv) and `args::bisync_args` (the full `rclone bisync` argv:
conflict resolution bias for the direction, keep-both flags, `--max-delete`,
default excludes, `--check-access --check-filename RCLONE_TEST`, plus
`--stats 10s --stats-one-line` for live progress and `--resilient --recover`
for resumability); `check_access.rs` creates/repairs the root-level marker on
local + remote before resync runs; `run::run_rclone` checks for the external
`rclone` executable before spawning it, then streams its stderr live to the terminal while
capturing it, and parses the capture into transferred/deleted/error counts
and an abort reason; if that abort is an incomplete baseline
(`AbortKind::PriorListingMissing`) and this run wasn't already a resync,
`command::sync_once` auto-resumes with one internal `Direction::Resync` retry
before continuing (journalled as such) — see [decisions.md](decisions.md);
`conflicts::rename_markers` (the post-pass) renames any rclone-left conflict
marker (`<original>.__brainconflict__<N>`) to the friendly
`name (conflict <host> <date>).ext`; `verify::classify` turns the parsed
outcome + the count of copies renamed + any leftover markers into `Clean` /
`NeedsAttention` / `Aborted` (a run that created any conflict copy is
`NeedsAttention` even after the markers are renamed away); and
`journal::Journal` records the run into the
SQLite journal at `<workspace-cache>/sync/journal.db` (table `sync_runs`,
machine-local, never synced). `command::sync_once` is the thin orchestrator
that runs this whole pipeline; `command::print_status`/`print_conflicts` back
`brain sync status`/`brain sync conflicts`. `setup.rs` is `brain sync setup`'s
interactive flow (collect bucket + credentials, verify/create the bucket via
`rclone lsd`/`mkdir`, write the `sync` block into brain env, bootstrap the
check-access markers through `sync_once(Direction::Resync)`, then run one
baseline `sync_once` with `Direction::Resync`) — `brain sync repair` reruns just
that resync on an already configured machine, mainly as the recovery path for
rclone's own "prior listings missing" guard. See
[integrations.md](integrations.md) for the rclone handoff detail and
[data-model.md](data-model.md) for the `sync` config fields and the journal
schema. `rclone` is an external dependency (not a Cargo crate): a soft
prerequisite, checked only when `brain sync` actually runs, never a startup
gate (`brain tasks doctor` reports its presence/version informationally).

`check.rs` backs `brain check`, a **read-only** sibling of `sync_once`: it
builds the same `Direction::Both` argv via `args::bisync_args` but appends
`--dry-run`, runs it through `run::run_rclone_capture` (a quiet, non-streaming
counterpart to `run::run_rclone` — no live terminal output, just `(exit_ok,
combined_output)`), then classifies the captured detection-phase lines with
`progress::classify_change`/`Side` (the same parser `progress.rs` already
exposed for a future live file-list). It then runs the CSV lane's read-only
counterpart in `check/csv.rs`: `check::collect_csv_pending_with_fetch` reads the cached
`csv_sync::baseline_path` text and the local task/habit CSVs, fetches each
remote CSV through `csv_sync::remote_csv_arg` + rclone `copyto`, and compares
both sides to the baseline with UUID-aware, name-aligned
`csv_merge::parse`-backed row diffs. The pure
`check::format_report` receives both the file path lists and the CSV row
counts for the themed summary. The command prints default progress before the
rclone dry-run and before the CSV baseline pass. No journal entry, no conflict post-pass, no
baseline mutation: it never calls `rclone bisync` without `--dry-run`, and
its CSV pass never writes local files, remotes, or baselines.

**The auto-sync trigger layer** (`lock.rs`/`watch.rs`/`trigger.rs`, wired
into the shell lifecycle and receiver dispatch) makes sync automatic while keeping the pure/impure
split. The **pure** cores carry the decisions and the tests; the thin shells do
the IO/threads/`Command`:

- `lock.rs` — one advisory sync lock per workspace UUID at
  `<workspace-cache>/sync/sync.lock` (a PID file beside the journal). Pure
  `is_stale(owner_alive, age, cap)` decides reap-ability (dead owner or
  heartbeat mtime past the cap); `try_acquire(path)` is the atomic
  (`create_new`/O_EXCL) thin IO shell returning `Option<Guard>` (`None` when a
  live, fresh sync holds it), and `Guard` owns a heartbeat thread that refreshes
  the lockfile mtime until drop. Drop stops the heartbeat and removes the file
  only if it still holds our PID. It wraps **all** sync entry points, including
  the manual command path in `src/command/sync.rs`, closing a pre-existing
  concurrent-`brain sync` race.
- `watch.rs` — the pure `Debouncer` (a clock-injected quiescence state machine:
  `on_event`/`time_until_fire`/`poll`) and the pure `is_watch_relevant(path)`
  exclude predicate, plus the thin `notify` shell `spawn_watcher_with` (owns the
  platform watcher, the mpsc event channel, and the debounce loop) and
  `spawn_watcher` (the real auto-sync watcher). `WatcherHandle` stops the thread
  on drop (drop the `Watcher` → the channel disconnects → the loop exits; no
  join, so teardown never blocks). On fire it spawns a detached
  `Direction::Push` run. That direction uses a one-way, non-deleting rclone
  copy; its CSV/counter pass reads remote state only to build a safe upload and
  never writes local state, so the push cannot re-arm its own watcher.
- `trigger.rs` — the single shell-facing entry point:
  `spawn_detached_sync(workspace, dir)` spawns the current exe as
  `brain --brain <canonical-name> sync [--pull|--push] --if-idle`, fully
  detached (`process_group(0)` + null stdio). Automatic startup, watcher, and
  receiver-freshness triggers go through it, for two reasons: a sync in a
  separate process can never write over the TUI, and a detached child in its own
  process group outlives the shell / terminal close. `--if-idle` makes a
  redundant trigger coalesce (exit silently) rather than follow. There is no
  in-process sync path anymore (the old `run_locked_sync`/`sync_in_background`
  are gone). The parent moves each `Child` into a small waiter thread so a
  completed background sync is reaped and cannot accumulate as a zombie.
- `current.rs` — the in-flight sync's UUID-scoped observable state, so a
  detached background sync stays observable. `Reporter` is the single output sink of a run: each
  line is appended to `<workspace-cache>/sync/current.log` and echoed to the
  process's own stderr (the terminal for a foreground run, `/dev/null` for a
  detached one). `begin` writes the `current.json` record (pid + direction +
  start); `Drop` removes it. Pure `running(state, owner_alive)` +
  `is_running()` gate on the record existing *and* its owner PID being alive, so
  a hard-killed sync's stale record never reads as live.
- `follow.rs` — when a user-run `brain sync` finds the lock held, it attaches
  instead of erroring: `follow_until_done` tails `current.log` (pure
  `appended(content, offset)` splits off each new tail, handling a truncated
  log) to the terminal until `is_running()` goes false, then prints the final
  journal outcome. Ctrl-C stops only the follower.
- `freshness.rs` — the pure two-hour threshold for deciding whether a receiver
  message needs a downstream pull. `journal::latest_downstream_completion`
  deliberately ignores push-only and aborted rows.
- `config.rs` carries `debounce_ms` (default 3000) and
  `debounce() -> Duration`; `command::format_triggers` renders the startup,
  change-push, and message-pull policies in `brain sync status`.

**The `run_tui` lifecycle seam** (`src/tui/event_loop/setup.rs`) is the one wire
point: after the startup work and before the event loop it calls
`trigger::spawn_detached_sync(Pull)` whenever sync is configured and holds a
`watch::spawn_watcher` handle (when `watch_effective()`). It drops the watcher
after the event loop and performs no exit sync. `tui/app_sync.rs` owns the
receiver freshness gate and the 250ms TUI status poll. It queues stale inbound
work behind a pull and reloads tasks before dispatch. All paths are gated and
best-effort; an unconfigured brain gets no watcher or automatic sync.

**The C5 conflict enumerator + resolver** builds on `conflicts.rs` to give
agents (not just humans) a way to close out a keep-both conflict. Still pure
where it can be:

- `conflicts.rs` gains `parse_conflict_name` (the strict inverse of
  `conflict_name`: friendly name → `ParsedConflict { original, host, date }`,
  or `None` on anything that isn't the exact grammar), `group_conflicts`
  (folds flat `list_conflicts` output into `ConflictGroup`/`ParsedCopy`,
  sorted for deterministic output), and `copies_for_original` (the copies
  belonging to one canonical original, used by `resolve`).
- `command/mod.rs` gains `conflict_display_paths`, which renders the human
  line-list from the same strict groups, and `conflicts_json`, a pure builder
  (fs metadata and existence checks injected as closures) that renders
  `&[ConflictGroup]` into the `serde_json::Value` array `brain sync conflicts
  --json` prints — see [data-model.md](data-model.md) for the exact shape.
- `command/resolve.rs` backs `brain sync resolve <original> [...]`: the pure
  `resolve_decision` classifies each original as `Delete(copies)` /
  `CanonicalMissing` / `NoCopies`, and the thin shell around it deletes the
  copies (never the canonical file) and never touches rclone or the journal.
  Bare `brain sync resolve` (no args) drops into an interactive picker over
  the open conflict groups.

Together these back the `/second-brain resolve-conflicts` skill: it reads
`conflicts --json`, merges each group into its canonical file, deletes the
copies via `resolve`, then runs one ordinary `brain sync`. See
[integrations.md](integrations.md) for that contract in full.

### `tasks/`
Everything specific to the **tasks main view**, ported from the old `tasks`
crate under one namespace: `identity` (immutable UUIDs and deterministic
legacy identity), `schema` (an explicitly inactive, backup-owning task-schema
migration helper split into path validation, pure transformation, and durable
transaction/recovery modules), `task` (CSV model, legacy-compatible load, and pure
assignment defaults/membership/UI visibility), `view` (sub-views +
`build_view`), `selector` (date parsing), `render` (task-card lines, chrome,
markdown), `shortcuts` (the help/footer catalogue), `complete` (native
task/habit completion), `triage_habits` (stable managed definitions,
marker-based mutation policy, complete purge, and durable grouped
replacement/recovery split into orchestration, artifact, and journal modules),
`doctor` (health check), `plain` (`--no-tui` printer),
and `cli` (the tasks clap args, nested under `brain tasks`). Reuses the
crate-level `session` / `state` / `pty_pane` shared with the brain-search view.
Native task command runners accept explicit `WorkspaceContext` and
`ActorContext`; they never re-resolve a global root. Shared CSV mutation code
normalizes legacy `assignee` headers to `assigned_to` before any write.
New task and habit rows carry UUIDv4 identity; completion and edits preserve
it, while a spawned habit occurrence gets a new UUID and retains assignment
and `system_key`. The schema helper requires the rollout coordinator to state
that the last legacy semantic sync is complete or not configured. It takes an
existing durable machine-local backup base plus a backup directory beneath
that base; the destination must resolve disjoint from the workspace tree. The
helper creates each missing descendant one component at a time and syncs every
actual parent, including partially created chains found on retry. Every
permanent backup entry and transaction artifact is file-synced and
parent-directory-synced before live replacement. A durable
prepared/committed journal makes failure or interruption between replacements
recoverable; retry rolls a prepared generation back before migrating, or
finishes committed cleanup. Nothing in startup, readiness, sync, or command
dispatch invokes it. Legacy CSVs keep `task_id` as their first sync key until
that coordinated migration runs. After migration, `sync/csv_merge/` owns
name-aligned UUID merge (`table` + `merge`), deterministic mutable display-ID
allocation (`reconcile`), and dependency/project reverse-link rewriting
(`relationships`). `csv_sync/operation.rs` preflights the manifest and every
base, local, and remote task/habit table as one operation before any write;
`csv_sync/metadata.rs` stages project metadata and republishes every
authoritative metadata file so retries heal partial remote publication.
Its publication result distinguishes local filesystem failures from remote
transport failures so command diagnostics identify the failing boundary.
`counters` consumes display-ID floors from the reconciled tables only after
that operation succeeds. None of these paths activates the migration helper.

App construction reconciles managed triage state before loading its final task
and habit vectors. Task reindex does the same before generic Python rules and
retention cleanup. Explicit sync repair reconciles only after a clean repair
outcome; ordinary sync does not gain a second policy branch.

### `tui/` (the merged shell)
The persistent shell, built from the ported tasks `tui/` and extended with the
main-view axis. One `App` owns: the tasks-view state, the embedded
`picker::App` (`search`, the brain-directory view), the app-level `brain`
panel, `focus` (main panel vs brain panel), `main_view`, `panel_side`, and one
startup-resolved task `AssignmentContext`. That context carries the effective
actor, portable-member rows, and the one-person/shared visibility decision.
`App` also owns the process-scoped assignee filter and its captive member
picker. Startup validates `--assigned-to` against the assignment context and
seeds that same field; view materialization retains the complete base set so
body rebuilding can switch members or clear to all before fuzzy matching. Plain
output instead applies assignment as a final render filter. Header composition
renders assignment only from that live App field, while non-assignment CLI
filters remain in the static chip row. The context's
detail mode controls task-card rendering, and its create, reassign, and filter
flags independently gate their palette rows. A missing portable registry uses
a one-actor compatibility context with hidden assignment controls.
`event_loop` routes keys in the precedence documented in
[keybindings.md](keybindings.md): app-level accelerators (view switch, help,
panel focus/scroll, brain open/close/new, quit) → captive modal → brain panel
(forward bytes) → active main view (`handlers` for tasks, `search_view` for the
brain-directory picker). `draw` renders the active main view in the main
panel, the brain panel beside it (`panel_side`), and any modal over the top.
`search_view.rs` is the brain-directory view's handler (its picker nav, in-place
open, PDF/delete confirms, and its own `menu` palette). The remaining
submodules (`handlers`, `keymap`, `palette`, `modals`, `links`, `draw_*`,
`app_*`, `shell`) are the tasks view's. The assignee picker has its own
`draw_assignee` module so the shared-workspace overlay stays separate from the
general confirm, link, and brain-input modal renderer.

The larger submodules are directories split by concern: `handlers/`
(`overlay`/`tasks_view`/`input`), `event_loop/` (`setup`/`modal_route`/`run`),
`draw/` (`tasks_panel`/`brain_panel`/`layout`, with the `draw` entry in
`draw/mod.rs`), `palette/` (`command`/`state`), `app_state/`
(`construct`/`nav`/`view`/`selection_query`), `app_actions/`
(`commands`/`triage`), `app_brain/` (`launch`/`lifecycle` plus receiver
`dispatch`/`completion`/`state` and focused tests), and `tests/` (split by
area). `app_brain/` owns the main persistent controller, receiver dispatch,
and completion delivery;
`app_triage_tab.rs` owns the ephemeral daily-triage controller and tab
(open/close/select, the `BrainTab` resolution, and the `tick_triage_done`
auto-close). The overlay-modal state
structs (`PaletteState`, `ConfirmState`, `BrainInputState`, `HelpState`,
`LinkPickerState`, and the confirm enums) live in `modal_state.rs` with
`pub(super)` fields; `mod.rs` keeps only the `App` shell type, `Panel`,
`filter_tasks`, and the module wiring. `status_warning.rs` validates receiver
phone configuration and renders persistent warning content independently from
the transient palette flash.

### Startup (`run_tui`)
`run_tui()` opens the state DB, builds the brain-search picker
(`build_search`), and constructs the `App` from the selected `CommandContext`.
The constructor derives its retained root and state-DB path from that context;
callers cannot supply competing workspace paths. `open_or_focus_brain(None)`
then launches the selected frontend through an `AgentController`
(Claude resume-vs-fresh; Codex fresh) and `focus_tasks()`
returns focus to the tasks main view so `j`/`k` work at once. It then wires the auto-sync
triggers (a mandatory detached pull-biased startup sync and, when
`watch_effective()`, a held `watch::spawn_watcher` handle), runs the event
loop, explicitly shuts down the main and triage controllers, then drops the
watcher and releases the session lock. No exit sync or
idle timer exists. The **daily-triage nudge**
is coupled to that startup sync: when a configured startup sync is pending, `run_tui`
does *not* run the check immediately. It captures the sync journal's latest
clean downstream row ID, kicks the sync, and calls `App::arm_triage_gate`
(deferral, no modal). Each event-loop tick then calls `App::tick_triage_gate`.
Once a newer clean pull/both/resync row appears, it strictly reloads portable
config, reconciles managed policy under the workspace task-store owner, reloads
both synced CSVs, and evaluates the live process-scoped alert state. Palette
re-enable while this gate is armed defers its check to that refreshed state;
the gate does not cache the launch-time alert value. Reload or
reconciliation errors are logged and shown in the TUI rather than discarded,
so the modal reflects post-sync completion state (pure `triage_gate_resolved`
decides resolution). `--no-daily-triage-check` disables only the final alert;
the same gate still performs the strict config, managed-policy, and task-table
refresh. With no startup sync, the check runs immediately as before. The
brain
panel is **closeable** (agent exit → `close_brain` shuts down its controller and the main
view goes full-width); `open_or_focus_brain` (`Ctrl+M`) re-opens it. The
brain-directory view keeps its own `scope`/`rescope`/`search_refresh` for
bucket rescoping (`Ctrl+R` / palette search rows). Unlike the pre-merge shell
there is no `Exit` enum — the shell just returns from the event loop on quit
(the tasks view never handed a plan back), and `Ctrl+T`/`Ctrl+B` switch views
in-process rather than exiting.

### `pty_pane.rs`
`PtyPane` is a dormant-capable `AgentTransport` that spawns a complete
`LaunchSpec` under a pseudoterminal (`portable-pty`),
streams its bytes through a `vt100` parser, and exposes the screen for
rendering. Reader / writer / waiter threads; `send` / `resize` /
`scroll_*` / `is_alive`. It applies the spec's selected workspace cwd before
the child starts and has no frontend-specific command or input knowledge. It
clears the inherited process environment, then applies only the launch spec's
selected workspace/actor values, hook metadata, narrow frontend necessities,
and a fixed `TERM`. The command string runs through fixed `/bin/sh -c`, which
preserves configured command parsing without sourcing login or interactive
profiles that could recreate filtered environment values.
Real-PTY transport regressions live in the owned `pty_pane/tests.rs` child so
the production module remains focused on transport behavior.

### `session.rs`
Compatibility launch planning: re-exported `agent::AgentKind`,
`Plan::{Resume,Fresh}` (chosen from actor-scoped DB resume candidates), and
`build_llm_command`, which adds the legacy shell `cd` prefix around the command
translated by the selected adapter and returns a typed error for a blank legacy
session ID. `env_for` and `env_for_triage` remain only
as compatibility helpers for pure callers and tests. Live TUI panels build
complete `LaunchRequest` values: the adapter supplies common workspace identity
and `BRAIN_AGENT_KIND`; the main panel's `HookMetadata` adds instance, PID,
state DB, and response attribution, while the triage panel adds only
`BRAIN_TRIAGE_DONE_URL` and `BRAIN_TRIAGE_TOKEN`. `claude_cmd`, `codex_cmd`,
and the reserved `opencode_cmd` are machine-local brain env values. The two
functional configured commands are spliced in
verbatim so they may carry their own flags, and brain never depends on a shell
alias.

### `triage_signal.rs`
The on-disk bridge for the daily-triage tab's completion signal. Pure
`parse_signal` + `ready_to_close` (the close gate: every path the run declared
in `require` must exist; core declares none, so an empty list closes at once)
plus a thin file shell (`record_done` / `read_signal` / `clear`,
`~/.cache/brain/triage-done.json`): the brain server writes it from
`POST /triage/done`, the TUI polls it each tick and holds a premature signal
until its required outputs land. Deliberately ignorant of *what* those outputs
are — see the extension-agnostic rule in [AGENTS.md](../AGENTS.md). See
[integrations.md](integrations.md). This file remains part of the transitional
machine-shared server control plane; workspace membership/routing and its
replacement belong to the approved shared-server phase.

### `state.rs`
The SQLite state layer (`rusqlite`, WAL) at `<workspace-cache>/state.db`.
`brain_sessions` tracks Claude and Codex sessions by a composite agent-kind,
session-ID, workspace-UUID, actor-ID, and channel key with a `locked_pid` lock
and `active`/`completed` completion status;
`meta` stores the `panel_side` layout preference and the
`skills_synced_version` render stamp (the brain version that last rendered this
workspace's skills, read by `skills::resync_on_version_change`). The resume
model is scoped lock + recency behind `agent::session::SessionStore`
(`reap_dead_locks`, `sessions_by_recency`, `claim`, `register`, `release`,
`mark_active`, `mark_completed`, `completion_status`). The `PanelSide` enum lives here since
it's the persisted value. Mirrors `tasks/src/state`. See
[data-model.md](data-model.md) and [integrations.md](integrations.md).

### `server/`
Brain is converging on one TUI-lifetime shared process. It currently serves the
local habits and triage routes, while the transitional receiver server remains a TUI-owned, opt-in
listener on `/sms` and `/email`; it is never started by ordinary TUI startup
and cannot outlive the interactive shell.
- `server/router.rs` — pure route mapping for `/habits`, `/habits/done`,
  `/triage/done`, `/sms`, and `/email`. The former `/webhooks/capture`
  placeholder is removed.
- `server/receiver.rs` + `server/receiver/` — the receiver facade and its
  single-responsibility modules: `http/` owns the bounded four-worker
  `tiny_http` listener and channel queue, `http/sms.rs` and `http/email.rs`
  own provider parsing, `attachments.rs` stages media, and `control.rs` owns
  the protected local command socket.
- `server/security.rs` owns pure Twilio HMAC, Resend/Svix HMAC, and the ordered
  authenticate-then-resolve decision for enabled portable identities.
- `server/lifecycle/` owns the shared-process boundary. `paths.rs` places
  `process.json`, `control.sock`, `election.lock`, and `server.log` below one
  machine-wide directory. `state.rs` owns the minimal generation-tagged record;
  `election.rs` owns the pure start decision, directory advisory mutex, exact
  owner checks, and parent-to-child token handoff with retained parent cleanup
  until child publication;
  `process.rs` owns the non-electing client, detached elected spawn, hidden
  server loop, signal cleanup, and narrow registration seam; `watchdog.rs`
  applies clock-injected expiry plus the bounded initial-registration deadline;
  `lease.rs`, `table.rs`, and `decision.rs` own typed leases and latched
  final-shutdown decisions. Signal flags and cleanup ownership precede process
  state publication. The table and process record never contain roots, users,
  credentials, prompts, logs, or message bodies.
  `lifecycle::pid_alive` remains the stable seam for sync callers.
- `server/routes/habits/` — the habits MVC route and embedded frontend. GET
  and completion POST requests carry `workspace_id`; the shared process
  reloads the registry by exact UUID and verifies the root's portable manifest
  before accessing that workspace's CSV. A missing, malformed, unknown,
  unavailable, or manifest-mismatched identity is rejected; it never falls
  back to the machine default.
- `server/routes/triage/` — the `POST /triage/done` controller: the ephemeral
  daily-triage session's completion signal (see `triage_signal.rs`).

`brain --with-receiver` starts the receiver listener after the TUI singleton is
acquired. The global palette can start, stop, restart, and inspect it while
the TUI is alive. Inbound work is queued as workspace UUID plus immutable
actor context in a bounded in-memory channel and
is never allowed to interrupt an active agent turn. `tui/receiver_state.rs`
distinguishes a submitted turn from an idle open PTY, so an idle startup panel
can switch to the receiver session even when a modal is on screen. It also
distinguishes active receiver work from a three-minute warm channel lease:
interactive Stop-hook completions are still polled, a same-channel message
reuses the warm PTY, and another channel replaces it only after work finishes.
`tui/app_sync.rs` holds inbound dispatch behind a pull when downstream state is
more than two hours old and exposes current sync state to the footer and
palette. Failed PTY launches retain the message for a backoff retry. Provider replies are handed
to the bounded background worker in `server/delivery.rs`, keeping network
latency off the TUI event loop. The listener rejects
bodies over 1 MiB, uses a fixed worker pool so one slow provider call cannot
block every route, treats idle time as normal, and uses
`tiny_http::unblock()` only for graceful shutdown.

### `lib.rs`
Re-exports the modules so integration tests in `tests/` and the thin binary
entry point share one compiled module graph. `main.rs` calls this library
surface instead of privately declaring every module a second time.

## Build / run loop

`run.sh` rebuilds `target/release/brain` whenever `Cargo.toml` or any
`src/**/*.rs` is newer than the binary, then `exec`s it (build chatter goes
to stderr so stdout stays clean). The user never types `cargo run`. Manual
rebuild:

```sh
( cd path/to/brain && cargo build --release )
```

## Invariants the code depends on

- **`Bucket` declaration order is the display order** (Projects → Areas →
  Resources → Archive). The picker's `sort_by` and `build_display_rows`
  rely on the derived `Ord`.
- **The binary's stdout is only intentional short-lived CLI output:**
  `config/env/version`, `workspace list`, explicit plain-task output, help, and
  non-TUI logs mirrored by `--verbose`. Clap errors and diagnostics go to
  stderr. The TUI renders to `/dev/tty`.
- **Every `Choice` has exactly one palette row** (guarded by a test on
  `items(side, …)`) so the menu can't silently drop an action.
- **The brain panel is open at startup but closeable.** `tui` launches the
  selected controller at startup and is two-panel; when its agent
  exits the panel **closes** (search goes full-width) — it does not quit the
  shell. `open_or_focus_brain` ("Message brain" / `Ctrl-M`) re-opens it.
- **Exactly one frontend session per brain instance is locked at a time.**
  A SessionStart hook may update an exact registered tuple or rotate an
  already registered active lineage; it rejects unregistered events and frees
  the instance's other sessions on every accepted start
  (so `/new` leaves the prior conversation resumable). Its authorization and
  mutation share one `BEGIN IMMEDIATE` transaction, so concurrent rotations
  recheck target ownership after serialization. `release` clears the lock on
  exit; dead-PID locks are reaped on the next startup.
- **A committed completion always has a durable response artifact.** The Stop
  hook stages a unique synced file, acquires `BEGIN IMMEDIATE`, rechecks and
  updates the same exact locked session scope, publishes and syncs the artifact,
  then commits. A failed publication or commit rolls back and cleans up only
  that attempt; a concurrent SessionStart rotation is rechecked after writer
  serialization.

## Dependencies

Beyond the picker's core (clap, ratatui, crossterm, nucleo, walkdir,
anyhow), the persistent shell pulls in four crates, all mirroring the `tasks`
sibling so the two projects share a stack:

- `portable-pty` + `vt100` + `tui-term` — spawn, parse, and render the
  embedded `claude` PTY.
- `rusqlite` (`bundled`) — the WAL state DB shared with the SessionStart
  hook; `bundled` avoids a system libsqlite dependency.
- `uuid` (`v4`, `v5`): fresh runtime/workspace/task identities plus
  deterministic task identities for fixture-tested legacy migration.
- `include_dir` — embeds the repo's `skills/` dir (SKILL.md + scripts) into the
  binary so a public cloner needs no repo checkout; `brain skills sync` writes
  them out. Multi-file skill assets rule out `include_str!`.
- `tiny_http` — the two small synchronous HTTP services under `src/server/`:
  the machine-wide shared local process and the transitional TUI-owned
  receiver. The receiver uses a
  fixed four-worker pool, bounded request bodies, and a bounded handoff queue;
  this preserves concurrency and backpressure without pulling a Tokio runtime
  into an otherwise synchronous CLI.
- `signal-hook`: installs safe SIGINT/SIGTERM flags for the shared process.
  The accept loop observes the flag and lets its generation owner remove only
  the matching process record and control socket, without unsafe signal code.
- `fs2`: provides Rust-1.85-compatible advisory locking for the shared-server
  election mutex, avoiding unsafe platform calls while serializing exact owner
  reaping and parent-to-child adoption.
- `notify` (8.x) — cross-platform filesystem observation for the **C4
  auto-sync watcher** (`src/sync/watch.rs`). Linux uses the recommended native
  backend. macOS uses notify's one-second `PollWatcher`, because FSEvents can
  silently omit valid changes; this was reproduced by the real-filesystem
  watcher integration test. All decision logic remains in the pure,
  clock-injected `watch::Debouncer`, so we depend on neither
  `notify-debouncer-full` nor `notify-debouncer-mini`.

`brain sync` also depends on **`rclone`**, but as an external command it
shells out to (`src/sync/run.rs`), not a Cargo crate: brain builds the argv
and an env-var-only remote config and lets the user's own `rclone` install do
the transfer. It's a soft prerequisite (checked only when `brain sync` runs;
see [integrations.md](integrations.md)), unlike the hard `markdown-to-pdf`
gate.

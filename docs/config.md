# Configuration

`brain` splits its persisted state across **two config stores**, by lifecycle.
The machine-global path has one owner: the versioned workspace registry. Each
workspace record silos the machine-local values that belong to that workspace;
they are never inherited or merged from the default or another record.

| Store | Path | CLI | Synced? | Holds |
| --- | --- | --- | --- | --- |
| **brain env / workspace registry** | `$XDG_CONFIG_HOME/brain/env.json` (fallback `~/.config/brain/env.json`, outside every brain root) | `brain workspace …` manages records; `brain env …` reads and writes the already-selected record | **No**: machine-local, never rides any workspace sync | Schema-v4 canonical default, a machine-global `env` map, plus siloed workspace records (`workspace_id`, `root`, aliases, local user, receiver switch, and per-workspace machine env) |
| **brain config** | `<brain-root>/.config/config.json` (e.g. `~/brain/.config/config.json`) | `brain config {list\|get\|set}` | **Yes**: travels with the brain | Portable `access_mode`, logical `allowed_mcps`/`allowed_skills`, `linear_workspace`, triage settings, `response_email`, and SMS/email sender allowlists |

The rule of thumb: **brain env holds anything that would be *wrong* if copied to
another machine** — absolute paths, machine-specific binaries, secrets, and
machine-specific frontend launch commands.
**brain config holds anything that's *right* on every machine** — slugs,
preferences, behavior flags. [Personas](#personalization-personas) (below) are a
third store, content *about the workspace's people* keyed by portable user ID,
which also lives inside the brain root and syncs with it alongside
`config.json`.

Both CLIs run **before** the `markdown-to-pdf` prerequisite gate, so you can
always repair a broken environment or config even when that tool is missing.
Both normalize names the same way (lowercased, `-`→`_`).

## Deciding which store owns a new variable

Ask these two questions **in order**. The first one that answers settles it.

1. **Does the value have to exist before a brain workspace does?** If a variable
   is needed to *find*, *create*, or *restore* a workspace — the workspace root,
   the registry itself, the `sync` block that pulls a workspace down for the
   first time — it cannot live inside the workspace it bootstraps. It is **brain
   env**. Otherwise ask question 2.
2. **Must every machine connected to this workspace agree on the value, or may
   each machine set its own?** If machines are free to differ, it is **brain
   env** (machine-level). If they must agree, it is **brain config** (one value
   for every machine on the workspace).

Brain env then has two scopes, and one more question picks between them:

2a. **Could two workspaces on the *same machine* sensibly hold different
    values?** If yes, the value belongs in that machine's **workspace record**
    (`workspaces.<name>.env`) — a receiver URL, a sync block, a per-workspace
    credential. If no, it is **machine-global** (the registry's top-level `env`):
    `markdown_to_pdf_path` names one binary on one machine, and answering it
    differently per workspace was never meaningful.

Note the unit in question 2: a **machine**, not a user. One person may connect
several machines to the same workspace, and several people may connect their
own; "machine-level" means per-installation either way. Content *about a person*
is neither store — it belongs in [personas](#personalization-personas), which is
keyed by portable user ID.

Worked examples:

| Variable | Store | Why |
| --- | --- | --- |
| `root`, `sync.*` | env | Q1: needed before the workspace exists locally. |
| `claude_cmd`, `codex_cmd`, `opencode_cmd` | env (workspace record) | Q2: an absolute path/command that is only correct on the machine that has that binary. Q2a: a workspace may legitimately launch its frontend differently. |
| `markdown_to_pdf_path` | env (machine-global) | Q2: a machine-specific binary path. Q2a: one machine, one binary — every workspace on it must resolve the same one. |
| `default_agent_frontend` | workspace record | env | Q2: which frontend you drive is a per-machine preference; a laptop with only Claude installed must not be forced onto Codex by another machine. |
| `twilio_*`, `resend_*` | env (workspace record) | Q2: exactly one machine serves receiver ingress for a workspace at a time, so the provider credentials belong to that machine, not to every machine. Q2a: each workspace answers on its own number and address, which is also what routes an inbound message to it. |
| `brain_receiver_public_url` | env (machine-global) | Q2: the public origin is a property of this machine's tunnel or host. Q2a: one machine serves **one** `/sms` and one `/email` URL for every workspace on it, and providers sign the literal URL, so two answers would be a bug rather than a preference. |
| `skill_sessions` | env (workspace record) | Q2: a definition names a prompt whose skill must be installed on *this* machine, so a row that travelled to a machine without it would fail. Q2a: two workspaces on one machine may reasonably offer different sessions. |
| `linear_workspace`, `access_mode`, `allowed_skills`, `enable_triage_habits`, `enable_daily_triage_check` | config | Q2: a workspace-wide policy or slug that would be a bug to have disagree between machines. |

A genuinely one-invocation choice (`--with-receiver`, `--verbose`) is neither
store: it stays a CLI flag. A value that belongs in a store must not *also* be a
flag — see the CLI ↔ command-palette parity rule in `AGENTS.md`.

## Machine workspace registry (`~/.config/brain/env.json`)

`~/.config/brain/env.json` is the sole machine-global workspace registry
(`$XDG_CONFIG_HOME/brain/env.json` when XDG config is set). Schema version `3`
stores a canonical default and a sorted map of canonical workspace names to
complete `WorkspaceRecord` values. Each record owns its own machine root,
immutable UUID, aliases, `local_user_id`, `receiver_enabled` switch, and `env`
object. The `env` object is siloed: selecting a canonical name, an alias, or the
default returns only that record, with no copying or merging from any other
workspace. Portable access policy never lives in this machine-local file.

Registry loads accept only exact schema version `3`, a non-empty record map, a
default that names a canonical record, unique canonical/alias selectors under
ASCII case folding, unique UUIDs, and non-overlapping absolute roots compared
after lexical normalization. Every writer first acquires the adjacent
`.env.json.transaction.lock` SQLite transaction lock, then loads the current
bytes, mutates and validates a candidate, and persists it before releasing the
lock.
Lock acquisition has a bounded wait and reports the PID from the stable
`.env.json.transaction.lock.owner` sidecar when available. SQLite releases
ownership automatically if a process exits. Persistence is separately
crash-safe: brain writes and syncs a same-directory temporary file, then
atomically renames it. Failed validation or IO leaves both the live registry
and prior bytes intact.
Detaching a record changes only the registry and never removes or edits its
root directory. Creating a workspace also treats its new root chain as part of
the transaction. The complete candidate validates first, and `create_dir` is
the authoritative ownership decision; an `AlreadyExists` race fails and points
to `attach`. If later provisioning or persistence fails, Brain does not delete
any directory: a path-based ownership check cannot be coupled atomically with
deletion through the safe Rust 1.85 standard library. The structured error
retains the original failure as its source and lists only the directory paths
created by that invocation, deepest first, for manual inspection and cleanup.

Registry JSON crosses a private raw-schema boundary before becoming a
`MachineRegistry`; conversion always runs every whole-registry invariant.
Direct deserialization therefore cannot create a structurally valid but
domain-invalid registry. The store keeps domain validation errors typed and
reports structural JSON and IO failures with the failed operation and path
(plus the IO error kind and temporary path when applicable).

Canonical names and aliases are trimmed and ASCII lower-cased, then must match
`[a-z0-9][a-z0-9_-]*`. `--workspace <selector>` and `-w <selector>` resolve either
kind before or after a subcommand. An omitted selector uses only the canonical
`default_workspace`. The first record becomes default; later create and attach
operations preserve it. Rename preserves the UUID and updates the default name
when needed. Changing the default workspace never changes access mode, root,
local user, receiver enablement, aliases, identity, or env. Remove detaches
only the machine record and never deletes the root.

`brain workspace` explicitly loads this schema-v4 registry and applies every
mutation through `RegistryStore`'s interprocess transaction and atomic-save
boundaries. Startup migration and selected-record `brain env` writes use the
same lock, so they cannot overwrite a workspace command.
Receiver intent uses this same transaction boundary. `brain receiver start`,
`brain receiver stop`, startup `--with-receiver`, and the command palettes
mutate only the selected canonical record after rechecking its immutable UUID.
The status command reads the persistent value independently from current TUI
and shared-process availability.
Its global `--workspace/-w` selector resolves canonical names and aliases once at
the bootstrap boundary. Ordinary commands receive a ready selected context;
env writes verify both canonical name and immutable UUID, while config,
personalization, tasks, reindex, sync, receiver setup, and the TUI consume that
same context and its once-resolved actor. Changing the default or local user
after bootstrap cannot redirect or reattribute a read or write already in
progress.
Without a portable user store, legacy compatibility accepts only an exact
lower-case kebab `local_user_id`. A malformed nonblank legacy value is rejected
with `brain workspace repair -w <workspace> --local-user-id <USER_ID>`; Brain
does not create `users.json` as part of that repair path.

### The `brain workspace` command

| Command | Effect |
| --- | --- |
| `brain workspace list` | Deterministically list canonical records with the default marker, root, aliases, local user, receiver state, and access policy, then append the selected workspace's redacted required/optional health matrix. Empty, unavailable, and incomplete setup are explicit. |
| `brain workspace create [--name <name>] [--root <path>]` | Validate the complete candidate, create the normalized root and strict portable manifest, then register the same UUID; root basename supplies an omitted name. A later persistence failure preserves the manifest and every directory path the invocation created for manual cleanup. The first interactive tasks launch initializes an empty workspace with the default config, task/habit and lookup CSVs, counters, and PARA directories. If sync is configured, Brain completes the startup pull first and pushes the initialized result afterward. |
| `brain workspace attach [<root>]` | Validate a strict compatible manifest in an existing root and register its UUID without editing root contents. Invalid or colliding identities leave registry bytes unchanged. |
| `brain workspace rename [<workspace>] [<name>]` | Rekey the canonical name while preserving the complete record and updating the default if needed. |
| `brain workspace alias {add\|remove} [<workspace>] [<alias>]` | Add or remove an alternative case-folded selector. A duplicate alias on the same record is an actionable error and leaves bytes unchanged. |
| `brain workspace default [<workspace>]` | Set the canonical default through a canonical-name or alias selector. |
| `brain workspace remove [<workspace>]` | Detach only the registry record; root and every local/remote runtime artifact remain untouched. |
| `brain workspace repair [--manifest] [--local-user-id <id>]` | Recreate a missing manifest that matches the registry and/or set this machine's local identity. Omitting both flags uses the interactive prompt. |
| `brain workspace migrate [--acknowledge-all-machines-updated]` | Run or resume the coordinated legacy task/user rollout. A synced headless workspace requires explicit `--workspace <workspace>` selection and the acknowledgement flag. |

Every optional grammar value has a `/dev/tty` prompt when omitted and a flag
or positional noninteractive form. For create, attach, remove, and repair,
bootstrap collects and validates the complete request before legacy
classification or migration. EOF/cancellation therefore leaves legacy env and
pointer bytes, the root tree, manifests, backups, and registry bytes unchanged.
Complete noninteractive forms skip terminal IO and then perform any required
migration before executing the prepared request. Workspace commands run before
the `markdown-to-pdf` gate; on a genuinely fresh machine, first
`create`/`attach` can therefore establish the initial schema-v4 registry
without migration inventing a competing default.

### Portable manifest and readiness

Each workspace root carries `<brain-root>/.config/workspace.json`. Schema `1`
contains the workspace UUID, a stable receiver ingress UUID, and the minimum
compatible Brain version. Schema `1` is the only accepted manifest schema.
Version comparison uses a numeric `major.minor.patch` core and rejects missing,
extra, or nonnumeric components. Parsing also rejects unknown fields,
unsupported schema versions, invalid UUIDs, and a minimum version newer than
the running binary with exact update-required guidance.
The manifest UUID must equal the selected machine-registry UUID.
The manifest is create-only and strict: create publishes it only when the path
is absent, attach reads it without editing, and unknown fields or identity
mismatches fail rather than silently replacing portable identity.

The same directory carries strict schema-1 `users.json` when portable people
have been configured. It contains person IDs, display names, normalized phone
and email identities, inbound-enabled flags, and optional response emails.
The file travels with the workspace; the selected person's `local_user_id`
remains in the machine registry.

Receiver setup edits portable people directly. It asks for a phone identity
only when SMS is selected and an email identity only when email is selected.
The address is attached to an existing selected user or a newly named user,
with a separate inbound-allowed value. Legacy allowlists remain compatibility
inputs outside this setup path.

Create and attach are registry-only setup commands, so they can establish an
incomplete record. Before every ordinary command, Brain then requires manifest
agreement and, when `users.json` exists, a local ID that names one portable
person. An interactive first-use flow creates and selects the first person; a
headless invocation reports exact `brain user add` and `brain user local`
commands. Migration mapping adds `brain user reassign` to that headless
vocabulary, because a legacy assignment value often belongs to someone the
registry already has. An existing workspace with no `users.json` and a non-empty legacy
local ID stays ready without being rewritten. Version/help and hidden internal
server execution perform no workspace IO or prompt.

### Selected-workspace requirements and status

Brain centralizes configuration health without changing the startup readiness
contract. Root, compatible manifest UUID/schema, a nonempty portable user
registry, and a valid selected local user are required availability. Optional
features use three states: `off` when deliberately disabled or absent, `ready`
when all selected-workspace inputs are valid, and `incomplete` when configured
but malformed or partial.

The optional matrix covers cloud sync and its watcher; receiver, SMS, and
email; advisory access policy plus requested MCPs and non-core skills; managed
triage habits and modal pattern; PDF conversion; Linear; the local person's
persona role/organization/tag styles; **other members' personas** (which portable
members still have nothing filled in — reported here precisely because they are
never prompted for on somebody else's machine, and `off` when the workspace has
no portable roster); and browser/web views. `workspace_only` remains
an advisory policy, not filesystem isolation. PDF conversion appears in the
matrix but the established TUI startup prerequisite remains unchanged.

`brain workspace list`, `brain sync status`, `brain receiver status`, and
`brain tasks doctor` read only the pinned selected workspace when they render
this matrix. They do not inherit fields from the default or any peer workspace,
and they never reveal sync credentials, provider secrets, phone numbers, or
email addresses. Bare `brain receiver` and `brain receiver {email|phone}` are
the deliberate exception, and only for the receiver's own published addresses:
asking what address the receiver answers on is a request to see it. They still
never print a provider credential. Every incomplete row supplies noninteractive repair syntax;
interactive prompt metadata records which inputs are secret without carrying
their current values.

### Access policy status

`access_mode` belongs to portable workspace config, never the machine registry.
The first migrated or created workspace is seeded as `unrestricted`; a later
created or attached workspace is seeded as `workspace_only`. An already-present
valid portable value wins. A selected schema-v4 record is checked before an
ordinary mutating or TUI command, and a missing mode is seeded according to
current default/nondefault status. Read-only `workspace list` does not seed or
repair any record. Changing the machine default changes routing only and never
changes either portable value.

`workspace_only` is advisory prompt enforcement plus best-effort capability
filtering, easy to bypass, and not tenant isolation. It is intended only to
reduce accidents and naive cross-workspace leakage among trusted users.
Adversarial or sensitive isolation requires an external OS, VM, machine, or
container boundary.

Brain installs trusted advisory instructions in all registered agent frontends, filters
the child environment to selected-workspace context and frontend necessities,
sets the selected root as the child working directory, and exposes an
intentionally naive literal-path warning. Claude, Codex, and OpenCode continue
to use the user's shared frontend login; workspace selection does not create
separate credentials. The status output describes the advisory boundary directly:

```text
Access mode  workspace-only
Enforcement  advisory prompts and capability filtering
Sandbox      none
```

The portable `allowed_mcps` and `allowed_skills` arrays contain logical names
only. A missing `allowed_skills` field defaults to `contacts`, `second-brain`,
`todo`, and `triage`; an explicit empty array remains empty. In unrestricted
mode the frontends use their ordinary global MCP and skill configuration. In
workspace-only mode Brain resolves the logical names against only the selected
workspace record's `agent_capabilities` environment object. Run
`brain skills status` to see requested names, availability, and the honest
Claude/Codex/OpenCode enforcement level without printing connection material. Names are
ASCII case-normalized and must begin with a letter or digit; remaining
characters may be letters, digits, `.`, `_`, or `-`.

Inbound request actor selection reads `users.json`: provider signatures are
verified first, then the normalized sender must match an enabled phone or email
identity. Legacy receiver allowlists and response settings remain compatibility
inputs until explicit `brain workspace migrate` maps them to portable people.
That command also owns the final legacy semantic sync, durable backup, task
schema activation, schema-last remote publication, derived rebuild,
verification, and resumable journal. After its final legacy sync it reloads
portable config, users, and CSV assignments before preflight. While that
journal exists, ordinary sync and sync setup require migration to resume;
ordinary startup and sync paths never activate migration. Task `assigned_to`,
managed triage-habit policy, and the complete shared receiver lifecycle are
active. The agent-controller facade and advisory access modes are active;
OpenCode sessions use the same controller, state, and receiver lifecycle as
Claude and Codex. OpenCode-specific command arguments and lifecycle events are
owned by its adapter.

### Selected workspace env

Machine-local env values live at the fixed registry path, most of them inside
the selected workspace record. They do **not** depend on the workspace root and
never ride whatever syncs that root (Backblaze, a cloud drive, etc.). Structural
record fields are managed by `brain workspace`, not exposed as free-form env.

A few values are **machine-global** instead: they live in the registry's
top-level `env` object, outside every record, because two workspaces on one
machine could not sensibly disagree about them (see question 2a above).
`brain env get/set` addresses them by the same bare name; `brain env` reports
them once in the machine-global section rather than repeating them under every
workspace.

| Variable | Scope | Default | Meaning |
| --- | --- | --- | --- |
| `markdown_to_pdf_path` | machine-global | *(auto-discovered)* | Path to the `markdown-to-pdf` command on **this machine**, shared by every workspace registered here. Lives in brain env (not brain config) because it's a machine-specific binary path, never "right" on every machine. See below. |
| `claude_cmd` | workspace record | `claude --dangerously-skip-permissions` | Command that launches the brain panel's default Claude frontend on **this machine**. brain appends `--resume`/`--session-id` after it, so the value is the base command plus any of its own flags. Blank falls back to the default. If unset, a legacy `brain config claude_cmd` value is honored for back-compat. |
| `codex_cmd` | workspace record | `codex` | Command that launches the brain panel's Codex frontend on **this machine**. Current live panels start fresh because the adapter rejects resume candidates; the compatibility command builder retains `resume <id>` syntax for a validated future source. Fresh Codex panels launch without Claude-only `--session-id` / `--resume` flags. Blank falls back to `codex`. |
| `opencode_cmd` | workspace record | `opencode` | Command used to launch OpenCode on **this machine**. Blank falls back to `opencode`; Brain appends `--agent brain`, optional validated `--session <id>`, and optional `--prompt <text>`. The command must pass Brain's isolated supported-feature probes. |
| `default_agent_frontend` | workspace record | `claude` | Frontend the brain panel launches on **this machine** when no `--claude` / `--codex` / `--open-code` flag is passed. Exactly one of `claude`, `codex`, `opencode`; `brain env set` also accepts the flag's `open-code` spelling and stores it canonically, and rejects any other value. Machine-local because a machine that has only one frontend installed must not be dragged onto another by a peer machine. An unreadable stored value falls back to `claude` rather than failing the command. |
| `skill_sessions` | workspace record | *(unset → daily triage only)* | The **skill sessions** this machine offers in the tasks-view command palette: a JSON array of `{title, prompt, command_label}`. Each runs its prompt in its own brain-panel tab and closes when the run signals completion; while it runs, its palette row disappears. `prompt` is required, `title` defaults to it, `command_label` defaults to `Run <title>`. Daily triage is **builtin** (offered while `enable_daily_triage_check` is on) and is neither listed nor removable here. Machine-local because a definition names a skill that must actually be installed on *this* machine. `brain env set skill_sessions` with no value opens an add/edit/delete walkthrough. See [features.md](features.md) and [data-model.md](data-model.md). |
| `agent_capabilities` | workspace record | *(unset)* | Machine-local MCP commands, arguments, URLs, credentials, and non-bundled skill paths for this selected workspace. Logical allowlists stay in portable brain config. Credential descendants are redacted from `brain env list`. |
| `sync` | workspace record | *(absent → disabled)* | Backblaze B2 cross-machine sync config: `enabled`, `b2_bucket`, `b2_path`, `b2_key_id`, `b2_app_key`, optional `rclone crypt` fields (`crypt_password`, `crypt_password2`, `crypt_filename_encryption`, `crypt_directory_name_encryption`), `watch`, `debounce_ms`, `max_delete_percent`, `exclude`, `max_size`. Drives manual sync plus the mandatory startup pull and change-triggered pushes; there is no periodic idle pull. Written by **`brain sync setup`**, not raw `brain env set`. See [data-model.md](data-model.md) for the field-by-field schema. |

OpenCode launch configuration is supplied through `OPENCODE_CONFIG_CONTENT`.
If that variable already exists, it must contain a JSON object. Brain preserves
unrelated values and owns only `agent.brain`, `default_agent`, generated
`mcp.brain_ws_*` entries, and the selected workspace skill-path addition. Old
`brain_ws_*` MCP entries are pruned before current entries are added. In
workspace-only mode the Brain agent also receives deny-by-default selected
skill permissions; unrestricted mode leaves the user's ordinary global
capabilities in effect. The merged inline object is passed only to the child
and is not written to the user's OpenCode config file.

`agent_capabilities` has this selected-record shape. Each MCP defines exactly
one of `command` or `url`; every credentials field is machine-local. A custom
skill names an exact absolute, symlink-free directory containing a regular
`SKILL.md`. A command is one non-whitespace executable string with separate
control-free arguments. URLs must be exact `http` or `https` URLs with a host.
Environment credentials belong only to stdio MCPs; headers and bearer tokens
belong only to HTTP MCPs. Frontend/auth/lifecycle environment names are
reserved and cannot be MCP credential targets.

```json
{
  "agent_capabilities": {
    "mcps": [
      {
        "name": "notion",
        "url": "https://example.test/mcp",
        "credentials": {
          "headers": { "Authorization": "secret value" }
        }
      }
    ],
    "skills": [
      { "name": "custom-skill", "path": "/machine/local/custom-skill" }
    ]
  }
}
```

### The `brain env` command

Mirrors `brain config` exactly, over the env store:

| Command | Effect |
| --- | --- |
| `brain env list` | Print the whole-machine env breakdown (see below). Bare `brain env` is identical. |
| `brain env get <name>` | Print the effective value of one variable or dotted nested path, such as `sync.b2_bucket`. |
| `brain env set <name>=<value>` | Set a declared scalar variable or dotted nested env path in the selected record, preserving sibling values. Dotted paths descend objects by name and structured lists by index (`skill_sessions.0.prompt`); a missing object key is created, a missing list index is an error rather than an invented entry. Structural record fields such as `root`, UUID, aliases, local identity, receiver enablement, and access policy are rejected. |
| `brain env set <name>` | Interactive: prompt for the value. `skill_sessions` gets a themed add/edit/delete walkthrough instead of a value prompt (a JSON array is not something to type at a prompt); everything it writes is still settable as `brain env set skill_sessions '[…]'` or per field with `brain env set skill_sessions.0.prompt=…`. |
| `brain env set` | Interactive: pick a variable by number, then as above. |

#### The env breakdown

`brain env` shows the whole machine, the way `brain workspace list` does, rather
than only the selected record. It has four parts:

1. **`registry:`** — the absolute path of the `env.json` being read.
2. **Global** — every top-level `env.json` key that is *not* under
   `workspaces`, flattened to dotted paths. On a schema-v4 registry that is
   `schema_version` and `default_workspace`; an undeclared top-level key still
   lists, described generically, so nothing in the file is invisible.
3. **Workspaces** — one block per registered workspace, in canonical-name order,
   headed exactly like `workspace list` (`*` marks the default, and the labels
   read `(default)`, `(selected)`, or `(default, selected)`). Each block lists
   **every declared variable**, `(unset)` included, plus that workspace's own
   nested dotted paths. Every row resolves against **that** workspace's root and
   `env` map, so a block never shows a peer's value or a peer's legacy
   `config.json` fallback.
4. **Variables** — the legend: each name explained once, instead of repeating a
   long description on every row of every block. Nested dotted rows are covered
   by a single footnote rather than one legend line each.

Values distinguish three states: `(unset)` (absent), `(empty)` (present but an
empty string), and the value itself. Secrets render as `(set)` in every block,
including workspaces that are not selected.

**Redaction covers credentials, not identifiers.** `is_sensitive`
(`src/env/schema.rs`) redacts `twilio_auth_token`, `resend_api_key`,
`resend_webhook_signing_secret`, the `agent_capabilities` credential
descendants, and the sync transport secrets `sync.b2_app_key`,
`sync.crypt_password`, and `sync.crypt_password2`. Identifiers such as
`twilio_account_sid`, `sync.b2_bucket`, and `sync.b2_key_id` stay visible on
purpose: a user needs them to confirm which account and bucket a workspace
points at.

`brain env get` and `brain env set` are unchanged: both still act on the
**selected** workspace only, so `-w <workspace>` picks which record a write
lands in.

### The `brain sync` command

`brain sync` reads and drives the `sync` block above; the block itself is
written by **`brain sync setup`** (interactive: bucket + credentials,
validate the selected local manifest, verify or initialize remote workspace
identity, explicitly adopt a nonempty manifestless target when requested,
establish the baseline under the UUID sync lock), not by hand-editing
`env.json` or `brain env set`. See [features.md](features.md) for the full
command surface (`brain sync [--push|--pull] {setup|repair|status|conflicts}`)
and [integrations.md](integrations.md) for the rclone handoff.

The bucket must already exist. Setup probes the selected record's configured
bucket/path before persisting the candidate `sync` block. A matching strict
remote manifest proceeds; a demonstrably empty remote receives the exact local
manifest only after setup publishes an append-only UUID-named ownership claim,
enumerates and validates every claim, and wins deterministic UUID election.
The first publication of a new claim stages ownership and returns without
publishing a canonical manifest or saving credentials. A retry elects from the
durable claim set, then verifies the canonical manifest by read-back. A nonempty manifestless target first
shows the selected name/UUID, configured target, and observed status, then
requires a positive interactive confirmation or an exact matching
`--adopt-workspace-id <UUID>` flag. `--yes` alone is insufficient. Mismatch,
malformed, incompatible, or present-but-unreadable manifests, and unreachable
probes fail closed. Concurrent setup processes cannot share the selected
workspace lock, and different workspace UUIDs targeting the same empty remote
compete through remote claims; only the elected claimant may publish the
canonical manifest. Setup keeps its local UUID lock through remote identity,
task-schema preparation, and the complete initial baseline. It persists the
candidate machine-local credentials only after the baseline is classified
`Clean`; attention, abort, and transport-error outcomes leave them unsaved.
All later sync and check invocations load the same selected record's config and
repeat this identity gate before remote data work.

Optional `rclone crypt` is enabled by adding an already-obscured
`crypt_password` to the same machine-local `sync` block; `crypt_password2` is
an optional obscured salt. Generate those values with `rclone obscure` and
escrow the original passphrases in a password manager. brain stores only the
obscured rclone values and cannot recover encrypted remote data if the original
passphrases are lost.

Like `config`/`env`/`persona`/`skills`, `brain sync` is dispatched
**before** the `markdown-to-pdf` prerequisite gate (see below), so it works
even when that tool is missing.

#### Auto-sync triggers (`watch` / `debounce_ms`)

Two `sync`-block fields tune automatic change pushes. They are **brain env**
fields (machine-local, in `~/.config/brain/env.json`, never synced). The startup
pull always runs whenever sync is configured. A machine with no `sync` block
runs neither startup pulls nor a filesystem watcher.

| Field | Default | What it does | Disable with |
| --- | --- | --- | --- |
| `watch` | `true` | Watch the brain tree while the shell is open (native events where reliable; a one-second polling backend on macOS). After edits settle, it performs a one-way, non-deleting upload and does not download remote files. | `watch=false` |
| `debounce_ms` | `3000` | The watcher's quiescence window (ms): a sync fires once changes settle for this long, so a burst of edits coalesces into one sync. | lower/raise the number |

`SyncConfig::watch_effective()` folds `is_configured()` into `watch`, so the
watcher is on only when sync is actually configured *and* `watch` isn't
explicitly `false`. These flags live in the `sync` block written by
`brain sync setup`; `brain sync status` shows startup-pull, change-push, the
debounce window, and the receiver's two-hour message-pull policy.

There is no idle timer and no exit sync. Remote changes are always pulled at startup,
or immediately before an inbound SMS/email is dispatched when the most recent
successful downstream sync is more than two hours old. Legacy `on_start`,
`on_exit`, and `idle_pull_secs` keys in an existing JSON object are ignored.
See [features.md](features.md) for the user-facing behavior and
[data-model.md](data-model.md) for the schema.

**`rclone` is a soft prerequisite, not a startup gate.** Unlike
`markdown-to-pdf`, brain never blocks startup or any command on `rclone`
being installed — a missing `rclone` just makes `brain sync` itself fail when
it tries to spawn it. `brain tasks doctor` reports rclone's presence/version
and whether sync is configured as one informational line; an unconfigured (or
rclone-less) sync is a normal, healthy state.

### Structural workspace root and legacy back-compat

`root` is a required structural field on each schema-v4 `WorkspaceRecord`, not
a free-form env key. Workspace create/attach and the one-time legacy migration
establish it; ordinary commands use the immutable root snapshot in their
selected `WorkspaceContext`. `brain env set root=...` is therefore rejected
instead of allowing an env write to split record identity from its root.

The old `paths::brain_root()` / `brain_root_path()` resolution order remains a
compatibility seam for legacy migration only: pre-migration flat `root`, then
the read-only `~/.config/brain-root` pointer, then `~/brain`. It is not an
ordinary TUI, config, task, receiver-payload, or sync workspace selector.

**Migration.** When bootstrap finds an `env.json` that is invalid or not at the
current schema, Brain passes it through `env::migrate`. A valid current-schema
registry remains byte-for-byte unchanged. There are two distinct paths:

**Schema upgrade (v2 → v3 → v4), in place.** A registry at an older schema keeps
every record — UUID, root, aliases, local user, receiver intent, and the rest of
its env — and only moves machine-scoped values out of the records into the
top-level `env` map. v3 hoisted `markdown_to_pdf_path`; v4 hoists
`brain_receiver_public_url`, once one machine-wide webhook URL replaced the
per-workspace ingress path. Both hoists are the same rewrite, driven by
`env::MACHINE_GLOBAL_VARS`. If several records
carried one, the first in **canonical workspace-name order** wins and the rest
are dropped: they describe a single machine, so any is as good as
another, and choosing deterministically means every retry and every command
agrees. A blank value never displaces a real one. The exact previous bytes are
written to `env.json.legacy-backup` first, and the whole rewrite happens inside
the registry transaction, so an interrupted upgrade leaves the old file intact.
It runs on the next **ordinary** command — no user has to ask for it. Read-only
probes (`brain workspace list`, `brain sync status`, `brain receiver status`,
bare `brain receiver`, `brain receiver {email|phone}`,
`brain tasks doctor`) instead upgrade the value **in memory** and report
normally, because a status command must neither fail on an old schema nor write.

**Legacy flat env (pre-registry).** Ordinary selected-workspace startup and selected
`brain workspace repair` validate or seed only the selected root's portable
access mode; they do not inspect other registered roots. `brain workspace
list` and the explicit whole-registry migration path validate or seed every
registered root before succeeding. Any body that requires legacy migration is
interpreted as the legacy flat JSON object; invalid or non-object JSON is
treated as an empty object. Migration creates exactly one default record:

1. Root precedence remains flat nonblank `root`, then the nonblank legacy
   `~/.config/brain-root` pointer, then `<home>/brain`.
2. Leading `~` is expanded against the explicit home and the result is made
   absolute and lexically normalized without requiring the target directory to exist.
3. A valid root basename becomes the canonical workspace name; otherwise it
   falls back to `brain`.
4. Migration creates the root, then validates and adopts an existing strict
   portable manifest's workspace UUID and receiver-ingress UUID without
   rewriting it. Only when the manifest is absent does migration generate both
   identities and create the manifest. The record gets no aliases, an empty
   `local_user_id` for readiness, and the old receiver-enabled value in the
   dedicated field.
   Every other flat machine-local value except `root` moves unchanged into that
   record's `env`, including nested objects. Receiver and access-policy keys are
   not duplicated into `env`. Migration preserves an existing portable mode or
   seeds `access_mode: "unrestricted"` in portable config.

Before an existing flat file is replaced, its exact original bytes are written
beside it as `env.json.legacy-backup`; a collision uses the first free suffix
(`.1`, `.2`, and so on). Only then does the atomic registry save replace
`env.json`. A successful rerun keeps the same UUID and exact registry bytes,
creates no new backup, and reports no new migration. The pointer file is
compatibility input only: brain never creates, rewrites, or removes it.

For registry-only `workspace create` and `workspace attach`, an existing
`env.json`, legacy `$XDG_CONFIG_HOME/brain-root` pointer, or `<home>/brain`
directory counts as legacy-install evidence and is migrated before the new
record is added. Only a machine with none of those sources is treated as fresh,
so its requested create or attach becomes the first workspace.

The same pass preserves the older `markdown_to_pdf_path` relocation. If the
portable `config.json` still contains that key and the legacy env lacks it, the
value is folded into the new record before the registry write. Portable config
is cleaned up only after that write succeeds; a failed registry write leaves
the portable value intact. The explicit migration API returns typed errors,
while the existing startup wrapper remains nonfatal.

You can still hand-edit `~/.config/brain-root` before migration (or have a
dotfiles tool track it there). Brain only reads it as compatibility input. New
roots are established through `brain workspace create --root <path>` or
`brain workspace attach <path>`.

## brain config (`<brain-root>/.config/config.json`)

`brain` keeps its portable settings under the **brain config dir**,
`<brain-root>/.config/` (e.g. `~/brain/.config/`):

| File / dir | Holds |
| --- | --- |
| `config.json` | portable runtime knobs (`calendar_id`, triage settings, …) |
| `personalization.json` | content *about you* (name, role, who you work for, tag styles) |
| `extensions/<skill>.md` | additive personalization of a bundled skill (see [features](features.md)) |
| `plugins/<name>/` | whole user-owned skills installed alongside the bundled cores |

The config dir lives **inside the brain root**, so it travels with the brain:
whatever syncs the brain dir across your machines syncs the config too, and no
dotfiles tool is involved (`brain` never writes any external repo). Everything
in it is created on demand; a fresh checkout has none. Every value here is
meant to be identical on every machine — nothing machine-specific lives in
`config.json` anymore (see [brain env](#brain-env-configbrainenvjson) above for
what does).

This document is mostly about the **config store**
(`<brain-root>/.config/config.json`). Manage it with `brain config` rather than
editing it by hand (though hand-editing is fine). For personas see the [Personas](#personalization-personas) section below and
[data-model.md](data-model.md).

Access-mode initialization and `brain config set access_mode=...` parse an
existing file strictly, preserve unrelated object fields, and replace the file
from a synced same-directory temporary. Malformed JSON, a non-object value, or
an invalid stored mode is reported without changing the live bytes. A failed
replacement removes its temporary and leaves the prior file available for a
safe retry. TUI startup always parses `access_mode` and live TUI settings
strictly, so an invalid mode cannot silently fall back to unrestricted
behavior. When that validated mode is `unrestricted`, startup deliberately
does not deserialize `allowed_mcps` or `allowed_skills`, because neither field
is used by an unrestricted launch. `workspace_only` continues to parse both
lists strictly and fails closed when either is malformed.

### Receiver response configuration

Active receiver identity, authorization, and response routing live in portable
`.config/users.json` mappings. These three declared config variables name the
same three facts, so `brain config` resolves them **from the portable roster
first** and falls back to a value in `config.json` only when no portable user
answers — which is exactly the pre-migration case that key exists for:

| Variable | Live value | `config.json` value |
| --- | --- | --- |
| `response_email` | every portable user's `response_email`, comma-joined | legacy migration input |
| `allowed_sms_senders` | every inbound-allowed phone in the roster, comma-joined | legacy migration input |
| `allowed_email_senders` | every inbound-allowed email in the roster, comma-joined | legacy migration input |

Identities the roster lists but does **not** mark `inbound_allowed` are not
reported: that flag is the whole authorization decision, so listing a
disallowed number as allowed would misreport security state. One identity
shared by two people is listed once, in roster order.

`brain config list` prints a muted note under the table naming `users.json` as
the owning store and `brain user` as the command that edits it, and
`brain config set` **refuses** all three, because writing them to `config.json`
persists a value nothing enforces. The refusal names `brain user list`,
`brain user add`, and `brain receiver setup`. `brain config get` returns the
live value, so an agent reading one of these gets the answer brain actually
enforces rather than a stale legacy string.

Reporting these from `config.json` alone is what made a receiver configured
through `brain receiver setup` — which writes `users.json` and never
`config.json` — read as `(unset)`. See
[decisions.md](decisions.md#a-config-variable-that-another-store-answers-must-say-so).

Provider credentials are machine-local values in the selected workspace's
record in `~/.config/brain/env.json`. `brain receiver setup` prompts for the
credentials required by the selected SMS, email, or both-channel configuration
and stores them only in that record. Ordinary provider lookup never reads
`TWILIO_*`, `RESEND_*`, or `BRAIN_RECEIVER_PUBLIC_URL` from Brain's process
environment, so a workspace cannot inherit another workspace's shell values.
`brain env list` and `brain env get` redact secret values.

The setup prompt asks for one public base URL, such as
`https://brain.example.com`, and derives the machine's two webhook endpoints
`/sms` and `/email` from it. That origin is stored machine-global, so setting it
under any `-w` sets it for every registered workspace, and the confirmation says
so. Setup prints both URLs when it finishes, and `brain receiver url`
(optionally narrowed with `--sms` / `--email`) and `brain receiver status`
reprint them at any time from that one input — deliberately without consulting
receiver intent or a live server, since a provider portal is configured *before*
ingress is ever enabled. No URL names a workspace: brain routes each inbound
message by the number or address it arrived at, so `-w` cannot change what
`receiver url` prints. Both report which variable to set when this machine has no public base URL,
rather than printing a URL with a missing origin: the machine-wide `brain env
set` first, since it is the exact fix, then guided `brain receiver setup -w
<workspace>` **with** its selector, because that path also collects one
workspace's provider credentials and must not silently target the default. Provider values are saved as
strings in one selected-record transaction, including values that look numeric.
A shared validation step for guided and headless setup requires an HTTPS origin
without a path, query, fragment, or credentials; normalizes the Twilio sender to
E.164 and the Resend sender to lowercase email form; and rejects missing or
blank selected-channel values without echoing them. A missing channel
credential, public URL, user address, or explicit allowed state fails before
configuration is written. Guided `/clear` therefore cannot erase a value that
the selected channel requires. SMS sender matching is exact, so every configured phone number
must use the same E.164 form Twilio sends. Brain preserves the leading `+` when
writing and listing these values. Config files written by an older release
that stored one phone number as a JSON number are read and displayed with the
leading `+` restored.

Headless setup accepts `--channels`, provider flags, `--user-id`, optional
`--user-name` for creation, `--phone`/`--phone-allowed`, and
`--email`/`--email-allowed`. Supplying `--channels` without `--user-id` keeps
the selected channel but enters guided setup for the missing portable-user
mapping. A successful setup or `receiver set` asks only an
already-running shared process to reload the selected workspace UUID. No
receiver configuration command elects, restarts, or keeps a process alive.
Setup snapshots the selected record, portable users, and hook artifacts before
its first write. A later persistence or hook failure rolls back only values and
files that still equal this setup attempt's writes. Concurrent changes survive,
transaction-lock pathnames are never restored or unlinked, rollback errors are
aggregated with the original failure, and no live reload notification is sent.
Secret `brain env set` confirmations print only the variable name and `saved`,
for both direct assignment and interactive entry.

`receiver_enabled` is only persistent intent. Current acceptance is the
conjunction of that selected-record value and an unexpired exact-workspace TUI
lease. The four `brain receiver status -w <workspace>` rows keep those facts
separate: `Receiver`, `TUI`, `Server`, and `Accepting`. Bare `brain receiver`
repeats those four rows per registered workspace and adds the configured
`resend_from_email` and `twilio_from_number` — the addresses that route a
message to that workspace — above which it prints the machine's own
`brain_receiver_public_url` and the one webhook URL per channel derived from it;
`brain receiver email` and `brain receiver phone` print one of those addresses
alone.
Reading any of them uses
literal read-only bootstrap. It never fills in a missing access mode, migrates
or repairs registry/users state, renders skills, writes a render stamp or run
log, or starts a process. A live process is inspected through one
generation-bound control response; failures from that live process are
reported, not converted to `TUI not live`. Inspection does not expire leases
or otherwise mutate the server. Repair an incomplete workspace explicitly
before asking for its receiver status.

## The `brain config` command

| Command | Effect |
| --- | --- |
| `brain config list` | Print every variable, its effective value, and its description as an aligned table. Bare `brain config` also lists. |
| `brain config get <name>` | Print the effective value of one variable (explicit value, else built-in default). |
| `brain config set <name>=<value>` | Set a variable and persist it. Unknown names are rejected. Numeric/boolean values are stored with their JSON type. |
| `brain config set <name>` | **Interactive** (no `=value`): `namespaces` and `tags` open the toggle-checklist (see below); any other variable prompts once on `/dev/tty` for a value. |

Names are normalized (lowercased, `-`→`_`), so `brain config set Linear-Workspace=acme` works.

`namespaces` and `tags` are personalization (they live in `personalization.json`,
not `config.json`), but `brain config set` is a single front door for both: those
two names route to the interactive checklist that edits the personalization set,
while every other name is a config-store variable. `brain config set namespaces`
and `brain config set tags` (or the same via onboarding) show the current set with
every item pre-checked; space toggles, `a` adds new comma/semicolon-separated
items (tolerantly parsed), Enter saves. With no terminal, the checklist is skipped
(the set is left unchanged) and a scalar interactive set errors with a pointer to
the `name=value` form.

## Schema

| Variable | Default | Meaning |
| --- | --- | --- |
| `access_mode` | `unrestricted` | Portable agent boundary policy. `workspace_only` adds advisory trusted instructions and capability filtering; accepted values are exactly `unrestricted` and `workspace_only`. It is not a filesystem sandbox. |
| `allowed_mcps` | `[]` | Portable logical MCP names requested by workspace-only launches. Connection details and credentials belong only in the selected machine registry record. Accepts a JSON array or comma-separated names through `brain config set`. |
| `allowed_skills` | `["contacts","second-brain","todo","triage"]` when missing | Portable logical skill names requested by workspace-only launches. An explicit `[]` disables every skill. Accepts a JSON array or comma-separated names through `brain config set`. |
| `enable_triage_habits` | `true` | Portable managed-triage policy. `brain config set enable_triage_habits=true` reconciles one open daily and weekly chain. Setting `false` uses one durable grouped transaction to purge managed rows and derived references before committing config. Manual `/triage` remains available. |
| `linear_workspace` | *(unset)* | Linear workspace slug (e.g. `acme`). `config.rs` interpolates it into `https://linear.app/<slug>/issue/`, to which a task's `linear_issue` id is appended for the `Ctrl+O` "open link" action. Empty → no Linear links. |
| `daily_triage_name_pattern` | `Morning Triage` | Case-insensitive regex matched against habit *names* to find the habit that gates the tasks view's startup triage nudge. Empty (or invalid regex) disables it. Read by `config.rs`. |
| `enable_daily_triage_check` | `true` | Portable startup-nudge policy. `false` means no shell launched against this workspace ever opens the daily-triage modal; the post-sync refresh gate still runs. Accepts exactly `true` or `false`. The command palette's Disable/Enable daily triage alert row flips the same state for one running session without writing config. Read by `config.rs`. |
| `day_rollover_hour` | `6` | Local hour (0-23) the "logical day" rolls over for the triage re-check on refresh. Out-of-range → default. Read by `config.rs`. |
| `skills_auto_sync` | `true` | When `true`, the bundled skills are auto-rendered into the selected workspace's `.agents/skills` directory on two triggers: a `config`/`personalize` mutation (`skills::resync_skills`), and the first ready-workspace invocation after the brain binary's version changes (`skills::resync_on_version_change`). Default `true`; set `false` to manage workspace skills only via explicit `brain skills sync`. Read by `src/skills/`. |

`markdown_to_pdf_path`, `claude_cmd`, `codex_cmd`, and `opencode_cmd` are **not** in this table
— they live in [brain env](#brain-env-configbrainenvjson)
(`brain env set markdown_to_pdf_path=…`,
`brain env set claude_cmd=…`, `brain env set codex_cmd=…`,
`brain env set opencode_cmd=…`), since they are
machine-specific values.

Every variable is optional; a missing file or missing field falls back to the
default above. The brain directory is the selected `WorkspaceContext::root()`;
only one-time legacy migration consults `paths::brain_root_path()` and the old
pointer/default precedence. The runtime knobs
(`access_mode`, `allowed_mcps`, `allowed_skills`, `enable_triage_habits`,
`enable_daily_triage_check`, `daily_triage_name_pattern`, `linear_workspace`,
`day_rollover_hour`) are read
by `config.rs::Config`; they all read the same `config.json` and ignore fields
they don't use. Agent launch commands are resolved from the selected machine
record by `agent::configured_command` instead.

## The `markdown-to-pdf` prerequisite

`markdown-to-pdf` is a **hard prerequisite** — brain spawns it for the "Create
PDF" command. Its location is not hardcoded (the repo is public), so:

1. On first run brain **auto-discovers** it, in order: an executable named
   `markdown-to-pdf` on `$PATH`; then conventional bin dirs (`~/.local/bin`,
   `/usr/local/bin`, `/opt/homebrew/bin`, `~/bin`); then the login shell, which
   resolves an autoloaded shell-function wrapper to the script it wraps.
2. The first hit is persisted to `markdown_to_pdf_path` **in brain env**
   (`~/.config/brain/env.json`) — not `config.json`.
3. At every startup the configured path is validated. If it is set but
   missing/not executable on *this* machine, brain re-runs discovery and heals
   the value automatically. Only if it is unset (or invalid) **and** discovery
   finds nothing does brain print a red `❌` error and exit, telling you to run
   `brain env set markdown_to_pdf_path=/path/to/markdown-to-pdf`.

The `brain config …` and `brain env …` commands themselves are exempt from this
gate, so you can always `env set` your way out of a bad path.

## Testing the loaders

The IO-touching wrappers are thin; the decisions worth testing are pure:

- `settings/` units — schema resolution, the `config list` table layout, the
  prerequisite message wording, shell-output path extraction, value coercion.
- `env/` units — the writable env schema/vars (`markdown_to_pdf_path`,
  `claude_cmd`, `codex_cmd`, `opencode_cmd`), structural-name rejection, and the
  migration `plan` (legacy pointer→record `root`, config→env `markdown_to_pdf_path`
  relocation), and the store round-trip.
- `sync::config` units — `SyncConfig` field defaults, `is_configured`,
  `watch_effective`; plus `sync::args` (the bisync argv per direction),
  `sync::remote` (creds land only in env, never the arg), `sync::run` (parsing
  rclone's transferred/deleted/error/abort output), `sync::verify` (outcome
  classification), `sync::conflicts` (friendly-name rewriting), and
  `sync::command` (hostname, direction/label mapping, status formatting). See
  [data-model.md](data-model.md).
- `paths::parse_root_key` — reading the legacy flat or schema-default record
  root during compatibility migration.
- `paths::resolve_root` — the legacy flat root → read-only pointer → default
  precedence.
- `paths::parse_brain_root_file` — reading the legacy pointer file, empty-is-unset.
- `paths::expand_tilde_with_home` — tilde expansion against an explicit home.
- `paths::machine_config_dir_from` — the XDG-vs-`~/.config` precedence for the
  brain-env directory.
- `config.rs` units — `linear_base_url` interpolation, defaults, and ignoring
  unknown keys.

See those modules' unit tests and `tests/root_resolution.rs`.

## Personalization (personas)

Personalization is content *about the people using a workspace*, stored beside
`config.json` in the brain config dir at
`<brain-root>/.config/personalization.json`. It is just another brain config,
inside the brain root, so it travels with the brain and every machine sees the
same people. Manage it with `brain persona` (see [features.md](features.md));
the schema lives in [data-model.md](data-model.md).

Neither of the two store questions above decides this file: a persona is not a
setting a machine or a workspace holds *one* of, it is a fact about a **person**.
So the file is keyed by portable user ID — the same IDs as `users.json` — with
one persona object per member:

```json
{
  "schema_version": 2,
  "personas": {
    "pablo": { "name": "Pablo", "role": "CEO", "works_for": "Avandar" },
    "sam":   { "name": "Sam", "role": "designer", "works_for": "myself" }
  }
}
```

Each persona object holds:

| Field | Meaning |
| --- | --- |
| `name` | Optional display name. |
| `role` | Free-text role the assistant serves (e.g. `CEO`, `engineer`, `student`). The generic *rule* "act as a personal assistant" stays in the skill; only the *who* is personalized. |
| `works_for` | Org that person works for, `myself`, or empty. |
| `namespaces` | Their project `<namespace>__<outcome>` life-buckets. Empty falls back to the generic defaults. |
| `tag_styles` | Map of `tag → { emoji, label }` layered over the generic defaults (`mit`/`personal`/`work`). Unknown tags render as their raw name. |

Reads that concern one person (the task renderer's tag styles, the namespace
checklist, the skill-lookup block) use **this machine's local user** unless a
command names another. `brain persona list` is the all-members view.

**Schema 1 → 2.** The previous file was a single unowned persona object with the
fields above at the top level. It is migrated on read, keyed onto the local user
of whichever machine reads it first, and rewritten in the keyed schema on the
next write. An empty legacy file migrates to *no* personas rather than handing
the local user a blank record.

Two sibling stores live under the same hidden `<root>/.config/` dir and also
sync with the brain (see [features.md](features.md) for how they customize skills):

- `<root>/.config/extensions/<skill>.md` — per-skill **extensions** injected into
  a bundled skill's built copy.
- `<root>/.config/plugins/<name>/` — whole user **plugins** installed alongside
  the bundled skills.

A missing or broken personalization file parses to no personas — the app runs
fine with none, and skills fall back to generic behavior. A member with no
persona is prompted for one on their own machine's next `brain` command, and
reported (never prompted for) as the `other members' personas` optional feature in
`brain workspace status`. Any
`persona`/`config` mutation triggers the active deterministic skill
render-and-install pipeline (`skills::resync_skills`) so the installed skills
stay in sync. The first ready-workspace invocation after a Brain version change
also runs that pipeline when `skills_auto_sync` is enabled.

## Persistent state (`~/.cache/brain/workspaces/<workspace-id>/state.db`)

Neither config store is the only *user-edited* state. The **persistent brain
shell** also keeps machine-managed state in a SQLite DB at
`~/.cache/brain/workspaces/<workspace-id>/state.db` (created on first run; see `state.rs` and
[data-model.md](data-model.md)):

- `brain_sessions` records Claude, Codex, and OpenCode session identity plus workspace,
  actor, and channel attribution, with a per-session PID lock used for scoped
  lock-and-recency resume. Written by Brain and the generic session-start bridge.
- `meta`: small key/value store. `panel_side` (`"left"` or `"right"`) records
  the panel layout; `skills_synced_version` records the last Brain version that
  successfully rendered this workspace's installed skills.

You don't edit a workspace state DB by hand. Deleting it is safe: brain recreates it,
starts a fresh agent session, and reverts to the default right-side layout.
The `brain config`, `brain env`, and `brain tasks {complete,doctor,--no-tui}`
utility paths never touch it.

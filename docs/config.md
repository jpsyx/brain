# Configuration

`brain` splits its persisted state across **two config stores**, by lifecycle.
The machine-global path has one owner: the versioned workspace registry. Each
workspace record silos the machine-local values that belong to that workspace;
they are never inherited or merged from the default or another record.

| Store | Path | CLI | Synced? | Holds |
| --- | --- | --- | --- | --- |
| **brain env / workspace registry** | `$XDG_CONFIG_HOME/brain/env.json` (fallback `~/.config/brain/env.json`, outside every brain root) | `brain workspace …` manages records; `brain env …` reads and writes the already-selected record | **No**: machine-local, never rides any workspace sync | Schema-v2 canonical default plus siloed workspace records (`workspace_id`, `root`, aliases, local user, receiver switch, and per-workspace machine env) |
| **brain config** | `<brain-root>/.config/config.json` (e.g. `~/brain/.config/config.json`) | `brain config {list\|get\|set}` | **Yes** — travels with the brain | `linear_workspace`, triage settings, `response_email`, and SMS/email sender allowlists |

The rule of thumb: **brain env holds anything that would be *wrong* if copied to
another machine** — absolute paths, machine-specific binaries, secrets, and
machine-specific frontend launch commands.
**brain config holds anything that's *right* on every machine** — slugs,
preferences, behavior flags. [Personalization](#personalization) (below) is a
third store, content *about you*, which also lives inside the brain root and
syncs with it alongside `config.json`.

Both CLIs run **before** the `markdown-to-pdf` prerequisite gate, so you can
always repair a broken environment or config even when that tool is missing.
Both normalize names the same way (lowercased, `-`→`_`).

## Machine workspace registry (`~/.config/brain/env.json`)

`~/.config/brain/env.json` is the sole machine-global workspace registry
(`$XDG_CONFIG_HOME/brain/env.json` when XDG config is set). Schema version `2`
stores a canonical default and a sorted map of canonical workspace names to
complete `WorkspaceRecord` values. Each record owns its own machine root,
immutable UUID, aliases, `local_user_id`, `receiver_enabled` switch, and `env`
object. The `env` object is siloed: selecting a canonical name, an alias, or the
default returns only that record, with no copying or merging from any other
workspace. Portable access policy never lives in this machine-local file.

Registry loads accept only exact schema version `2`, a non-empty record map, a
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
`[a-z0-9][a-z0-9_-]*`. `--brain <selector>` and `-b <selector>` resolve either
kind before or after a subcommand. An omitted selector uses only the canonical
`default_workspace`. The first record becomes default; later create and attach
operations preserve it. Rename preserves the UUID and updates the default name
when needed. Changing the default workspace never changes access mode, root,
local user, receiver enablement, aliases, identity, or env. Remove detaches
only the machine record and never deletes the root.

`brain workspace` explicitly loads this schema-v2 registry and applies every
mutation through `RegistryStore`'s interprocess transaction and atomic-save
boundaries. Startup migration and selected-record `brain env` writes use the
same lock, so they cannot overwrite a workspace command.
Its global `--brain/-b` selector resolves canonical names and aliases once at
the bootstrap boundary. Ordinary commands receive a ready selected context;
env writes verify both canonical name and immutable UUID, while config,
personalization, tasks, reindex, sync, receiver setup, and the TUI consume that
same context and its once-resolved actor. Changing the default or local user
after bootstrap cannot redirect or reattribute a read or write already in
progress.
Without a portable user store, legacy compatibility accepts only an exact
lower-case kebab `local_user_id`. A malformed nonblank legacy value is rejected
with `brain workspace repair -b <workspace> --local-user-id <USER_ID>`; Brain
does not create `users.json` as part of that repair path.

### The `brain workspace` command

| Command | Effect |
| --- | --- |
| `brain workspace list` | Deterministically list canonical records with the default marker, root, aliases, local user, receiver state, and the root's portable `access_mode` when available. Empty/unavailable setup is explicit. |
| `brain workspace create [--name <name>] [--root <path>]` | Validate the complete candidate, create the normalized root and strict portable manifest, then register the same UUID; root basename supplies an omitted name. A later persistence failure preserves the manifest and every directory path the invocation created for manual cleanup. |
| `brain workspace attach [<root>]` | Validate a strict compatible manifest in an existing root and register its UUID without editing root contents. Invalid or colliding identities leave registry bytes unchanged. |
| `brain workspace rename [<workspace>] [<name>]` | Rekey the canonical name while preserving the complete record and updating the default if needed. |
| `brain workspace alias {add\|remove} [<workspace>] [<alias>]` | Add or remove an alternative case-folded selector. A duplicate alias on the same record is an actionable error and leaves bytes unchanged. |
| `brain workspace default [<workspace>]` | Set the canonical default through a canonical-name or alias selector. |
| `brain workspace remove [<workspace>]` | Detach only the registry record; root and every local/remote runtime artifact remain untouched. |
| `brain workspace repair [--manifest] [--local-user-id <id>]` | Recreate a missing manifest that matches the registry and/or set this machine's local identity. Omitting both flags uses the interactive prompt. |

Every optional grammar value has a `/dev/tty` prompt when omitted and a flag
or positional noninteractive form. For create, attach, remove, and repair,
bootstrap collects and validates the complete request before legacy
classification or migration. EOF/cancellation therefore leaves legacy env and
pointer bytes, the root tree, manifests, backups, and registry bytes unchanged.
Complete noninteractive forms skip terminal IO and then perform any required
migration before executing the prepared request. Workspace commands run before
the `markdown-to-pdf` gate; on a genuinely fresh machine, first
`create`/`attach` can therefore establish the initial schema-v2 registry
without migration inventing a competing default.

### Portable manifest and readiness

Each workspace root carries `<brain-root>/.config/workspace.json`. Schema `1`
contains the workspace UUID, a stable receiver ingress UUID, and the minimum
compatible Brain version. Parsing rejects unknown fields, unsupported schema
versions, invalid UUIDs, and a minimum version newer than the running binary.
The manifest UUID must equal the selected machine-registry UUID.
The manifest is create-only and strict: create publishes it only when the path
is absent, attach reads it without editing, and unknown fields or identity
mismatches fail rather than silently replacing portable identity.

The same directory carries strict schema-1 `users.json` when portable people
have been configured. It contains person IDs, display names, normalized phone
and email identities, inbound-enabled flags, and optional response emails.
The file travels with the workspace; the selected person's `local_user_id`
remains in the machine registry.

First-person setup asks for an email identity only when the workspace email
receiver allowlist is non-empty. A legacy `response_email` supplies the default
and migrates only when its normalized value matches that allowlist. A response
setting alone does not enable inbound email or create a portable identity.

Create and attach are registry-only setup commands, so they can establish an
incomplete record. Before every ordinary command, Brain then requires manifest
agreement and, when `users.json` exists, a local ID that names one portable
person. An interactive first-use flow creates and selects the first person; a
headless invocation reports exact `brain user add` and `brain user local`
commands. An existing workspace with no `users.json` and a non-empty legacy
local ID stays ready without being rewritten. Version/help and hidden internal
server execution perform no workspace IO or prompt.

### Access policy status

Access-mode enforcement is not part of the current foundation. The migrated
or default workspace remains unrestricted unless a later access-policy phase
explicitly configures it otherwise. Planned `workspace_only` behavior uses
prompt-based guidance and light guardrails. It is not a filesystem sandbox,
authentication boundary, container, OS-account boundary, or protection from a
malicious trusted user. Its purpose is only to reduce accidental and naive
cross-workspace leakage in a high-trust self-hosted environment.

Inbound request actor selection now reads `users.json`: provider signatures are
verified first, then the normalized sender must match an enabled phone or email
identity. Legacy receiver allowlists and response settings remain compatibility
inputs while the coordinated portable schema migration stays deferred. Task
`assigned_to`, triage-habit policy, the agent-controller/OpenCode facade, and
the final shared receiver lifecycle remain later phases.

### Selected workspace env

Machine-local env values live inside the selected workspace record at the
fixed registry path. They do **not** depend on the workspace root and never
ride whatever syncs that root (Backblaze, a cloud drive, etc.). Structural
record fields are managed by `brain workspace`, not exposed as free-form env.

| Variable | Default | Meaning |
| --- | --- | --- |
| `markdown_to_pdf_path` | *(auto-discovered)* | Path to the `markdown-to-pdf` command on **this machine**. Lives in brain env (not brain config) because it's a machine-specific binary path, never "right" on every machine. See below. |
| `claude_cmd` | `claude --dangerously-skip-permissions` | Command that launches the brain panel's default Claude frontend on **this machine**. brain appends `--resume`/`--session-id` after it, so the value is the base command plus any of its own flags. Blank falls back to the default. If unset, a legacy `brain config claude_cmd` value is honored for back-compat. |
| `codex_cmd` | `codex` | Command that launches the brain panel's Codex frontend on **this machine**. brain appends `resume <id>` only when it has a Codex session id to resume; fresh Codex panels launch without Claude-only `--session-id` / `--resume` flags. Blank falls back to `codex`. |
| `sync` | *(absent → disabled)* | Backblaze B2 cross-machine sync config: `enabled`, `b2_bucket`, `b2_path`, `b2_key_id`, `b2_app_key`, optional `rclone crypt` fields (`crypt_password`, `crypt_password2`, `crypt_filename_encryption`, `crypt_directory_name_encryption`), `watch`, `debounce_ms`, `max_delete_percent`, `exclude`, `max_size`. Drives manual sync plus the mandatory startup pull and change-triggered pushes; there is no periodic idle pull. Written by **`brain sync setup`**, not raw `brain env set`. See [data-model.md](data-model.md) for the field-by-field schema. |

### The `brain env` command

Mirrors `brain config` exactly, over the env store:

| Command | Effect |
| --- | --- |
| `brain env list` | Print every env value, including recursively nested objects, using dot-separated paths such as `sync.b2_bucket`. Bare `brain env` also lists. |
| `brain env get <name>` | Print the effective value of one variable or dotted nested path, such as `sync.b2_bucket`. |
| `brain env set <name>=<value>` | Set a declared scalar variable or dotted nested env path in the selected record, preserving sibling values. Structural record fields such as `root`, UUID, aliases, local identity, receiver enablement, and access policy are rejected. |

### The `brain sync` command

`brain sync` reads and drives the `sync` block above; the block itself is
written by **`brain sync setup`** (interactive: bucket + credentials,
verify/create the bucket, establish the baseline), not by hand-editing
`env.json` or `brain env set`. See [features.md](features.md) for the full
command surface (`brain sync [--push|--pull] {setup|repair|status|conflicts}`)
and [integrations.md](integrations.md) for the rclone handoff.

Optional `rclone crypt` is enabled by adding an already-obscured
`crypt_password` to the same machine-local `sync` block; `crypt_password2` is
an optional obscured salt. Generate those values with `rclone obscure` and
escrow the original passphrases in a password manager. brain stores only the
obscured rclone values and cannot recover encrypted remote data if the original
passphrases are lost.

Like `config`/`env`/`personalize`/`skills`, `brain sync` is dispatched
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

`root` is a required structural field on each schema-v2 `WorkspaceRecord`, not
a free-form env key. Workspace create/attach and the one-time legacy migration
establish it; ordinary commands use the immutable root snapshot in their
selected `WorkspaceContext`. `brain env set root=...` is therefore rejected
instead of allowing an env write to split record identity from its root.

The old `paths::brain_root()` / `brain_root_path()` resolution order remains a
compatibility seam for legacy migration only: pre-migration flat `root`, then
the read-only `~/.config/brain-root` pointer, then `~/brain`. It is not an
ordinary TUI, config, task, receiver-payload, or sync workspace selector.

**Migration.** When an invocation's bootstrap policy permits registry access,
brain checks `env.json` through `env::migrate`. A valid schema-v2 registry is a
byte-for-byte no-op and does not inspect the default workspace's portable
config. Any other
body is interpreted as the legacy flat JSON object; invalid or non-object JSON
is treated as an empty object. Migration creates exactly one default record:

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
   not duplicated into `env`; portable access setup remains required.

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
editing it by hand (though hand-editing is fine). For personalization see the
[Personalization](#personalization) section below and
[data-model.md](data-model.md).

### Receiver response configuration

These portable values configure who may issue remote brain messages and where
long-form SMS responses are delivered:

| Variable | Meaning |
| --- | --- |
| `response_email` | The user's email address for long responses requested over SMS. |
| `allowed_sms_senders` | Comma-separated E.164 phone numbers permitted to send SMS/MMS messages, including the leading `+` and country code (for example, `+16072809118`). |
| `allowed_email_senders` | Comma-separated email addresses permitted to issue brain messages and participate in automatic thread replies. |

Provider credentials are machine-local values in the selected workspace's
record in `~/.config/brain/env.json`. `brain receiver setup` prompts for the
credentials required by the selected SMS, email, or both-channel configuration
and stores them only in that record. Ordinary provider lookup never reads
`TWILIO_*`, `RESEND_*`, or `BRAIN_RECEIVER_PUBLIC_URL` from Brain's process
environment, so a workspace cannot inherit another workspace's shell values.
`brain env list` and `brain env get` redact secret values.

The setup prompt asks for one public base URL, such as
`https://brain.example.com`, and derives the exact webhook endpoints
`/sms` and `/email`. A missing credential, public URL, or sender allowlist
fails closed. SMS sender matching is exact, so every configured phone number
must use the same E.164 form Twilio sends. Brain preserves the leading `+` when
writing and listing these values. Config files written by an older release
that stored one phone number as a JSON number are read and displayed with the
leading `+` restored.

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
| `enable_triage_habits` | `true` | Portable managed-triage policy. `brain config set enable_triage_habits=true` reconciles one open daily and weekly chain. Setting `false` uses one durable grouped transaction to purge managed rows and derived references before committing config. Manual `/triage` remains available. |
| `linear_workspace` | *(unset)* | Linear workspace slug (e.g. `acme`). `config.rs` interpolates it into `https://linear.app/<slug>/issue/`, to which a task's `linear_issue` id is appended for the `Ctrl+O` "open link" action. Empty → no Linear links. |
| `daily_triage_name_pattern` | `Morning Triage` | Case-insensitive regex matched against habit *names* to find the habit that gates the tasks view's startup triage nudge. Empty (or invalid regex) disables it. Read by `config.rs`. |
| `day_rollover_hour` | `6` | Local hour (0-23) the "logical day" rolls over for the triage re-check on refresh. Out-of-range → default. Read by `config.rs`. |
| `skills_auto_sync` | `true` | When `true`, a `config`/`personalize` mutation re-renders and installs the bundled skills into the agent registry (`skills::resync_skills`). Default `true` since the B4 cutover; set `false` to manage the registry only via explicit `brain skills sync`. Read by `src/skills/`. |

`markdown_to_pdf_path`, `claude_cmd`, and `codex_cmd` are **not** in this table
— they live in [brain env](#brain-env-configbrainenvjson)
(`brain env set markdown_to_pdf_path=…`,
`brain env set claude_cmd=…`, `brain env set codex_cmd=…`), since they are
machine-specific values.

Every variable is optional; a missing file or missing field falls back to the
default above. The brain directory is the selected `WorkspaceContext::root()`;
only one-time legacy migration consults `paths::brain_root_path()` and the old
pointer/default precedence. The runtime knobs
(`enable_triage_habits`, `daily_triage_name_pattern`, `linear_workspace`, `day_rollover_hour`) are read
by `config.rs::Config`; they all read the same `config.json` and ignore fields
they don't use. Agent launch commands are read by `env::claude_command` and
`env::codex_command` instead.

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
  `claude_cmd`, `codex_cmd`), structural-name rejection, and the
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

## Personalization

Personalization is content *about you*, stored beside `config.json` in the brain
config dir at `<brain-root>/.config/personalization.json`. It is just another
brain config, inside the brain root — it lives inside the brain root and travels
with the brain. Manage it with `brain personalize` (see [features.md](features.md));
the schema lives in [data-model.md](data-model.md).

| Field | Meaning |
| --- | --- |
| `name` | Optional display name. |
| `role` | Free-text role the assistant serves (e.g. `CEO`, `engineer`, `student`). The generic *rule* "act as a personal assistant" stays in the skill; only the *who* is personalized. |
| `works_for` | Org you work for, `myself`, or empty. |
| `tag_styles` | Map of `tag → { emoji, label }` layered over the generic defaults (`mit`/`personal`/`work`). Unknown tags render as their raw name. |

Two sibling stores live under the same hidden `<root>/.config/` dir and also
sync with the brain (see [features.md](features.md) for how they customize skills):

- `<root>/.config/extensions/<skill>.md` — per-skill **extensions** injected into
  a bundled skill's built copy.
- `<root>/.config/plugins/<name>/` — whole user **plugins** installed alongside
  the bundled skills.

A missing or broken personalization file parses to empty — the app runs fine
with no personalization, and skills fall back to generic behavior. Any
`personalize`/`config` mutation triggers a skill re-render (`skills::resync_skills`)
so the installed skills stay in sync; the render pipeline itself is a later
sub-project (the trigger is wired now, currently a no-op).

## Persistent state (`~/.cache/brain/workspaces/<workspace-id>/state.db`)

Neither config store is the only *user-edited* state. The **persistent brain
shell** also keeps machine-managed state in a SQLite DB at
`~/.cache/brain/workspaces/<workspace-id>/state.db` (created on first run; see `state.rs` and
[data-model.md](data-model.md)):

- `brain_sessions` records Claude and Codex session identity plus workspace,
  actor, and channel attribution, with a per-session PID lock used for scoped
  lock-and-recency resume. Written by both `brain` and the SessionStart hook.
- `meta` — small key/value store; today just `panel_side` (`"left"` or
  `"right"`), the side the brain panel sits on, set by the palette's "Move
  brain panel…" command and read on startup.

You don't edit a workspace state DB by hand. Deleting it is safe: brain recreates it,
starts a fresh agent session, and reverts to the default right-side layout.
The `brain config`, `brain env`, and `brain tasks {complete,doctor,--no-tui}`
utility paths never touch it.

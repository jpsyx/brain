# Data model

Most of `brain`'s "model" is the in-memory representation of the selected
workspace's directory tree plus the picker's match state. The **persistent
shell** adds a UUID-scoped SQLite store (sessions + layout), described under
"Persistent state" below.

## `Bucket` (`entry.rs`)

```rust
enum Bucket { Projects, Areas, Resources, Archive }
```

The four PARA top-level buckets `brain` searches. **Declaration order is
display order**: the picker groups sections Projects → Areas → Resources →
Archive by relying on the derived `Ord`. `label()` returns the human string
("Projects", etc.). Archive sorts last: it's retired material, so it's
still searchable but stays out of the way of live work.

## `Entry` (`entry.rs`)

```rust
struct Entry {
    path: PathBuf,   // absolute path on disk; handed to `open` / the editor / trash
    display: String, // `~/<selected-root-name>/...` form when root is below HOME
    bucket: Bucket,  // which section it renders under
}
```

`collect(brain, roots)` produces these by walking each `(Bucket, root)`
pair with `walkdir`:

- **Hidden files are skipped** — any path component starting with `.`
  (`.git`, `.DS_Store`, dotfiles). This mirrors the old `fd .` default.
- **The root itself is skipped** (`depth() == 0`); only its contents are
  pickable.
- **`display` rewrites a selected root below `$HOME` to
  `~/<selected-root-name>/...`** via `display_path`, which strips the selected
  root's parent prefix. Paths outside that prefix fall back to absolute form.
- **Missing roots are silently skipped**, so a brain without an `areas/`
  dir doesn't error.

Both files *and* directories are collected, so you can pick (and reveal /
cd into) a folder, not just a leaf note.

## Picker match model (`picker/`)

### `HaystackBuf` — slug-aware matching

Each entry's `display` is preprocessed once into a `HaystackBuf`:

- `normalized`: the display string with slug separators (`-`, `_`, `.`)
  removed. nucleo matches against this, so a query word like `afloat`
  finds the slug `ann-afloat` without the dash breaking the contiguous
  run. `ann-afloat` → `annafloat`.
- `normalized_char_to_display_byte`: for each char in `normalized`, the
  byte offset of that same char in the original `display`. Built at
  startup so the highlight indices nucleo returns (char positions over a
  `Utf32Str`) translate cheaply to display byte offsets at render time.

`char_positions_to_byte_positions` does that translation; the resulting
`BTreeSet<usize>` of display byte offsets is what `render::entry_line`
colors.

### `Match` and `DisplayRow`

```rust
struct Match { entry_idx, bucket, score, highlight_bytes }
enum DisplayRow { Header(Bucket, count), Match(usize) }
```

`refilter()`:

1. If the query is empty, every entry becomes a zero-score match.
   Otherwise nucleo scores each haystack with a smart-case, substring
   `Pattern`; non-matches are dropped.
2. Matches are sorted by **bucket** (P → A → R → Archive), then **score**
   (descending), then **walk order** (`entry_idx`) as a stable tiebreak.
3. `build_display_rows` walks the sorted matches and inserts one
   `Header(bucket, n)` before each contiguous run of a bucket's matches,
   producing the interleaved render list. Selection only ever lands on
   `Match` rows; the cursor (`selected`) indexes into `matches`, and
   `ensure_visible` keeps the section header directly above the selected
   match on screen.

### Overlays: palette + Create-PDF confirm

`App` carries two optional modal overlays that take key routing before the
picker itself:

- `palette: Option<menu::MenuApp>` — the command palette (`Ctrl-p`). Its row
  list is contextual: when the highlighted entry is a `.md` file, a leading
  **"Create PDF for '…'"** row (`Choice::CreatePdf`) is added, keyed off
  `App::selected_markdown_filename`; when any entry is highlighted, a trailing
  **"Delete '…'"** row (`Choice::Delete`) is added, keyed off
  `App::selected_filename`.
- `confirm: Option<confirm::Confirm>` — the shared yes/no modal, holding the
  target `path`, a `ConfirmKind` (`Pdf` → green, defaults Yes; `Delete` → red,
  defaults No), and which button is highlighted. It routes **before** the
  palette. On `Accept` (driven by `tui/search_view.rs`): Pdf converts the file
  in place and Delete trashes the path, then the entry is dropped/refreshed and
  the shell stays open.

## Workspace identity (`workspace/`)

`WorkspaceContext` is the immutable in-memory identity for one workspace:

```rust
struct WorkspaceContext {
    id: WorkspaceId,       // immutable UUID identity
    name: WorkspaceName,   // canonical [a-z0-9][a-z0-9_-]* slug
    root: PathBuf,         // absolute lexical path, resolved once
    local_user_id: String, // this machine's selected portable person
    paths: WorkspacePaths, // machine-local paths keyed by id
}
```

Its fields are private and read only through `id()`, `name()`, `root()`,
`local_user_id()`, and `paths()`. This prevents callers from constructing or
mutating a context whose UUID, root, and UUID-derived runtime paths disagree.

`WorkspaceName::parse` trims and lower-cases input, then accepts only canonical
slugs. `WorkspaceName::from_root` derives a name from a root's final component.
`WorkspaceId` wraps a UUID and is the stable identity even if the display name,
root, aliases, or a machine-local default change later.

`WorkspaceContext` stores an already-normalized root. Relative input is resolved
against an explicit supplied current directory; lexical `.` and `..` components
are collapsed without filesystem access, so a missing root and a symlinked root
stay valid inputs. A relative root requires an absolute injected base;
`WorkspaceContext::new` otherwise returns a typed error rather than storing a
relative path. It intentionally carries no alias, default, or registry
reference.

### Shared-server lease routing (`server/lifecycle/`)

The shared process receives only live TUI registrations. Its pure
`LeaseTable` records each `WorkspaceLease` by stable `WorkspaceId` and keeps a
separate catalog of previously seen opaque `IngressId` values. A lease contains
its unique `LeaseId`, canonical workspace name, ingress ID, TUI PID,
workspace-local job socket, receiver-enable snapshot, and monotonic expiry.
It contains no root, registry environment, user identity, credential, prompt,
log, or inbound message data.

`IngressId` is a distinct UUID newtype even though the current portable
manifest still serializes `receiver_ingress_id` through `WorkspaceId`. The two
types preserve exactly the same UUID-string representation at that boundary;
conversion is explicit so a public ingress can never be used accidentally as a
workspace selector. A future manifest migration is therefore unnecessary for
this type split.

At every injected monotonic instant ordinary table operations filter expired
leases without removing them. The shared control and watchdog transition owns
revoke-aware removal. Registration rejects an incoming lease whose expiry is
already elapsed and accepts one
live lease per workspace, ingress, and lease ID. An exact replay of the current
workspace's lease, canonical name, ingress, PID, and derived socket is an
idempotent retry that refreshes expiry and authoritative enablement after a
lost response. A different lease or changed identity still conflicts. It renews only the matching
lease ID and keeps a known ingress after orderly removal or expiry. Routing is
therefore one of `Accepting(live lease)`, `Disabled` (a live lease exists but
the receiver is off), `NoLiveTui` (known but no live lease), or `Unknown`. Expired
leases are never returned. Removing or expiring the final live lease yields
`ShutdownNow`; otherwise the process receives `KeepRunning`. The production
schedule uses a one-second heartbeat and five-second TTL, while tests inject
their own `LeaseTiming` and never sleep. Once pruning removes the final lease,
the table latches that shutdown decision until a successful replacement
registration. A failed late heartbeat, receiver update, or rejected
registration therefore cannot consume the final-expiry signal before the
watchdog observes it.

Watchdog expiry first removes exact lease authority under the control-state
mutex, then revokes matching admissions outside it with one absolute deadline.
Pending and authorized admissions become cancelled; work whose socket commit
already linearized may finish. Orderly disable and unregister wait only to the
request deadline, and a timeout leaves their lease mutation unapplied.
Ordinary lease-table paths filter expired leases without removing them. Shared
control and watchdog entry use the single revoke-aware removal transition.
Final socket commit performs persisted-intent IO outside the control mutex,
then samples exact TTL, revalidates the route and admission identity, and
performs the admission CAS within one control-mutex operation.

Every mutating control request is tagged with the process generation. A stale
generation yields `StaleGeneration` without touching the table. Registration
contains workspace, lease, and ingress UUIDs, canonical name, TUI PID, and the
TUI-resolved root plus UUID-local job socket. The root is an ephemeral
comparison value, never a lease field or state selector. The server reloads the
registry and manifest to verify the identity tuple and normalized root, derives
the authoritative socket path from machine state and workspace UUID, and
requires both the singleton PID and job listener to be live. Only that derived
socket enters the lease, and its liveness probe shares the control request's
absolute deadline. Enablement comes from the authoritative registry. The
read-only snapshot exposes only the generation and live-lease count. The
generation-bound workspace-ingress query exposes only an optional ingress for
the exact requested live workspace UUID. It prunes expiry first and never falls
back to a known historical ingress or another workspace's lease.

The public route identity is a typed portable `IngressId`, never a canonical
name, root, default selection, or query parameter. Every accepted path has the
provider shape `/w/<ingress>/{sms,email}` or local capability shape
`/local/<lease>/w/<ingress>/{habits,habits/done,session/done}`. A local route
accepts the live lease's own ID, plus the single capability that lease
inherited when it superseded a browser-only background lease for the same
workspace; the inherited capability is dropped with the lease that holds it.
Shared-process routing first consults
`LeaseTable::availability`. Only `Accepting` yields a live lease and a
generation-bound `WorkspaceRouteTicket`. Registry, root, and manifest IO then
occurs without the control-state mutex. The route revalidates that the same
generation and exact authority revision are still accepting before
constructing the immutable `WorkspaceContext`. A heartbeat renews expiry without changing
the revision. Registration and receiver enablement refreshes advance the
workspace's remembered revision. Removal or expiry leaves no accepting
authority, and any later registration advances that remembered revision, so
even a later lease that reuses the same ID, workspace, ingress, TUI PID, and
job socket cannot match a ticket from before revocation. An unregister,
disable, replacement, or expiry makes the ticket stale. Unknown
ingress maps to 404; a known ingress with receiver disabled or no live TUI maps
to 503. The returned context and lease remain paired with the original ticket
for later forwarding without reopening another selector. Receiver dispatch
reloads the exact canonical registry record after actor/job construction,
requires the same workspace UUID and persisted `receiver_enabled = true`, and
then revalidates that ticket immediately before the socket handoff. A disable,
unregister, expiry, or replacement during provider work therefore cannot
enqueue, including when a persisted disable's live-refresh notification is
lost. It also derives one absolute handoff deadline,
capped at two seconds and before the separately reserved response window, and
carries it through nonblocking connect, frame write, and acknowledgment read.
A registration replay or enablement refresh
computes its next revision before changing expiry, enablement, or registration
state; revision overflow rejects the complete transition without extending or
reviving authority.

For Resend only, a known unavailable ingress can yield its remembered workspace
UUID without yielding a live route ticket. That UUID selects exactly one
registry record for signature verification and bounded in-memory provider-ID
deduplication. It never constructs `WorkspaceContext`, loads portable users, or
opens the job socket. A verified unavailable ID is a permanent discard in the
same 1024-key workspace/channel cache, not a queued job or durable replay item.
The accepted registration is also the source for local habits and triage URL
generation, so a later portable-manifest change cannot redirect a live TUI or
selected short-lived command through a peer workspace's ingress.

The machine-wide lifecycle record is deliberately smaller than a lease. Brain
publishes `~/.cache/brain/server/process.json` with only the process PID,
loopback HTTP port, generation UUID, and RFC3339 start time. Sibling
`control.sock`, `election.lock`, and `server.log` artifacts are infrastructure,
not workspace state. The record never contains a workspace UUID or root,
ingress ID, job socket, actor, sender, credential, prompt, log payload, or
message body. A generation UUID guards cleanup so a stale owner cannot remove a
new winner's record or socket. The elected process must receive its first
registration within two seconds or it exits and removes its generation
artifacts, covering a starter TUI that disappears before registration.

There is intentionally no durable inbound-work model. `InboundJob` crosses one
bounded Unix connection and exists afterward only in the exact target TUI's
64-entry memory queue. A successful acknowledgment means that append occurred;
an unavailable response means the message was discarded. No row, spool file,
replay cursor, or headless-agent record exists. Consequently, zero live TUIs
means zero server and no Brain response, while a live peer plus unavailable
target means one unavailable response and no retained work.

Status uses a separate `ReadOnlyWorkspace` bootstrap policy. It reads an
already-valid current-schema selected record, manifest, portable users, persistent
intent, and any existing generation snapshot without invoking recovery or
write-capable stores. This preserves the four-field receiver projection
(`Receiver`, `TUI`, `Server`, `Accepting`) without changing bytes or process
state. One generation-bound control response carries the process lease count
and exact-workspace lease state. The underlying lease-table view filters
expired entries without removing them or changing revisions and shutdown
state; watchdog expiry remains a separate mutation.

`WorkspacePaths` derives its full base from the ID:

```text
~/.cache/brain/workspaces/<workspace-uuid>/
├── state.db
├── tui.lock
├── jobs.sock              (live TUI only, mode 0600)
├── users.transaction.lock
├── tasks.transaction.lock
├── inbox/
├── responses/
├── logs/                  (reserved, currently unused)
├── capabilities/
├── migrations/
│   └── multi-workspace-v1.json
├── migration-backups/
└── sync/
    ├── sync.lock
    ├── journal.db
    ├── current.json
    ├── current.log
    ├── bisync/
    └── baselines/
```

Its state database, transaction/TUI locks, live job socket, inbox, responses,
capability material, migration journal/backups, reserved log path, and sync
working data are all children of that base. `cache_dir()` borrows the stored
base; each child accessor derives an owned path. Distinct IDs therefore cannot
share runtime paths. `WorkspacePaths` remains the sole UUID-derived authority;
sync's focused workdir and CSV-baseline helpers only append below its `sync/`
children. Active run logs remain under `/tmp` through `logging.rs`,
are created exclusively with mode `0600`, and receive only centrally redacted
argv values.
`WorkspacePaths::logs_dir` is reserved and unused; it does not describe the
current diagnostic-log destination.

### Machine registry schema v3 (`workspace/registry/`)

The sole machine-global workspace registry is
`$XDG_CONFIG_HOME/brain/env.json`, or `~/.config/brain/env.json` when XDG config
is unset. Deterministic ordered names and aliases make its JSON stable:

```json
{
  "schema_version": 3,
  "default_workspace": "brain",
  "env": { "markdown_to_pdf_path": "/Users/example/.local/bin/markdown-to-pdf" },
  "workspaces": {
    "brain": {
      "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
      "root": "/Users/example/brain",
      "aliases": ["personal"],
      "local_user_id": "local-user",
      "receiver_enabled": false,
      "env": {}
    }
  }
}
```

The top-level `env` map is **machine-global**: values that describe this machine
rather than any one workspace, so every registered workspace resolves the same
answer. It is optional and omitted when empty. `crate::env` routes a declared
variable to this map or to the selected record by
`env::schema::MACHINE_GLOBAL_VARS`, which is the single source of truth for the
scope; `brain env get/set` uses the same bare name either way.

**Schema 1 → 2 → 3.** v2 introduced the record map; v3 added the machine-global
`env`. A v2 file fails the exact-version check, which is what routes it into
`workspace::registry::upgrade` on the next ordinary command: a pure JSON rewrite
that moves every `MACHINE_GLOBAL_VARS` key out of the records (first canonical
workspace name wins, blanks skipped) and stamps the new version, wrapped by the
transaction, an exact-bytes backup, and a strict re-validation before saving.
`RegistryStore::load_readable` applies the same pure upgrade **in memory** for
read-only probes, which must not write.

`WorkspaceId` is encoded as a UUID string. Canonical keys, aliases, and the
default deserialize through `WorkspaceName` validation rather than bypassing
the newtype. Missing `aliases`, `receiver_enabled`, and `env` fields default to
an empty set, `false`, and an empty object respectively. Canonical-equivalent
duplicate workspace keys or aliases are rejected during deserialization rather
than silently collapsed by the ordered map or set.

The trusted deserialization boundary is a private raw schema DTO followed by a
fallible conversion that runs all whole-registry validation. Both public
`Deserialize<MachineRegistry>` and `RegistryStore::load_from` cross that same
boundary. The store parses the raw DTO directly only to preserve the distinction
between structural JSON errors (operation, path, and parser message) and typed
domain validation errors.

`receiver_enabled` is persistent intent, not evidence of a server or live TUI.
All mutation surfaces use `ReceiverAction` plus the pure
`receiver_transition(current, action)` decision. Persistence reloads under the
registry transaction and requires both the selected canonical key and the UUID
captured at bootstrap. A replaced record therefore fails without changing the
new record or any peer. Runtime availability is the conjunction of persistent
intent and an unexpired exact-workspace lease in the current shared-process
generation. Authoritative route loading rechecks the exact record's persistent
intent, so a disable takes effect even before the live lease refresh arrives.

Every record is a silo. Canonical, alias, and omitted-selector/default lookup
returns a borrowed `(canonical_name, record)` view of exactly one record; no
environment fields are copied or merged across workspaces. Access policy is
portable workspace data and is never stored in this machine registry.
Validation is whole-registry and requires schema `2`, at least one record, a
canonical default, selector uniqueness under ASCII case folding (including
alias versus canonical collisions), unique UUIDs, and absolute roots that,
after lexical normalization, are neither equal nor ancestors/descendants of
one another. `add_alias` also treats reinserting a case-folded alias already on
the same record as a typed error; it never reports a no-op as success.

Rename rekeys only the `BTreeMap` canonical name and updates the default when
needed, preserving the UUID and every `WorkspaceRecord` field. Changing the
default workspace never changes access mode and changes no record field.
Removal detaches a record only and never touches its
root or contents. At the storage boundary all writers acquire the stable
adjacent `.env.json.transaction.lock` database with `BEGIN IMMEDIATE` before
loading. Under that lock they clone, mutate, whole-registry validate, perform a
same-directory atomic save, then replace the live value. Validation or write
failure preserves the original in-memory value and file bytes. The bounded
acquisition error is typed with the lock path, wait duration, and owner PID
when readable from the stable `.owner` sidecar. SQLite releases a crashed
process's lock, and guards never unlink the stable lock database or sidecar.

Methods on `MachineRegistry` are in-memory transactions: they clone, validate,
and replace the live value but do not persist. `RegistryStore::update` reloads
inside the interprocess transaction, advances the file first, and replaces the
caller's live value only after the atomic save succeeds. IO failures retain the
failed operation, relevant destination or temporary paths, error kind, and
message. Startup migration and selected-record env writes use this same
transaction owner.

### Workspace CLI decisions (`workspace/command/`)

Clap retains `--workspace/-w` as an unresolved `Option<String>`. At the registry
boundary, `MachineRegistry::select` case-folds it and resolves a canonical name
or alias. Before Clap delegates a trailing task argument list, one shared
real/test normalization extracts `--workspace value`, `-w value`, or
`--workspace=value` from any pre-`--` position and keeps the exact raw value.
Selector-looking tokens after `--` remain delegated values. Bootstrap applies
this selection once for every ordinary command and returns one immutable
`CommandContext`. Every ordinary store and runtime path receives that context
or an explicit path derived from it; no handler reselects the default.

Detached Brain children carry the canonical `--workspace` selector, never the
alias the caller happened to use. Brain-owned integrations receive exactly the
common identity boundary `BRAIN_WORKSPACE_ID`, `BRAIN_WORKSPACE`, `BRAIN_ROOT`,
`BRAIN_ACTOR_ID`, and `BRAIN_CHANNEL`; agent-session variables are layered on
separately.

Collected management values first become a pure `Mutation` enum. `Create` and
`Attach` carry a validated canonical name plus an absolute, tilde-expanded,
lexically normalized root. Rename and alias decisions carry validated new
names; default/removal carry only selectors. In particular, `Remove` has no
filesystem path or deletion operation. The shell then loads the current schema directly
and persists via `RegistryStore`; it never flattens through legacy env helpers.

Fresh create records receive a new UUID, empty local-user placeholder,
disabled receiver, no aliases, and an empty machine env. `create` may create
only its requested root after candidate validation. It records the exact
missing directory chain. If later provisioning or persistence fails, every
path created by that invocation is preserved. Brain performs no automatic
directory deletion because safe Rust 1.85 path APIs cannot atomically verify
ownership and delete the same object. The structured error retains the
original failure as its source and lists only those invocation-created paths,
deepest first, for manual inspection and cleanup. `AlreadyExists` during
creation is treated as a race, left untouched, and omitted from the cleanup
list. `attach` requires an existing directory with a strict portable manifest
and adopts its UUID. Rename, alias, and default
changes preserve the record's UUID and all unrelated data. Removal detaches a
non-default record only.

### Portable workspace manifest and command readiness

`<workspace-root>/.config/workspace.json` is portable and strict:

```json
{
  "schema_version": 1,
  "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
  "receiver_ingress_id": "e806258e-491a-436d-9db4-a5ca9903e0d4",
  "minimum_brain_version": "0.18.3"
}
```

Schema `1` is the only accepted portable-manifest schema; older and newer
numeric schema values are both unsupported. Brain and minimum versions use a
numeric `major.minor.patch` core. Missing, extra, or nonnumeric components are
errors. A client older than the minimum fails with the exact update guidance
`workspace requires Brain <minimum> or newer; this is Brain <current>`.
Unknown fields and malformed UUIDs are also errors. The manifest UUID must
equal the selected registry record; the stable receiver ingress UUID remains
portable across machines. Create
writes the manifest before registry persistence. Attach reads it without
editing it. Legacy flat-env migration creates the root and first matching
manifest before replacing the flat registry.

The ingress UUID is generated only by `WorkspaceManifest::new` for a newly
initialized workspace. Receiver setup loads and validates it without writing
the manifest. Attach adopts it, while canonical rename, alias changes, and
machine-default changes operate only on the machine registry and cannot rotate
portable ingress identity.

`workspace::bootstrap` maps every parsed route to `None`, `InternalNoPrompt`,
`RegistryOnly`, `ReadOnlyWorkspace`, or `ReadyWorkspace`. The last two classes
select a record. `ReadOnlyWorkspace` requires already-valid registry,
manifest, and user bytes and opens no recovery or write seam. `ReadyWorkspace`
may run ordinary readiness and repair. Readiness is manifest validity/UUID
agreement plus portable
membership when `.config/users.json` exists. In that schema, the machine-local
`local_user_id` must parse as a user ID and name one member. Missing values become a pure
`ReadinessAction::Prompt(fields)` interactively or a typed error carrying exact
repair commands headlessly. Successful interactive repair happens under the
registry transaction, then bootstrap reloads and constructs one
`CommandContext` containing `Arc<WorkspaceContext>` and `RegistryStore`.

One missing value never prompts or fails: when the manifest is present, exactly
one portable person exists, and `local_user_id` was never set, readiness returns
`ReadinessAction::AdoptLocalUser(id)`. A single-user workspace can only mean that
one person, so bootstrap adopts them as this machine's local actor under the
registry transaction (a one-line themed note on stderr) and continues the
requested command in **every** mode, interactive or headless. This self-heals a
workspace that reached the users-present/`local_user_id`-empty state, so an
ordinary command such as `brain skills sync` never stops to send the user off to
run `brain user local`. Adoption is deliberately narrow: a nonblank but unknown
`local_user_id`, or two or more people with no local selection, is a genuine
choice that still prompts (interactive) or errors with the exact repair command
(headless).

The first create deliberately leaves `local_user_id` empty. Its next ordinary
interactive command creates the first portable person, selects it locally, and
continues the requested command. Headless setup uses `brain user add` followed
by `brain user local` (or, once one person exists, the sole-user adoption above).
An existing workspace with no `users.json` and a non-empty legacy local ID
remains ready because ordinary startup never activates migration. The explicit
workspace migration command performs the reviewed conversion.

### Requirement health is not readiness

`workspace::Requirement` is a read-only projection over one pinned
`CommandContext`:

| Field | Meaning |
| --- | --- |
| `scope` | One required component or optional feature, including dynamic MCP and skill names. |
| `status` | `Required(Ready|Unavailable)` or `Feature(Off|Ready|Incomplete)`; required and optional states cannot be confused. |
| `prompts` | Labels plus a secret/non-secret bit for an interactive setup surface; no stored value is retained. |
| `remediation` | Exact noninteractive CLI syntax for an unavailable or incomplete row. |

Required rows cover the selected root, compatible matching manifest, nonempty
schema-1 portable user registry, and a local user ID that names a portable
person. Optional rows cover sync/watcher, receiver/SMS/email,
access/MCPs/skills, triage habits/modal, PDF, Linear, the local person's
persona, other members' personas, and
browser/web views. A disabled feature has no setup error. A present malformed
sync block or provider field fails closed as incomplete. Receiver channel
activation comes from receiver intent plus current provider-field presence or
portable inbound mappings, not a second channel toggle.

The inspector reloads the exact canonical record and rechecks its UUID against
the pinned context. It never falls back to another record. Sync requirement
health describes complete local configuration; every actual setup, sync,
repair, or check still probes the remote portable manifest and refuses a UUID
mismatch before data movement.

### Portable people (`.config/users.json`)

Schema `1` is strict and rejects unknown fields:

```json
{
  "schema_version": 1,
  "users": [
    {
      "id": "alex-smith",
      "name": "Alex Smith",
      "phones": [{ "value": "+12125550123", "inbound_allowed": true }],
      "emails": [{ "value": "alex@example.com", "inbound_allowed": true }],
      "response_email": "alex@example.com"
    }
  ]
}
```

User IDs are exact lower-case kebab identifiers. Names are non-empty display
labels. Phone values are normalized to unambiguous E.164, and email values are
trimmed and ASCII-lowercased without provider-specific rewriting. A person
cannot repeat one contact, and one enabled inbound contact cannot belong to
multiple people. `response_email`, when present, must also appear in that
person's email list; it need not be enabled for inbound resolution.

Legacy conversion associates a workspace-level response email with the first
portable person only when it matches a normalized email receiver allowlist
entry. An unmatched response address and every other allowlisted address stay
in the unresolved proposal for explicit assignment. A response setting by
itself does not configure an inbound email identity or trigger an email prompt.

Receiver setup performs explicit assignment against this model. A selected
phone or email is normalized, inserted or updated on one exact user, and
carries its own `inbound_allowed` boolean. A new ID also requires a display
name. Channel selection controls required fields: SMS has no email requirement,
and email has no phone requirement.

Removing a person can change `tasks.csv`, `habits.csv`, and `users.json` as one
recoverable group. Same-directory staged files and backups preserve each live
file's mode. The transient `.config/.brain-user-transaction.json` journal is
portable so another machine can recognize an interrupted publication; relative
paths in it are validated before recovery. The SQLite serialization lock is
machine-local at
`~/.cache/brain/workspaces/<workspace-uuid>/users.transaction.lock`. Before a
portable-user load, Brain rolls any journaled group back to its complete old
generation. Removing the journal commits the new generation.

`local_user_id` stays in the machine registry because different machines may
be used by different people in the same portable workspace. Two machines may
also select the same portable ID for the same person; Brain does not create a
machine-specific version of that person. The field denotes the person acting
locally, not a device identity, workspace owner, creator, authentication claim,
or audit principal.

### Legacy flat-env migration

`workspace::registry::migrate` turns pre-v2 `env.json` into one default record.
It preserves every machine-local flat value except the structural `root`, the
receiver switch moved into `receiver_enabled`, and access-policy keys that do
not belong in machine data. Nested JSON values remain unchanged. The root uses
the legacy precedence (flat value, read-only pointer, `<home>/brain`), expands
leading `~`, and is made absolute and lexically normalized without filesystem
canonicalization. A valid normalized basename supplies the canonical name;
invalid basenames use `brain`.

When the migrated root already has a valid portable manifest, the record adopts
that manifest's immutable workspace UUID and preserves its receiver-ingress UUID
and exact bytes. Only an absent manifest causes migration to generate those
identities and create a matching manifest. Migrated records have empty aliases
and an empty `local_user_id`; the outcome marks local identity setup required,
and migration does not invent that user identity.
Existing flat bytes are copied exactly
to the first free adjacent `env.json.legacy-backup[.N]` before atomic registry
replacement. A valid v2 registry input is never rewritten or backed up, making
reruns UUID-stable and byte-stable. Before that path succeeds, Brain strictly
validates each registered root's portable access mode and atomically seeds
missing modes: the current default receives `unrestricted`, and nondefault
roots receive `workspace_only`. Valid existing values are never rewritten.

On a machine with no registry, a first explicit create/attach establishes the
requested workspace directly. A fresh ordinary or repair invocation instead
synthesizes the compatible default `brain` workspace and then crosses the
normal readiness boundary.

### Current identity and schema boundary

The current release resolves one immutable `ActorContext` at ordinary command
bootstrap, before task, reindex, TUI, or local-agent work. Local/TUI work
resolves `local_user_id`; authenticated
SMS/email work resolves an enabled portable identity and takes precedence over
that machine default. A queued receiver job contains the workspace UUID and
the resolved actor, never an untrusted sender string as `BRAIN_ACTOR_ID`.
`InboundJob` is a bounded JSON frame containing a fresh job UUID, workspace
UUID, `ActorContext`, channel, normalized authenticated sender, prompt,
attachment references, receipt time, provider delivery ID, authenticated
thread participants, the actor's acceptance-time normalized response email,
and the acceptance-time allowed response recipients. It exists only in the
matching live TUI's in-memory queue. A socket acknowledgment means that append
succeeded; failed acknowledgment writes roll it back. Follow-ups retain the
initiating actor and channel even if machine registry or portable user data
changes during the turn. A ready legacy workspace whose portable
user store is absent uses its exact lower-case kebab local ID as an immutable
compatibility actor and does not create `users.json`. Malformed nonblank legacy
IDs are readiness errors with an explicit machine-local repair command. This
does not add authentication,
ownership, creator metadata, audit history, or device identity. The release
now provides canonical `assigned_to` task and habit fields. The value is a
portable `UserId`; creation defaults to the immutable effective actor,
unrelated edits preserve it, and explicit changes validate membership. Readers
accept legacy `assignee`, prefer `assigned_to` when both exist, and writers
migrate to `assigned_to` by column name. Task rows accept an optional
`task_uuid` during the compatibility window; all new rows receive UUIDv4, and
mutations preserve any existing UUID. The coordinator-only schema helper derives
legacy UUIDv5 values from
`<workspace-uuid>:<csv-kind>:<legacy-task-id>`, backs up both CSVs, both
counters, and `SCHEMA.json`, then writes task schema version 2 with
`task_uuid` as immutable merge identity and `task_id` as mutable display
identity. The caller supplies an existing durable backup base; a backup
destination is accepted only when its canonicalized path is beneath that base
and disjoint from the workspace tree. Before copying, every pre-existing
inventory parent and destination is inspected without following symlinks;
nested symlinks and non-directory components are rejected. Each missing descendant is created
separately, and every actual parent is synced before continuing, including on
retry through a partially created chain. Exact backup bytes are file-synced
and their actual parent directory is synced before any portable replacement.
The helper publishes a durable prepared/committed transaction journal before
sequential atomic replacements, so retry can roll back an interrupted prepared
generation or finish cleanup for a committed one; failed journal publication
removes its temporary file immediately. Current detection validates the merge
key, mutable display identity, canonical assignment, `system_key`, and UUIDs,
not the schema version alone. It is called only by explicit
`brain workspace migrate`. The rollout coordinator owns the last legacy
semantic sync, acknowledgement and identity gates, portable backup, step
journal, activation, schema-last remote publication, and final verification.
The local migrated header uses a complete canonical order for every known task
column, then appends forward-compatible columns in lexical order. Migration
sets `forward_compatible_columns: true`, so independently migrated legacy
copies and later semantic merges converge byte-for-byte without discarding
newer fields. Existing legacy files retain
`task_id` as their merge key until migration. Schema-v2 files merge by
`task_uuid` and reconcile mutable display IDs without activating that
migration.

Pre-existing duplicate UUIDs are normalized before current publication: the
first row in deterministic tasks-then-habits order keeps its UUID, while later
duplicates receive deterministic replacements derived from workspace, CSV kind,
original UUID, display ID, and row position. Resumed verification repeats this
idempotent repair and republishes current CSVs and baselines before checking the
remote copy.

### Coordinated rollout journal and retained backup

Explicit `brain workspace migrate` owns one machine-local rollout generation
per workspace UUID. Its active journal is
`<workspace-cache>/migrations/multi-workspace-v1.json`; its retained backup is
`<workspace-cache>/migration-backups/<UTC>-pre-multi-workspace/`. Both paths
come only from `WorkspacePaths`, so two workspaces can migrate independently
without sharing recovery state. Neither path is portable or synced.

Before discovery, planning, or journal creation, the coordinator acquires the
workspace UUID sync lock and retains it through the complete rollout. It then verifies the selected manifest and
workspace UUID, remote identity when sync is configured, explicit all-machine
acknowledgement for a synced headless rollout, and a disjoint machine-local
backup destination. Unconfigured migration also finishes portable user and
assignment mapping before journal creation. Configured migration first records
the final legacy sync in the journal. If the remote marker is absent or is the
recognized pre-v2 task schema, this is the ordinary legacy semantic merge. If
a present remote marker strictly
declares supported schema v2, a migration-owned join merges the legacy
baseline/local generation with current remote rows by `task_id`, preserves the
remote UUID for every matching row, and performs no remote task publication.
The same replayable bridge reconciles each local id counter to
`max(local, remote, joined_max + 1)` before the journal can record the legacy
semantic step complete. Missing or malformed counter text is absent input;
joined rows still provide the safe floor. Neither CSV nor counter state is
published by this bridge.
The coordinator then reloads config, portable users, and
both CSV assignment columns and reruns mapping preflight before backup or
portable mutation. If both `assigned_to` and legacy `assignee` exist,
`assigned_to` is canonical. One mapping answer either adds a portable person or
adopts an existing one. Adopting an existing person for an assignment value
records that value in an assignment rewrite set instead of creating a second
person; the recheck that must find no remaining issue runs against the rewritten
values, and the journaled task cutover applies the same set to `assigned_to` in
both CSVs while the retained backup keeps the original values. An adopted phone
or email records no rewrite because those identities move into the member
record itself. The journal binds the migration ID, workspace UUID,
canonical root, original plan, original timestamp, retained backup, and
completed steps. Reentry must match that identity exactly and resumes the same
generation; a mismatch fails closed.

The ordered steps are final legacy semantic sync, portable backup, user
migration, local task-schema migration, remote task-schema transition,
managed-triage reconciliation, reindex, and final verification. The final
legacy sync completes before UUID task identity can become authoritative. The
local migration and remote transition share the UUID sync lock. The transition
publishes current `tasks.csv` and `habits.csv`, durably writes both exact
machine-local baselines, and publishes `tasks/SCHEMA.json` last. Those three
paths are excluded from ordinary rclone transfer. Each completed step is
atomically journaled, and the task-schema subtransaction has its own
prepared/committed recovery boundary for multi-file replacement. An active
rollout journal makes ordinary sync and sync setup refuse after taking the UUID
lock, so crash recovery must resume the journaled transition. Success removes
only the active rollout journal and retains the backup. An interrupted run
prints the exact resume command. Recovery is resume-only for every active
journal state because remote task publication may have completed before its
step record became durable. Restoring only one machine could therefore diverge
from the authoritative remote generation. Rerunning a
fully current workspace is byte-idempotent and creates no new backup.

The backup hardening rejects all pre-existing nested symlink and non-directory
components. Publication writes through a verified temporary file outside the
backup tree, opens the destination parent with no-follow directory flags, and
renames through that descriptor. A parent replacement after validation cannot
redirect either the temporary write or the final publish.

Managed triage identity lives in the optional `system_key` column. The reserved
values `brain.triage.daily` and `brain.triage.weekly` identify Brain-owned
chains even if their display names change. When enabled, reconciliation keeps
exactly one pending occurrence for each key; recurrence creates a fresh UUID
and retains its key and assignment. When disabled, one journaled grouped
replacement removes every keyed task/habit row plus exact derived UUID/display
references. Every managed UUID is removed. A managed display ID is also
removed unless an unmanaged row with that display ID survives; duplicate
managed rows do not make their shared display ID ambiguous. Name-only matches
are never purged.

The managed-triage transaction journal is schema version 2. It records the
workspace UUID, normalized root, state (`preparing`, `prepared`, or
`committed`), generated transaction ID, and exact live/staged/backup set.
Recovery authenticates those fields before touching a file. Project purge
rewrites only the top-level `.METADATA.json:tasks[]` reference field;
malformed JSON, invalid UTF-8 indexes, and traversal errors abort the whole
transaction before publication. A display reference shared with an unmanaged
row is preserved because the surviving row remains its possible target.

## Portable access policy (`access/`, `.config/config.json`)

`AccessMode` accepts exactly `unrestricted` and `workspace_only`. It is
portable workspace data, not a field in `MachineRegistry`: the first migrated
or created root is seeded unrestricted, later created roots are seeded
workspace-only, and a default-workspace change never rewrites either file.
Attach and valid-v2 startup apply the same default/nondefault rule to missing
values before registry publication or readiness succeeds. Existing config is
strictly parsed, unrelated fields are preserved, and access writes use a
same-directory temporary, file sync, atomic replace, and parent-directory sync.
Malformed/non-object config and invalid stored modes are errors, never an
implicit unrestricted fallback.

At launch, `AccessPolicy` snapshots the trusted mode, selected
`WorkspaceContext`, and resolved `ActorContext`. User and inbound message text
remain only in `LaunchRequest::initial_prompt`; it cannot change the policy
snapshot. Unrestricted mode has no boundary prompt. Workspace-only mode builds
one advisory prompt naming the selected root and actor, then every interactive,
SMS, email, fresh, resumed, and triage request carries it to the selected
frontend. Claude/Codex use an option terminator and OpenCode uses one quoted
`--prompt` value, so a leading `-` remains prompt data. `workspace_only` is advisory prompt
enforcement plus best-effort capability filtering, easy to bypass, and not
tenant isolation. It reduces accidents and naive leakage among trusted users;
adversarial or sensitive isolation requires an external OS, VM, machine, or
container boundary.

`CapabilityPlan` is a separate immutable launch snapshot. Portable config owns
only ordered logical `allowed_mcps` and `allowed_skills` names. Resolution reads
only `agent_capabilities` from the already-selected machine registry record and
retains that record's workspace UUID as credential provenance. Missing MCP
connection data, incomplete credentials, and missing custom skill paths become
`Unavailable`. Logical names use a lower-case canonical form derived from an
ASCII letter/digit plus ASCII letters, digits, `.`, `_`, or `-`; whitespace,
controls, Unicode, other punctuation, and duplicates after case normalization
are configuration errors. MCP commands are exact non-whitespace executables
with control-free argument strings. HTTP transports require exact `http` or
`https` URLs with a host and control-free header data. Credential kinds must
match their transport, and protected frontend environment names are rejected.
Unrestricted plans delegate to frontend global configuration. Workspace-only
plans preserve an explicit empty list, while a missing skill list receives the
four core defaults. The controller requires one workspace-only plan and rejects
a mode mismatch or credential provenance UUID from another workspace before
frontend translation.

A machine skill `path` is absolute. Its root-owned first component is the
trusted source anchor. Brain canonicalizes that anchor and the complete path,
requires the result to stay below the anchor, rejects symlinks in every
component below it and in all source descendants, and stores the canonical
path in the capability plan. This accommodates operating-system aliases such
as `/var` without treating a configurable parent symlink as trusted.

Capability artifacts use the selected workspace's UUID cache directory as
their trusted filesystem root. Recursive cleanup validates that root and every
ancestor of its target with `symlink_metadata`; the final target may be an
unlinked symlink, but no ancestor symlink is followed. Missing targets are a
no-op and any unexpected entry type fails closed.

`classify_obvious_outside_path` is a pure defense-in-depth warning for literal
absolute and `~/` paths. It does not resolve symlinks or aliases and does not
attempt prompt-injection detection. Paraphrasing and indirect requests can
bypass it.

## Persistent state (`state.rs`, `<workspace-cache>/state.db`)

The persistent shell tracks frontend-scoped actor sessions and the layout
preference in SQLite (WAL). Receiver completion uses the same generic lifecycle
bridge for all registered frontends.
Two tables:

```sql
brain_sessions(
  agent_kind         TEXT NOT NULL,  -- claude | codex | opencode
  agent_session_id   TEXT NOT NULL,
  brain_instance_id  TEXT NOT NULL,  -- one per running `brain` shell (a lineage)
  locked_pid         INTEGER,        -- live brain holding it, or NULL when free
  source             TEXT,           -- last session-start source (startup/resume/clear/…)
  workspace_id       TEXT NOT NULL,
  actor_id           TEXT NOT NULL,
  channel            TEXT NOT NULL,  -- interactive | sms | email
  created_at         INTEGER NOT NULL,
  last_active_at     INTEGER NOT NULL,
  completion_status  TEXT NOT NULL,  -- active | completed
  PRIMARY KEY(agent_kind, agent_session_id, workspace_id, actor_id, channel)
)
meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)
  -- 'panel_side'            = 'left' | 'right'
  -- 'skills_synced_version' = brain version that last rendered this workspace's
  --                           skills (the startup auto-resync stamp)
```

The `meta` table is a generic key/value store, so a new key like
`skills_synced_version` needs no schema migration. It records the
`CARGO_PKG_VERSION` that last rendered this workspace's skills into the
registry; `skills::resync_on_version_change` re-renders (and re-stamps) when the
running binary differs, so a version bump ships its bundled-skill changes
without a manual `brain skills sync`.

**The lock + recency model.** A session is "free" when `locked_pid IS NULL`.

- `SessionStore::sessions_by_recency` selects free sessions only within the exact
  agent/workspace/actor/channel scope, newest (`last_active_at DESC`)
  first. Claude walks them and resumes the first whose **transcript
  exists** on disk (`ClaudeFrontend::resume_candidate_exists` +
  `agent::claude::project_dir_name`); a session opened but never chatted in has a
  DB row but no `<id>.jsonl`, and `claude --resume` can't find it, so it's
  skipped (and the user gets a status-line alert when that forces a fresh
  chat). OpenCode snapshots `session list --format json` in the selected root
  and accepts only a live root session whose reported directory resolves to
  that exact workspace; archived, deleted, child, malformed, and cross-root
  rows are rejected. A stale DB candidate therefore falls through to the next
  row or a fresh launch. Codex participates in the same store but currently
  rejects resume candidates and starts fresh.
- `SessionStore::claim` → lock a free session in the exact composite scope to this
  shell's PID (loses cleanly if another shell grabbed that scoped row first).
- `SessionStore::register` inserts a fresh placeholder for any registered frontend with
  complete immutable attribution and `active` status. The session-start bridge records the
  actual frontend session ID only when the exact tuple is registered or
  the ID rotates an already registered active shell lineage; every other event
  is rejected. The authorization reads and accepted rotation mutation
  share one `BEGIN IMMEDIATE` transaction, so concurrent target claims are
  serialized and a rejected or failed attempt preserves both lineages.
- `SessionStore::mark_completed` and the turn-complete bridge transition the exact scoped
  row to `completed`; an accepted session-start event or
  `SessionStore::mark_active` after a successful local or queued submit returns
  it to `active`.
- Legacy schema-v2 through schema-v4 rows migrate transactionally as Claude,
  interactive rows
  for the selected workspace and its machine-local user; existing locks,
  source, and timestamps are preserved. Schema v5 adds
  `completion_status`, defaulting every existing row to `active`.
- Receiver runtime state distinguishes an active remote job
  (`receiver_started` is set) from a warm channel panel (`receiver_session_id`
  plus a three-minute `receiver_lease`). A warm lease never counts as active
  LLM work. This lets bridge completion release queued work while keeping
  the completed SMS/email conversation visible and reusable.
- `SessionStore::release` → when the panel closes (the agent exits) or the shell quits, clear
  this instance's locks and stamp `last_active` (floats it to the top of the
  next resume — so re-opening with "Message brain" picks it back up, and a
  second terminal could too).
- `SessionStore::reap_dead_locks` → on startup, free exact scoped rows whose PID is no
  longer alive (`kill -0`), so a crashed shell doesn't strand its session.
  Equal opaque IDs in other frontend/workspace/actor/channel scopes remain
  independent.

The invariant: at most one live shell holds a given session (no tangled
threads), and exactly one session per instance is current (the session-start
bridge frees the instance's others on every start, handling `/new`). The
`PanelSide` enum (`Left` / `Right`, default `Right`) lives in `state.rs`
because it's the persisted layout value.

**Skill-session tabs are deliberately *absent* from this table.** Each ephemeral
skill session (`App.skill_sessions`) is launched by an `AgentController` from a
fresh `LaunchRequest`. Its hook metadata carries the session-done URL and token
but no `BRAIN_INSTANCE_ID`, `BRAIN_STATE_DB`, or `BRAIN_RESPONSE_ID`. The
session-start bridge no-ops without the tracking values, so no `brain_sessions`
row is ever written and it is never a resume candidate. A tab lives only in
process memory (`App.skill_sessions: Vec<SkillSessionTab>` — its `SessionTabId`,
`SkillSessionKey`, tab title, completion token, and controller — plus
`App.active_brain_tab: BrainTab`) and is torn down when its run completes or the
shell exits.

Both main and skill-session values are `AgentController` instances, not raw PTYs.
Their shared semantic API owns launch, input, session, completion, terminal,
and shutdown behavior; only frontend adapters translate those operations.
Whole-shell teardown explicitly shuts down every controller before releasing
the session-store lock.

## Skill sessions (`skill_session/`, `skill_sessions` env)

A workspace's own skill sessions live in its machine-local env record as
`skill_sessions`, a JSON array of objects:

```json
"skill_sessions": [
  { "title": "Email triage", "prompt": "/email-triage", "command_label": "Run email triage" }
]
```

`prompt` is the only required field — an entry without one is not a session and is
dropped. `title` defaults to the prompt; `command_label` defaults to
`Run <title>`. A dropped entry does **not** renumber its siblings: a session's
identity is `SkillSessionKey::Custom(<index in the raw array>)`, so fixing a
malformed neighbor can never silently repoint a palette row at a different
session. The builtin daily triage is `SkillSessionKey::DailyTriage`, is not stored
here, and is offered only while the workspace's daily-triage check is enabled.
Parsing (`skill_session::parse_configured`) and offering
(`available` / `runnable`) are pure.

## Skill-session completion signal (`skill_session/signal.rs`, `<workspace-cache>/skill-sessions/<token>.json`)

The cross-process signal that closes a skill-session tab. When a run finishes it
POSTs `{"token": "<one-time-token>", "require": ["<path>", …]}` to the brain
server's `POST /local/<exact-live-lease>/w/<selected-ingress>/session/done`; after
live-lease and manifest resolution, the handler writes to only that workspace's
UUID-scoped cache, one file per token:

```json
{ "token": "<one-time-token>", "require": ["/abs/path/one", "/abs/path/two"], "at": 1730000000 }
```

`token` is the value brain handed the session in `BRAIN_SESSION_TOKEN`; it must be
safe as a file name (alphanumerics, `-`, `_`; brain issues UUIDs) or the POST is a
400, since the value arrives in a request body. `at` is an epoch-seconds
diagnostic. `require` is the set of **output paths this run declared must exist
before its tab may close** — the fix for a premature signal closing a tab before
the run's outputs were written. **Core declares none**, so `require` is empty
unless the run was told otherwise (for daily triage, by an extension rendered in
at the `triage:daily-required-outputs` hook); `signal.rs` and the TUI stay
completely ignorant of *what* the paths are. The TUI polls each open tab's own
token each tick and closes that tab only when its signal arrives **and** every
path in `require` exists on disk (an empty list closes immediately, so a fork with
no extensions behaves as before). One file per token is what lets several sessions
run concurrently without one's completion closing another's tab; a stale signal
from an earlier run cannot close a fresh tab, and the shell clears every pending
signal at startup. `parse_signal` and `ready_to_close` are pure; the file IO
(`record_done` / `read_signal` / `clear` / `clear_all`) is a thin shell around
them.

## Personalization (`personalization/`, `<brain-root>/.config/personalization.json`)

Content *about the people using a workspace*, stored beside `config.json` in the
brain config dir (`settings::config_dir()`) — just another brain config, inside
the brain root so it travels with the brain. A missing/broken file parses to no
personas — the app never requires personalization.

`Personas` (`personalization/personas.rs`) is the whole store: a
`schema_version` (`2`) plus `personas`, a `BTreeMap<String, Persona>` keyed by
the same portable user IDs as `users.json`. Ordering is by ID, so the file has
one canonical serialization no matter which machine wrote it.

`Persona` (`personalization/persona.rs`) is one member's entry:

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `name` | `String` | `""` | Optional display name. |
| `role` | `String` | `""` | Free-text role the assistant serves. |
| `works_for` | `String` | `""` | Org, `myself`, or empty. |
| `namespaces` | `Vec<String>` | `[]` | Project `<namespace>__<outcome>` life-buckets. Empty falls back to the generic defaults (`work`, `personal`); edited via the onboarding / `brain config set namespaces` checklist (`personalization/namespaces.rs`). |
| `tag_styles` | `Map<String, TagStyle>` | `{}` | Per-tag display overrides. The tag *set* (its keys) is chosen via the same checklist (`brain config set tags`). |

**Schema 1 → 2 migrates on read.** Version 1 was one unowned `Persona` at the
top level. `Personas::parse` takes the reading machine's `local_user_id` and
keys a legacy object onto it, because that is the only person who can truthfully
claim an unowned record; the next write persists the keyed schema. A legacy
object with nothing set migrates to *no* personas, so nobody inherits a blank
record they would then be nudged to fill in. A file that already has a
`personas` key is never reinterpreted as a legacy persona.

Reads are per person: `store::load_persona(workspace, id)` and
`store::local_persona(workspace)`; a user with no entry reads as an empty
`Persona` rather than an error. `store::save_persona` is read-modify-write over
the whole map, so writing one member never disturbs another's entry.
`Personas::missing(roster)` reports which of the workspace's members still have
nothing filled in (an entry that exists but is empty counts as missing) and
drives the `other members' personas` optional-feature row in workspace status.

`TagStyle` (`personalization/tags.rs`) is `{ emoji: String, label: String }`,
rendered as `"{emoji} {label}"`. Resolution (`TagStyles`) layers the user's
overrides over the generic defaults (`mit` → `❗ MIT`, `personal` → `✌ personal`,
`work` → `💼 work`); an unknown tag falls back to its raw name. The TUI
loads the selected workspace's styles explicitly and retains them in its
`App`; there is no process-global personalization cache that another workspace
can inherit.

The `brain persona show` block is the **skill-lookup contract**: a stable,
keyed `user:`/`name:`/`role:`/`works_for:`/`namespaces:` text block that
identity-dependent skills read at runtime to learn who they serve. The
`namespaces:` line always shows the *effective* set (the configured list, or the
generic defaults when unset), so a skill like `second-brain` always sees a usable
namespace list. `brain persona list` emits one such block per member,
blank-line separated, marking the local person `(this machine)`; a member of
`users.json` with no entry still gets a block of `(unset)` values, and a stored
persona whose user has left the roster is still listed, so nothing a skill
depends on silently disappears. Both are built by pure functions
(`command::persona_block`, `command::roster_block`).

The **interactive checklist** (`personalization/checklist/`) is the shared UI for
choosing a set (namespaces, tags): a pure state machine (`Checklist` +
`handle_key`, unit-tested) rendered by a thin `/dev/tty` ratatui shell (`run`).
All rows start checked; space toggles, `a` opens a tolerant "create new" line
(commas/semicolons/whitespace, per-item normalize + dedupe), Enter confirms, Esc
cancels. Onboarding runs it for namespaces then tags; `brain config set
namespaces|tags` re-runs it seeded with the current set.

## Brain env (`env/`, the selected record in `~/.config/brain/env.json`)

Machine-local config is siloed under each workspace record, deliberately
**outside** every brain root so it never rides whatever syncs the brain
directory. Workspace management resolves explicit selection directly against
the registry. Env callers receive the selected `CommandContext`; writes reload
under the registry transaction and require both canonical name and immutable
UUID before replacing that record's free-form `env` object.
See [config.md](config.md) for migration and storage details.

`env::schema::VARS` (`src/env/schema.rs`):

| Variable | Type | Default | Meaning |
| --- | --- | --- | --- |
| `markdown_to_pdf_path` | `String` | *(unset)* | Path to the `markdown-to-pdf` command on this machine. Auto-discovered and self-healed by the startup gate (`settings::markdown_pdf`). |
| `claude_cmd` | `String` | `claude --dangerously-skip-permissions` | Command used to launch the Claude brain-panel frontend on this machine. Resolved by `agent::configured_command`; blank falls back to the default, and a legacy portable config value is honored only when env is unset. |
| `codex_cmd` | `String` | `codex` | Command used to launch the Codex brain-panel frontend on this machine. Resolved by `agent::configured_command`; blank falls back to `codex`. |
| `opencode_cmd` | `String` | `opencode` | Machine-local command used to launch OpenCode. Blank falls back to `opencode`; Brain appends `--agent brain`, optional validated `--session`, and optional `--prompt`, after isolated compatibility probes. |
| `agent_capabilities` | `Object` | *(unset)* | Selected-workspace machine material. `mcps[]` contains a logical `name`, exactly one `command` plus optional `args` or `url`, and optional `credentials` (`environment`, `headers`, `bearer_token`). `skills[]` contains a logical `name` and machine-local directory `path`. Credential descendants render as `(set)` in env listings. |

For OpenCode, the effective inline config is an inherited JSON object plus a
reserved Brain layer. Brain owns `agent.brain`, `default_agent`, generated
`mcp.brain_ws_*` keys, and the selected actor's rendered skill-path entry.
Every unrelated key survives. The generated MCP values contain environment
variable names, never credential values, and OpenCode capability enforcement
remains advisory because inherited global sources cannot be proven absent.

All declared env variables and recursively flattened nested values render
through the same `Resolved { name, value, description }` type `brain config`
uses (re-exported from `settings::schema::Resolved`). Nested paths use dot
notation, for example `sync.remote.key_id`; array elements use numeric path
segments.

`brain env` groups those rows into a `Breakdown` (`src/env/breakdown.rs`):
a registry path, the machine-global rows (every top-level `env.json` key except
`workspaces`, flattened the same way), one `WorkspaceEnv { name, is_default,
is_selected, rows }` per registered workspace, and a `VarDoc { name,
description }` legend. Per-workspace rows come from `vars::resolve_all_at(root,
env)` — **root-based, not selected-context-based** — so a block resolves each
value, each default, and each legacy `config.json` fallback against its own
workspace and can never borrow a peer's. `assemble` is pure over
`(raw_json, registry, selected_uuid)`; `collect` is the thin registry-reading
shell, and an unreadable registry yields an empty view instead of an error.
`src/env/render.rs` turns a `Breakdown` into text (pure given a `Theme`),
padding one shared name column across all sections.

The workspace root is not an env variable. It is a validated structural field
on `WorkspaceRecord`; free-form env writes reject `root` and other structural
names. Legacy flat `root` and the old pointer are consumed only while building
the first registry record.

The `sync` field is not in `VARS`, but its nested values are still listable and
addressable with dotted `brain env get` and `brain env set` paths. The sync
setup flow remains the preferred way to create or validate the complete block.

## Sync config (`sync/`, the `sync` block in `env.json`)

`sync::SyncConfig` (`src/sync/config.rs`) is a typed view of the `sync` object
nested under the selected workspace record's `env`. As of C2, `brain sync`
reads it to drive `rclone bisync` reconciliation and one-way `rclone copy`
uploads (see
[integrations.md](integrations.md) and [architecture.md](architecture.md)); as
of C4 a configured sync always starts with a pull-biased background sync, and
the `watch` flag controls a debounced filesystem watcher while the shell is
open. `debounce_ms` sets the watcher's quiescence window. An absent
`sync` block parses to all defaults, so
sync reads as fully disabled and brain behaves exactly as if the key didn't
exist (`brain sync` prints "sync is not configured — run `brain sync setup`" and
does nothing, with no watcher thread or startup sync).

**Machine-local runtime state** (never synced) lives beside the journal under
`<workspace-cache>/sync/`:

- `current.json` — the [`current::CurrentState`] record of the sync in progress
  right now: `{ pid: u32, direction: String, started_at: String }`. Written
  when a run starts, removed when it ends (or when its `Reporter` drops). Its
  presence, validated against `pid`'s liveness, is how `brain sync status` and a
  following `brain sync` know a sync is underway; a hard-killed run's leftover
  record reads as not-running.
- `current.log` — the in-progress run's progress lines, appended live so a
  following `brain sync` can tail them and `brain sync status` stays honest.
  Truncated at the start of each run.
- `bisync/`: the brain-owned rclone bisync workdir (`--workdir`), with `.lst`
  baseline listings, and any `.lck` lock file (reaped before each run while
  brain holds its own sync lock, since it can only be from a dead run).
- `baselines/`: the selected workspace's semantic task and habit CSV
  snapshots. They never overlap another UUID's baselines and never enter the
  portable workspace.

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enabled` | `bool` | `false` | Master on/off switch for Backblaze B2 sync. |
| `b2_bucket` | `String` | `""` | B2 bucket name. |
| `b2_path` | `String` | `""` | Optional path prefix within the bucket. |
| `b2_key_id` | `String` | `""` | B2 application key id. |
| `b2_app_key` | `String` | `""` | B2 application key (secret; stays machine-local in `env.json`, never synced). |
| `crypt_password` | `String` | `""` | Optional rclone-obscured crypt password. Empty disables the `rclone crypt` layer. |
| `crypt_password2` | `String` | `""` | Optional rclone-obscured crypt salt. Used only when `crypt_password` is non-empty. |
| `crypt_filename_encryption` | `String` | `""` | Optional rclone crypt filename mode override (empty uses rclone's default, `standard`). |
| `crypt_directory_name_encryption` | `bool` | `true` | Whether rclone crypt encrypts directory names. |
| `watch` | `bool` | `true` | Run the debounced filesystem watcher while the shell is open. Its automatic push is a one-way, non-deleting upload. See `watch_effective` below. |
| `debounce_ms` | `u64` | `3000` | The watcher's quiescence window in milliseconds: a sync fires once changes under the brain root settle for this long. `debounce()` maps it to a `Duration`. |
| `max_delete_percent` | `u8` | `50` | Bisync safety guard: the max percent of files a sync run may delete before aborting. |
| `exclude` | `Vec<String>` | `[]` | Extra rclone exclude patterns, appended to the built-in excludes (e.g. `"**/test-data/**"`). |
| `max_size` | `String` | `""` | Skip files larger than this rclone size string (e.g. `"100M"`); empty means no cap. |

Two derived predicates:

- `SyncConfig::is_configured()` — `enabled && !b2_bucket.trim().is_empty()`.
  Sync only counts as "configured" once both the switch is on *and* a bucket is
  named.
- `SyncConfig::watch_effective()` — `is_configured() && watch`. The watcher is
  on by default whenever sync is configured; `watch: false` is the explicit
  opt-out.
- `SyncConfig::crypt_enabled()` — `!crypt_password.trim().is_empty()`. When
  true, `sync::remote::build_remote` returns the env-defined `BRAINCRYPT:`
  remote layered over the B2 remote instead of the raw `BRAIN:<bucket>/<path>`
  target.

The sync transport executable is not part of this data model. `brain sync`
checks for external `rclone` before invoking the configured remote.

`SyncConfig::load(command)` reads the `sync` key out of the selected workspace
record and deserializes it, falling back to `SyncConfig::default()`
on a missing key or a parse failure — a broken or absent `sync` block never
blocks startup.

## Remote workspace identity (`sync/identity/`)

Every remote data path is owned by one portable workspace manifest at
`.config/workspace.json`. The remote file uses the same strict schema and UUID
as the selected root's local manifest. The pure observation distinguishes an
empty target, a nonempty manifestless target, a compatible manifest with its
UUID, an invalid or incompatible manifest, and a manifest that is listed but
cannot be read. The identity decision then
allows a match, permits setup initialization for an empty target, and refuses
mismatched UUIDs or untrusted manifests. Ownership claims used during empty
initialization live at `.config/workspace-claims/<workspace-uuid>.json`. They
contain the claimant's exact manifest bytes and are append-only setup metadata;
claim paths alone do not make the target nonempty for setup retry.

Ordinary sync, push, pull, repair, and `brain check` accept only the matching
outcome. Setup first publishes and reads back its UUID-named claim. A newly
published claim ends that attempt without canonical publication. A retry
strictly enumerates and validates all claim names and the elected claim
contents, and elects the lexically lowest UUID. It then re-probes the canonical manifest;
only the winner may publish the selected root's exact existing manifest bytes
to an empty remote, using immutable-copy defense, and must read them back and
revalidate before saving the
candidate sync block or writing check markers, CSVs, counters, or bisync data.
The local manifest is immutable validation input in this flow and is excluded
from ordinary rclone transfer. A remote that already contains data but lacks a
manifest is never adopted implicitly. Setup displays the local canonical name
and UUID, target, and observed remote status, then requires either an explicit
interactive confirmation or `--adopt-workspace-id <UUID>` matching the exact
selected UUID. The setup stages hold the UUID-scoped sync lock from identity
through task-schema preparation and initial baseline, then persist credentials
only for a clean outcome. No portable remote workspace name exists. Ordinary sync and
internal server paths cannot supply adoption authority or prompt. Registry
records, machine-local env credentials, and UUID-derived runtime state remain
outside the portable remote.

## Sync journal (`src/sync/journal.rs`, `<workspace-cache>/sync/journal.db`)

Every `brain sync` run (including `setup`'s initial baseline) is recorded into
a SQLite journal, machine-local and **never synced** (it lives under the
selected UUID cache, not inside the brain root). WAL mode, like the
state DB. One table:

```sql
sync_runs(
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  started_at    TEXT NOT NULL,     -- RFC3339 UTC timestamp
  finished_at   TEXT NOT NULL,     -- RFC3339 UTC timestamp
  direction     TEXT NOT NULL,     -- "both" | "push" | "pull" | "resync"
  outcome       TEXT NOT NULL,     -- "clean" | "needs_attention" | "aborted"
  transferred   INTEGER NOT NULL,  -- files transferred (rclone's file-count line)
  deleted       INTEGER NOT NULL,  -- files deleted
  conflicts     INTEGER NOT NULL,  -- conflict copies renamed to their friendly name this run
  errors        INTEGER NOT NULL,  -- rclone-reported transfer errors
  note          TEXT NOT NULL      -- human-readable detail; empty when outcome is "clean"
)
```

`Journal::record` inserts a row per run; `Journal::recent(n)` returns the last
`n`, newest (`id DESC`) first — `brain sync status` reads just the most recent
one (`command::format_last_run`) alongside the configured trigger flags
(`command::format_triggers`) and the open-conflict count.

`Journal::latest_downstream_completion()` returns the newest non-aborted
`both`, `pull`, or `resync` completion. Push-only rows do not refresh this
timestamp. The receiver compares it with the current UTC time and starts a
pull before dispatch only when it is missing or more than two hours old.

**Outcome classification** (`src/sync/verify.rs`,
`classify(run, conflicts, leftover_markers)`): `Clean` only when rclone exited
successfully, reported zero errors, created **no** conflict copies, and left no
un-renamed conflict markers; a nonzero error count, a `conflicts > 0` count
(copies created and renamed this run), or a leftover marker is
`NeedsAttention`; an rclone abort (the `--max-delete` guard, or rclone's own
"prior listings missing" guard — see [integrations.md](integrations.md)) is
`Aborted`. Surfacing `conflicts > 0` is what keeps a real conflict from being
reported as clean once its markers have been renamed away (leftover 0). Both
carry a human-readable `note` that becomes the journal row's `note` and the
message `brain sync` prints. `note` can also carry (prefixed onto whatever
outcome message would otherwise apply) **"auto-resumed after interrupted
baseline"** — `command::sync_once` sets this when a run aborted with
`AbortKind::PriorListingMissing` and it automatically retried once as a
resync (see [integrations.md](integrations.md) and
[decisions.md](decisions.md) for why this one abort kind is safe to
auto-retry while others are not).

## Sync lock (`src/sync/lock.rs`, `<workspace-cache>/sync/sync.lock`)

The advisory lock is one file per workspace UUID (beside that workspace's sync
journal, machine-local and never synced). Different workspaces may sync
concurrently; the same workspace still serializes every trigger. Its "record"
is intentionally minimal: **the file's contents are
the bare owning PID** (the decimal `std::process::id()`, no JSON, no timestamp).
The timestamp is the file's mtime: `Guard` refreshes it from a heartbeat thread
while the sync is running. It is created atomically with `create_new` (O_EXCL),
so a second acquirer can't race in; `try_acquire` returns `Some(Guard)` on
success (the `Guard` stops the heartbeat and removes the file on drop, but only
if it still holds our PID) or `None` when a live, fresh sync already holds it. A
stale lock (owner PID no longer alive per `kill -0`, heartbeat mtime older than
the stale cap, or an unreadable/garbage file) is reaped and re-taken. See
[integrations.md](integrations.md) for how every sync trigger coalesces through
it.

## Check-access marker (`RCLONE_TEST`)

rclone's `--check-access` guard requires a marker file named `RCLONE_TEST` at
both sync roots. brain owns that marker lifecycle through
`src/sync/check_access.rs`: `brain sync setup` and `brain sync repair` write a
generic `<brain-root>/RCLONE_TEST` file and copy it to the remote root before
the resync baseline is established. The marker contains no secrets and is
ordinary synced metadata. Normal sync runs do not recreate it proactively; if it
is missing on either side, rclone aborts and brain automatically announces and
runs the narrow `brain sync repair` flow. An explicit repair remains available
when the automatic recovery cannot complete.

## Conflict-copy naming (`src/sync/conflicts.rs`)

`rclone bisync` is configured (`args::bisync_args`) to keep, not drop, the
losing side of a same-file conflict, marking it with a `__brainconflict__`
suffix on the filename. rclone's real format is
`<original>.<MARKER><N>` — a literal dot, the suffix, and a trailing integer
`N` ≥ 1 (e.g. `one.md` → `one.md.__brainconflict__1`, `README` →
`README.__brainconflict__1`); the marker lands on **both** sides. Right after
the rclone run, `conflicts::rename_markers` walks the brain root and renames
every such marker file (matched by `conflicts::is_marker`, which strips the
`.<MARKER><digits>` tail) to a friendly name:

```
name (conflict <host> <date>).ext
```

— e.g. `note.md` → `note (conflict mac 2026-07-25).md`; an extensionless
`README` → `README (conflict mac 2026-07-25)`. `<host>` is this machine's
short (unqualified) hostname (`command::hostname`) and `<date>` is the sync
run's date (`YYYY-MM-DD`). The patterns `*(conflict *)*` (friendly copies) and
`*.__brainconflict__*` (raw markers) are both default rclone excludes, so
neither is synced back out on a later run (the marker exclude does not stop
rclone from creating the initial copy). `conflicts::list_conflicts` finds
existing friendly-named copies under the root (paths relative to the root) for
`brain sync conflicts` and the `brain sync status` open-conflict count;
`leftover_markers` counts any `__brainconflict__` files the rename pass failed
to rewrite, which `verify::classify` surfaces as `NeedsAttention`.

## Conflict grouping + `conflicts --json` (C5, `src/sync/conflicts.rs`, `src/sync/command/mod.rs`)

`parse_conflict_name` is the strict inverse of the friendly-name builder
above: given a path, it recovers a `ParsedConflict { original, host, date }`,
or `None` if the file name isn't exactly the `name (conflict <host>
<date>).ext` grammar (rejects raw markers, malformed dates, empty hosts, and
merely-similar titles). `group_conflicts(files: &[ConflictFile]) ->
Vec<ConflictGroup>` folds the flat `list_conflicts` output into one group per
canonical original:

```rust
struct ConflictGroup { original: PathBuf, copies: Vec<ParsedCopy> }
struct ParsedCopy { path: PathBuf, host: String, date: String }
```

Copies that don't parse are dropped; both the group list and each group's
copies are sorted for deterministic output. `copies_for_original(original,
files)` returns just one original's copy paths (never the original itself) —
the lookup `brain sync resolve` uses to know what to delete.

The plain `brain sync conflicts` line-list is derived from those same
`ConflictGroup` values, not directly from the looser filesystem scan, so it
matches `--json` on what counts as an open conflict copy.

`command::conflicts_json` (`src/sync/command/mod.rs`) renders `&[ConflictGroup]`
into the JSON `brain sync conflicts --json` prints — a `serde_json::Value`
array, one object per group:

```json
[
  {
    "original": "notes/idea.md",
    "original_exists": true,
    "copies": [
      {
        "path": "notes/idea (conflict mac 2026-07-25).md",
        "host": "mac",
        "date": "2026-07-25",
        "modified": "2026-07-25T10:04:11Z",
        "bytes": 1841
      }
    ]
  }
]
```

All paths are relative to the brain root. `original_exists` and each copy's
`modified`/`bytes` are read off the filesystem by injected closures (kept out
of the pure builder for testability); a copy whose metadata can't be read
serializes `modified`/`bytes` as JSON `null` rather than omitting the fields.
No groups at all serializes as `[]`. This is the shape the `/second-brain
resolve-conflicts` skill parses; see [integrations.md](integrations.md) for
the resolve side of the contract.

## CSV semantic merge (`src/sync/csv_merge/`, `src/sync/csv_sync/`)

`tasks/tasks.csv` and `tasks/habits.csv` are excluded from the bisync file
lane (`args::bisync_args`'s default excludes) and reconciled instead by a
pure, id-keyed 3-way merge, so the two files never produce a `(conflict …)`
copy the way a bisync'd file would (see [integrations.md](integrations.md)
for the transport, [decisions.md](decisions.md) for why).

Their two id counters, `tasks/.tasks_next_id` and `tasks/.habits_next_id`, are
likewise excluded from bisync and reconciled out-of-band. The counter takes
`max(local, remote, emitted_max + 1)`, so display-ID reconciliation cannot
leave a counter able to issue an ID that sync just emitted.

`Table` (`csv_merge/table.rs`) is the parsed shape, keyed by the active merge
column found by name:

```rust
struct Table {
    header: Vec<String>,                  // preserved output order
    rows: BTreeMap<String, Vec<String>>,  // task_uuid, or legacy task_id -> cells
    schema_status: SchemaStatus,          // selected from tasks/SCHEMA.json
}
```

**Portable identity is adopted, not minted, when a machine joins.** A root with
no `.config/workspace.json` and a configured remote takes the remote's manifest
(`src/sync/identity/adopt.rs`): Brain reads it, refuses it unless its
`workspace_id` matches the registry record, and writes it locally, so
`receiver_ingress_id` stays identical across machines. Minting from the registry
UUID is the fallback only when the remote carries no manifest either. This runs
*before* the first sync, because the sync's identity gate reads the manifest. It
matters because the manifest is excluded from bisync, so a locally minted one
would fork portable identity with nothing able to reconcile it.

Whether a remote is *legacy* is decided by what its task CSVs contain, not by
whether CSV files exist: `classify_remote_csvs`
(`src/sync/csv_merge/remote_csvs.rs`) returns `Absent`, `Current`, or `Legacy`,
and empty content proves nothing. A remote missing only its schema document has
it published during the sync, so no separate command is needed.

`tasks/SCHEMA.json` is required input for every schema decision, so Brain
carries the canonical current document (`src/tasks/schema/task_schema.json`,
embedded with `include_str!`) and seeds it into any workspace that has none.
Seeding is write-only-when-absent, exactly like the portable manifest: a
document that arrived over sync is authoritative and never replaced. It runs
both when an empty workspace is initialized and, for a workspace created before
Brain shipped the document, on the ordinary root-initialization path, so an
existing workspace repairs itself. `src/tasks/schema/seed.rs` owns it, and
`RequirementScope::TaskSchema` reports a workspace that still lacks it as
`incomplete`.

`merge(base, ours, theirs) -> (Table, Report)` uses `task_uuid` only when
`tasks/SCHEMA.json` activates the current task schema, and otherwise preserves
the inactive-migration compatibility path keyed by legacy `task_id`. Merely
adding a `task_uuid` column does not activate migration: normal writers may
populate it for new rows while existing rows remain blank. Rows are aligned by
column name before these rules run:

- **Present on one side only, absent from `base`** — added; kept as-is.
- **Added on both sides under the same id** — field-merged against an empty
  base (below), so identical adds collapse and differing adds still
  reconcile.
- **In `base`, missing from one side, unchanged on the other** — deleted.
- **In `base`, missing from one side, but edited on the other** — the edit
  wins over the delete (a soft conflict, journalled: "deleted on one side but
  edited on the other; kept the edit").
- **Present everywhere, unchanged from `base` on one or both sides** —
  whichever side actually changed it wins; unchanged-on-both keeps the
  `base` row untouched.
- **Changed on both sides relative to `base`** — a cell-by-cell field merge
  (`field_merge`):
  1. **Completion wins first, at the row level.** If exactly one side set
     `status=done`, that side's `status` and `completed_date` cells win
     outright before any other column is even considered.
  2. **Every other column merges independently.** A column unchanged from
     `base` on one side takes the other side's value (so disjoint field
     edits from both sides survive together); a column changed to the
     *same* value on both sides takes that value; a column changed to
     *different* values on both sides is a genuine same-field conflict,
     resolved by `resolve_conflict`.

`resolve_conflict` is last-writer-wins keyed off the row's own
`last_touched` column. Both `tasks.csv` and `habits.csv` carry it, and every
row mutator stamps the changed row before writing; the side with the greater
`last_touched` wins, with ties broken by the greater cell value so the
outcome never depends on which side is "ours" vs. "theirs" (needed for
convergence, below). A legacy or malformed table without the column still
falls back to a deterministic lexicographic tiebreak (the greater cell value
wins), noted as a soft conflict in the `Report`.

The output header is the deterministic union of names from local, remote, and
base. Under schema version 2, every known task field uses one canonical order
and forward-compatible fields follow in lexical order. Schema version 2 requires `task_uuid`, `task_id`, `assigned_to`, and
`system_key`; `last_touched` remains the preferred conflict timestamp but is
not an identity requirement. A nonempty legacy table must contain `task_id`.
Unknown columns survive only when `SCHEMA.json` declares
`forward_compatible_columns: true`. The manifest and all six base/local/remote
task and habit tables are preflighted together. The remote schema marker is
fetched and parsed before either remote CSV; absence is exactly legacy, while
malformed, incompatible, newer, or local/remote status mismatch is a refusal.
Any rejection occurs before
either CSV, baseline, metadata file, remote object, or counter changes.

After row merge, `reconcile.rs` groups equal display IDs. The
lexicographically smallest UUID retains each contested label; loser UUIDs are
ordered deterministically and assigned numbers after the maximum display
number across all three inputs. `relationships.rs` first resolves each side's
pipe/comma-separated `blocked_by` labels through that side's pre-reconciliation
display-to-UUID map, then emits the final labels. `see_also` is free text: task
IDs may be space-separated or surrounded by punctuation because writers append
URLs with a space. Its column-specific rewrite changes only bounded `T###` or
`H###` references outside `http(s)` URLs, preserving whitespace, punctuation,
separators, URLs, and text such as `T100` when only `T10` changed. It falls back
to the original display label when a referenced row is deleted, so temporary
UUID markers never reach disk. The same final table derives each project's
`.METADATA.json:tasks[]`; every metadata file is parsed and staged before any
local rewrite. Remote publication sends every authoritative metadata file,
including locally unchanged files, so retry heals a prior partial upload.

`serialize` writes rows in current merge-key order (the `BTreeMap`'s natural
ordering), so two machines merging the same three inputs — even with
`ours`/`theirs` swapped — produce **byte-identical** output (convergence),
and merging an already-merged table with itself is a no-op (idempotency);
both properties are asserted directly in `csv_merge`'s test suite
(`convergence_swapping_ours_and_theirs_is_byte_identical`,
`idempotency_merging_a_merged_table_with_itself_is_a_no_op`).

**Baseline.** `csv_sync::baseline_path(paths, name)` resolves to
`<workspace-cache>/sync/baselines/{tasks.csv,habits.csv}` — a machine-local
cache of the last-synced (post-merge) content for that file, never synced
itself, alongside the selected workspace's sync journal. `sync_one`
reads it as `base` (empty if absent, so the very first CSV sync on a machine
merges as a safe union of local + remote); after merging, it writes the
result to the local file and the remote (via `rclone copyto`), then
overwrites the baseline with that same merged text so the next sync's `base`
reflects exactly what was agreed this round. The whole-operation result also
carries task and habit display-ID floors directly from those reconciled tables;
counter reconciliation does not fetch either remote CSV again. Push-only sync
still advances the local counters to those floors before the next allocation.
The gated two-workspace local transport test runs baseline publication for two
UUIDs concurrently and asserts both path and byte ownership remain separate.
Its mismatch case leaves the selected workspace's workdir absent, which pins
the remote-manifest gate ahead of both bisync state and portable mutation.

**Journal note.** `command::format_csv_note` folds the `Report` from both
CSVs into one segment appended to the sync journal's `note` column (see
"Sync journal" above), e.g. `csv: +3 ~2 -1 (1 soft)` (added/merged/deleted
counts, plus a soft-conflict count when nonzero); empty when nothing
changed, so a clean run's note isn't cluttered by a no-op CSV pass.

**Read-only pending diff.** `brain check` does not run the full 3-way merge
or update any CSV state. Instead `check::CsvSideDiff` compares one side
against the cached baseline by `task_uuid` when the current task schema is
active (legacy `task_id` otherwise), aligns cells by column name, and counts
whole-row additions,
changes, and deletions. `check::CsvPending` holds one push diff
(`baseline` vs. local CSV) and, when the remote fetch succeeds, one pull diff
(`baseline` vs. remote CSV). This is a preview of pending row movement, not a
merge-result adjudication: same-field last-writer-wins is still applied only
by `brain sync`. If the baseline text is missing, `check` treats identical
local/remote CSVs as clean instead of double-counting both sides; when both
sides are non-empty and differ, it uses the remote CSV as a provisional
snapshot for local deltas so a local-only task addition does not appear as a
spurious pull. Schema metadata and all three CSV generations use a fallible
read boundary. Invalid schema, malformed records, or duplicate active keys
stop the preview with a labeled warning, without mutating CSVs, baselines,
metadata, counters, or remotes and without reporting a false clean state.

## Binary stdout (the output "schema")

The intentional stdout families are `config/env/version`, `workspace list`,
explicit plain-task output, and help. `--verbose` mirrors logs to stdout for
non-TUI commands. Clap errors and diagnostics go to stderr. The TUI renders to
`/dev/tty`. There is no plan protocol; the TUI performs its
file-open, Finder, PDF, trash, and `claude`-launch effects by spawning
processes itself. See [integrations.md](integrations.md).

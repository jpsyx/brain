# Data model

Most of `brain`'s "model" is the in-memory representation of `~/brain`'s
directory tree plus the picker's match state. The **persistent shell** adds
a small SQLite store (sessions + layout) — see "Persistent state" below.

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
    display: String, // `~/brain/...` form, for the UI and fuzzy matching
    bucket: Bucket,  // which section it renders under
}
```

`collect(brain, roots)` produces these by walking each `(Bucket, root)`
pair with `walkdir`:

- **Hidden files are skipped** — any path component starting with `.`
  (`.git`, `.DS_Store`, dotfiles). This mirrors the old `fd .` default.
- **The root itself is skipped** (`depth() == 0`); only its contents are
  pickable.
- **`display` rewrites `$HOME/brain/...` → `~/brain/...`** via
  `display_path`, which strips the `brain.parent()` prefix. Paths outside
  that prefix fall back to their absolute form.
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
    local_user_id: String, // this machine's user identity in the workspace
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

`WorkspacePaths` derives its full base from the ID:

```text
<home>/.cache/brain/workspaces/<workspace-uuid>/
```

Its state database, TUI lock, inbox, responses, logs, and sync working data are
all children of that base. `cache_dir()` borrows the stored base; each child
accessor derives an owned path. Distinct IDs therefore cannot share runtime
paths.

### Machine registry schema v2 (`workspace/registry/`)

The sole machine-global workspace registry is
`$XDG_CONFIG_HOME/brain/env.json`, or `~/.config/brain/env.json` when XDG config
is unset. Deterministic ordered names and aliases make its JSON stable:

```json
{
  "schema_version": 2,
  "default_workspace": "brain",
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
default changes no record. Removal detaches a record only and never touches its
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
message. Startup migration and the default-record env compatibility writer use
this same transaction owner.

### Workspace CLI decisions (`workspace/command/`)

Clap retains `--brain/-b` as an unresolved `Option<String>`. At the registry
boundary, `MachineRegistry::select` case-folds it and resolves a canonical name
or alias. Before Clap delegates a trailing task argument list, one shared
real/test normalization extracts `--brain value`, `-b value`, or
`--brain=value` from any pre-`--` position and keeps the exact raw value.
Selector-looking tokens after `--` remain delegated values. Bootstrap applies
this selection once for every ordinary command and returns one immutable
`CommandContext`. Full store-signature threading remains Task 6.

Collected management values first become a pure `Mutation` enum. `Create` and
`Attach` carry a validated canonical name plus an absolute, tilde-expanded,
lexically normalized root. Rename and alias decisions carry validated new
names; default/removal carry only selectors. In particular, `Remove` has no
filesystem path or deletion operation. The shell then loads schema v2 directly
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
  "minimum_brain_version": "0.16.0"
}
```

Unknown fields, unsupported schemas, malformed UUIDs, and incompatible minimum
versions are errors. The manifest UUID must equal the selected registry record;
the stable receiver ingress UUID remains portable across machines. Create
writes the manifest before registry persistence. Attach reads it without
editing it. Legacy flat-env migration creates the root and first matching
manifest before replacing the flat registry.

`workspace::bootstrap` maps every parsed route to `None`, `InternalNoPrompt`,
`RegistryOnly`, or `ReadyWorkspace`. Only the last class selects and validates
a record. Readiness is manifest validity/UUID agreement plus a non-empty
machine-local `local_user_id`. Missing values become a pure
`ReadinessAction::Prompt(fields)` interactively or a typed error carrying exact
repair commands headlessly. Successful interactive repair happens under the
registry transaction, then bootstrap reloads and constructs one
`CommandContext` containing `Arc<WorkspaceContext>` and `RegistryStore`.

The first create deliberately leaves `local_user_id` empty. Its next ordinary
command is therefore the prompt/remediation boundary; after repair that same
requested command continues.

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
replacement. A valid v2 input is never rewritten or backed up, making reruns
UUID-stable and byte-stable.

## Persistent state (`state.rs`, `~/.cache/brain/state.db`)

The persistent shell tracks Claude sessions and the layout preference in
SQLite (WAL). Codex panels currently launch fresh because their launch semantics
remain frontend-specific. Receiver completion is hook-backed in both frontends.
Two tables:

```sql
brain_sessions(
  claude_session_id  TEXT PRIMARY KEY,
  brain_instance_id  TEXT NOT NULL,  -- one per running `brain` shell (a lineage)
  locked_pid         INTEGER,        -- live brain holding it, or NULL when free
  source             TEXT,           -- last SessionStart source (startup/resume/clear/…)
  channel            TEXT NOT NULL DEFAULT 'interactive', -- interactive | sms | email
  created_at         INTEGER NOT NULL,
  last_active_at     INTEGER NOT NULL
)
meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)  -- key 'panel_side' = 'left' | 'right'
```

**The lock + recency model.** A session is "free" when `locked_pid IS NULL`.

- `free_sessions_by_recency` → free sessions, newest (`last_active_at DESC`)
  first. The caller walks them and resumes the first whose **transcript
  exists** on disk (`tui::session_transcript_exists` +
  `session::project_dir_name`) — a session opened but never chatted in has a
  DB row but no `<id>.jsonl`, and `claude --resume` can't find it, so it's
  skipped (and the user gets a status-line alert when that forces a fresh
  chat).
- `claim` → lock a free session to this shell's PID (loses cleanly if
  another shell grabbed it first).
- `register_fresh` → insert a brand-new session, locked.
- Channel sessions are reserved for exactly one reusable SMS session and one
  reusable email session. Remote messages select their channel row, while
  interactive work uses the `interactive` role. A `/new` inbound command
  creates a fresh row for that same channel.
- Receiver runtime state distinguishes an active remote job
  (`receiver_started` is set) from a warm channel panel (`receiver_session_id`
  plus a three-minute `receiver_lease`). A warm lease never counts as active
  LLM work. This lets Stop-hook completion release queued work while keeping
  the completed SMS/email conversation visible and reusable.
- `release` → when the panel closes (claude exits) or the shell quits, clear
  this instance's locks and stamp `last_active` (floats it to the top of the
  next resume — so re-opening with "Message brain" picks it back up, and a
  second terminal could too).
- `reap_dead_locks` → on startup, free locks whose PID is no longer alive
  (`kill -0`), so a crashed shell doesn't strand its session.

The invariant: at most one live shell holds a given session (no tangled
threads), and exactly one session per instance is current (the SessionStart
hook frees the instance's others on every start, handling `/new`). The
`PanelSide` enum (`Left` / `Right`, default `Right`) lives in `state.rs`
because it's the persisted layout value.

**The daily-triage tab is deliberately *absent* from this table.** The
ephemeral triage session (`App.triage_brain`) is launched with
`session::env_for_triage`, which omits `BRAIN_INSTANCE_ID` / `BRAIN_STATE_DB`;
the SessionStart hook no-ops without them, so no `brain_sessions` row is ever
written and it is never a resume candidate. It lives only in process memory
(`App.triage_brain` / `App.active_brain_tab: BrainTab` / `App.triage_token`) and
is torn down when triage completes or the shell exits.

## Daily-triage completion signal (`triage_signal.rs`, `~/.cache/brain/triage-done.json`)

The cross-process signal that closes the daily-triage tab. When the `/triage`
skill finishes a background pass it POSTs `{"token": "<one-time-token>"}` to the
brain server's `POST /triage/done`; the handler writes:

```json
{ "token": "<one-time-token>", "at": 1730000000 }
```

`token` is the value brain handed the session in `BRAIN_TRIAGE_TOKEN`; `at` is
an epoch-seconds diagnostic. The TUI polls this file each tick and closes the
triage tab only when `token` equals the token of the tab it opened, so a stale
signal from an earlier run cannot close a fresh tab. `parse_token` is pure; the
file IO (`record_done` / `read_token` / `clear`) is a thin shell around it.

## Personalization (`personalization/`, `<brain-root>/.config/personalization.json`)

Content *about you*, stored beside `config.json` in the brain config dir
(`settings::config_dir()`) — just another brain config, inside the brain root so
it travels with the brain. A missing/broken file parses to the default (empty)
value — the app never requires personalization.

`Personalization` (`personalization/model.rs`):

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `name` | `String` | `""` | Optional display name. |
| `role` | `String` | `""` | Free-text role the assistant serves. |
| `works_for` | `String` | `""` | Org, `myself`, or empty. |
| `namespaces` | `Vec<String>` | `[]` | Project `<namespace>__<outcome>` life-buckets. Empty falls back to the generic defaults (`work`, `personal`); edited via the onboarding / `brain config set namespaces` checklist (`personalization/namespaces.rs`). |
| `tag_styles` | `Map<String, TagStyle>` | `{}` | Per-tag display overrides. The tag *set* (its keys) is chosen via the same checklist (`brain config set tags`). |

`TagStyle` (`personalization/tags.rs`) is `{ emoji: String, label: String }`,
rendered as `"{emoji} {label}"`. Resolution (`TagStyles`) layers the user's
overrides over the generic defaults (`mit` → `❗ MIT`, `personal` → `✌ personal`,
`work` → `💼 work`); an unknown tag falls back to its raw name. The renderer
reads a process-cached copy (`personalization::runtime`, loaded once at startup)
so tag labels resolve without threading state through every render signature;
unit tests see the generic defaults (the cache is uninitialized), keeping them
hermetic.

The `brain personalize show` block is the **skill-lookup contract**: a stable,
keyed `name:`/`role:`/`works_for:`/`namespaces:` text block that
identity-dependent skills read at runtime to learn who they serve. The
`namespaces:` line always shows the *effective* set (the configured list, or the
generic defaults when unset), so a skill like `second-brain` always sees a usable
namespace list.

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
the registry. Existing env/runtime callers still read and atomically update the
default record through a compatibility view until runtime context threading
lands.
See [config.md](config.md) for migration and storage details.

`env::schema::VARS` (`src/env/schema.rs`):

| Variable | Type | Default | Meaning |
| --- | --- | --- | --- |
| `root` | `String` | `~/brain` | The selected record's absolute lexical brain root. During legacy migration its source precedence is flat root → read-only pointer → `<home>/brain`; commands that need the directory use `paths::brain_root()`, which creates it on demand, while `env::vars::resolve_one("root")` and `paths::brain_root_path()` remain side-effect-free. |
| `markdown_to_pdf_path` | `String` | *(unset)* | Path to the `markdown-to-pdf` command on this machine. Auto-discovered and self-healed by the startup gate (`settings::markdown_pdf`). |
| `claude_cmd` | `String` | `claude --dangerously-skip-permissions` | Command used to launch the Claude brain-panel frontend on this machine. Read by `env::claude_command`; blank falls back to the default, and a legacy portable config value is honored only when env is unset. |
| `codex_cmd` | `String` | `codex` | Command used to launch the Codex brain-panel frontend on this machine. Read by `env::codex_command`; blank falls back to `codex`. |

All declared env variables and recursively flattened nested values render
through the same `Resolved { name, value, description }` type `brain config`
uses (re-exported from `settings::schema::Resolved`), so `brain env list`
shares its table layout with `brain config list`. Nested paths use dot
notation, for example `sync.remote.key_id`; array elements use numeric path
segments.

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
`~/.cache/brain/sync/`:

- `current.json` — the [`current::CurrentState`] record of the sync in progress
  right now: `{ pid: u32, direction: String, started_at: String }`. Written
  when a run starts, removed when it ends (or when its `Reporter` drops). Its
  presence, validated against `pid`'s liveness, is how `brain sync status` and a
  following `brain sync` know a sync is underway; a hard-killed run's leftover
  record reads as not-running.
- `current.log` — the in-progress run's progress lines, appended live so a
  following `brain sync` can tail them and `brain sync status` stays honest.
  Truncated at the start of each run.
- `bisync/` — the brain-owned rclone bisync workdir (`--workdir`): its `.lst`
  baseline listings, and any `.lck` lock file (reaped before each run while
  brain holds its own sync lock, since it can only be from a dead run).

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

`SyncConfig::load()` reads the `sync` key out of the brain-env store
(`env::load_map()`) and deserializes it, falling back to `SyncConfig::default()`
on a missing key or a parse failure — a broken or absent `sync` block never
blocks startup.

## Sync journal (`src/sync/journal.rs`, `~/.cache/brain/sync/journal.db`)

Every `brain sync` run (including `setup`'s initial baseline) is recorded into
a SQLite journal, machine-local and **never synced** (it lives under
`~/.cache`, like `state.db`, not inside the brain root). WAL mode, like the
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

## Sync lock (`src/sync/lock.rs`, `~/.cache/brain/sync/sync.lock`)

The C4 machine-wide advisory sync lock is a single file at
`~/.cache/brain/sync/sync.lock` (beside the sync journal, machine-local cache,
never synced). Its "record" is intentionally minimal: **the file's contents are
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

## CSV semantic merge (`src/sync/csv_merge.rs`, `src/sync/csv_sync.rs`)

`tasks/tasks.csv` and `tasks/habits.csv` are excluded from the bisync file
lane (`args::bisync_args`'s default excludes) and reconciled instead by a
pure, id-keyed 3-way merge, so the two files never produce a `(conflict …)`
copy the way a bisync'd file would (see [integrations.md](integrations.md)
for the transport, [decisions.md](decisions.md) for why).

Their two id counters, `tasks/.tasks_next_id` and `tasks/.habits_next_id`, are
likewise excluded from bisync and reconciled out-of-band — but by a simpler
rule: `counters::merge_counter` takes `max(local, remote)` (stateless, no
baseline), the only rule that never regresses a monotonic counter and so never
lets a machine reuse an id the other already assigned. `None` on both sides
leaves the file absent, and id allocation falls back to `max_existing_id + 1`.

`Table` (`csv_merge.rs`) is the parsed shape, keyed by the first column:

```rust
struct Table {
    header: Vec<String>,                  // column order, task_id first
    rows: BTreeMap<String, Vec<String>>,  // task_id -> row cells
}
```

`merge(base, ours, theirs) -> (Table, Report)` unions the `task_id`s across
all three tables and resolves each id independently:

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

The header is chosen once per merge as whichever non-empty table has the
most columns (preferring `ours`, then `theirs`, then `base`), so a schema
superset survives a merge; every row is padded/truncated to that width
(`norm`) before any comparison runs.

`serialize` writes rows in `task_id` order (the `BTreeMap`'s natural
ordering), so two machines merging the same three inputs — even with
`ours`/`theirs` swapped — produce **byte-identical** output (convergence),
and merging an already-merged table with itself is a no-op (idempotency);
both properties are asserted directly in `csv_merge`'s test suite
(`convergence_swapping_ours_and_theirs_is_byte_identical`,
`idempotency_merging_a_merged_table_with_itself_is_a_no_op`).

**Baseline.** `csv_sync::baseline_path(name)` resolves to
`~/.cache/brain/sync/baselines/{tasks.csv,habits.csv}` — a machine-local
cache of the last-synced (post-merge) content for that file, never synced
itself, alongside the sync journal under `~/.cache/brain/sync/`. `sync_one`
reads it as `base` (empty if absent, so the very first CSV sync on a machine
merges as a safe union of local + remote); after merging, it writes the
result to the local file and the remote (via `rclone copyto`), then
overwrites the baseline with that same merged text so the next sync's `base`
reflects exactly what was agreed this round.

**Journal note.** `command::format_csv_note` folds the `Report` from both
CSVs into one segment appended to the sync journal's `note` column (see
"Sync journal" above), e.g. `csv: +3 ~2 -1 (1 soft)` (added/merged/deleted
counts, plus a soft-conflict count when nonzero); empty when nothing
changed, so a clean run's note isn't cluttered by a no-op CSV pass.

**Read-only pending diff.** `brain check` does not run the full 3-way merge
or update any CSV state. Instead `check::CsvSideDiff` compares one side
against the cached baseline by `task_id` and counts whole-row additions,
changes, and deletions. `check::CsvPending` holds one push diff
(`baseline` vs. local CSV) and, when the remote fetch succeeds, one pull diff
(`baseline` vs. remote CSV). This is a preview of pending row movement, not a
merge-result adjudication: same-field last-writer-wins is still applied only
by `brain sync`. If the baseline text is missing, `check` treats identical
local/remote CSVs as clean instead of double-counting both sides; when both
sides are non-empty and differ, it uses the remote CSV as a provisional
snapshot for local deltas so a local-only task addition does not appear as a
spurious pull.

## Binary stdout (the output "schema")

The binary's stdout carries only `brain config`/`brain env` output (the config
table, or a single value) plus clap's help / version / errors. There is no plan
protocol: the TUI renders to `/dev/tty` and performs its file-open, Finder, PDF,
trash, and `claude`-launch effects by spawning processes itself. See
[integrations.md](integrations.md).

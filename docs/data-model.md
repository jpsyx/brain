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

## Persistent state (`state.rs`, `~/.cache/brain/state.db`)

The persistent shell tracks Claude sessions and the layout preference in
SQLite (WAL). Two tables:

```sql
brain_sessions(
  claude_session_id  TEXT PRIMARY KEY,
  brain_instance_id  TEXT NOT NULL,  -- one per running `brain` shell (a lineage)
  locked_pid         INTEGER,        -- live brain holding it, or NULL when free
  source             TEXT,           -- last SessionStart source (startup/resume/clear/…)
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

## Brain env (`env/`, `~/.config/brain/env.json`)

Machine-local config, deliberately **outside** the brain root so it never rides
whatever syncs the brain directory. See [config.md](config.md) for the store
location, resolution, and the `brain env` command; this section is the schema.

`env::schema::VARS` (`src/env/schema.rs`):

| Variable | Type | Default | Meaning |
| --- | --- | --- | --- |
| `root` | `String` | `~/brain` | Path to the brain (PARA) directory on this machine. Resolved by `paths::resolve_root` (env key → legacy `~/.config/brain-root` pointer → default); `env::vars::resolve_one("root")` always shows the same value `paths::brain_root_path()` uses. |
| `markdown_to_pdf_path` | `String` | *(unset)* | Path to the `markdown-to-pdf` command on this machine. Auto-discovered and self-healed by the startup gate (`settings::markdown_pdf`). |

Both variables render through the same `Resolved { name, value, description }`
type `brain config` uses (re-exported from `settings::schema::Resolved`), so
`brain env list` shares its table layout with `brain config list`.

A third field, `sync`, is not in `VARS` (it isn't a scalar `brain env set`
target) but is a top-level key of the same JSON object — see below.

## Sync config (`sync/`, the `sync` block in `env.json`)

`sync::SyncConfig` (`src/sync/config.rs`) is a typed view of the `sync` object
nested under `~/.config/brain/env.json`'s top level. As of C2, `brain sync`
reads it to drive a real `rclone bisync` transport (see
[integrations.md](integrations.md) and [architecture.md](architecture.md));
`on_start`/`on_exit`/`watch` are configured now but only *act* as automatic
triggers in a later sub-project-C phase — today `brain sync status` just
displays them, and every sync is a manual `brain sync` invocation. An absent
`sync` block parses to all defaults, so sync reads as fully disabled and brain
behaves exactly as if the key didn't exist (`brain sync` prints "sync is not
configured — run `brain sync setup`" and does nothing).

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enabled` | `bool` | `false` | Master on/off switch for Backblaze B2 sync. |
| `b2_bucket` | `String` | `""` | B2 bucket name. |
| `b2_path` | `String` | `""` | Optional path prefix within the bucket. |
| `b2_key_id` | `String` | `""` | B2 application key id. |
| `b2_app_key` | `String` | `""` | B2 application key (secret; stays machine-local in `env.json`, never synced). |
| `on_start` | `bool` | `true` | Whether a future sync trigger fires on brain startup. |
| `on_exit` | `bool` | `true` | Whether a future sync trigger fires on brain exit. |
| `watch` | `bool` | `true` | Whether a future continuous watcher runs. See `watch_effective` below. |
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
message `brain sync` prints.

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

## Binary stdout (the output "schema")

The binary's stdout carries only `brain config`/`brain env` output (the config
table, or a single value) plus clap's help / version / errors. There is no plan
protocol: the TUI renders to `/dev/tty` and performs its file-open, Finder, PDF,
trash, and `claude`-launch effects by spawning processes itself. See
[integrations.md](integrations.md).

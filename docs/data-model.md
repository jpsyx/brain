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

## Personalization (`personalization/`, `~/.config/brain/personalization.json`)

Content *about you*, stored beside `config.json` in the brain config dir
(`settings::config_dir()`) — just another brain config, under `$HOME` rather
than inside the brain root. A missing/broken file parses to the default (empty)
value — the app never requires personalization.

`Personalization` (`personalization/model.rs`):

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `name` | `String` | `""` | Optional display name. |
| `role` | `String` | `""` | Free-text role the assistant serves. |
| `works_for` | `String` | `""` | Org, `myself`, or empty. |
| `tag_styles` | `Map<String, TagStyle>` | `{}` | Per-tag display overrides. |

`TagStyle` (`personalization/tags.rs`) is `{ emoji: String, label: String }`,
rendered as `"{emoji} {label}"`. Resolution (`TagStyles`) layers the user's
overrides over the generic defaults (`mit` → `❗ MIT`, `personal` → `✌ personal`,
`work` → `💼 work`); an unknown tag falls back to its raw name. The renderer
reads a process-cached copy (`personalization::runtime`, loaded once at startup)
so tag labels resolve without threading state through every render signature;
unit tests see the generic defaults (the cache is uninitialized), keeping them
hermetic.

The `brain personalize show` block is the **skill-lookup contract**: a stable,
keyed `name:`/`role:`/`works_for:` text block that identity-dependent skills
read at runtime to learn who they serve.

## Binary stdout (the output "schema")

The binary's stdout carries only `brain config` output (the config table, or a
single value) plus clap's help / version / errors. There is no plan protocol:
the TUI renders to `/dev/tty` and performs its file-open, Finder, PDF, trash,
and `claude`-launch effects by spawning processes itself. See
[integrations.md](integrations.md).

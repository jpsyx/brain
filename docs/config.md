# Configuration

`brain` reads a single optional file, `config.json`, sitting at the
project root (next to the `brain` wrapper and `Cargo.toml`).

## Schema

```json
{
  "root": "~/brain",
  "daily_triage_name_pattern": "Morning Triage",
  "linear_base_url": "https://linear.app/avandar/issue/",
  "day_rollover_hour": 6
}
```

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `root` | string (optional) | `$HOME/brain` | The brain directory `brain` operates on. A leading `~` / `~/` is expanded against `$HOME`. Read by `paths.rs`. |
| `daily_triage_name_pattern` | string | `"Morning Triage"` | Case-insensitive regex matched against habit *names* to find the habit that gates the tasks view's startup triage nudge. Empty (or invalid regex) disables the check. Read by `config.rs`. |
| `linear_base_url` | string | `https://linear.app/avandar/issue/` | Prefix a task's `linear_issue` identifier is appended to for the `Ctrl+O` "open link" action. Read by `config.rs`. |
| `day_rollover_hour` | int 0-23 | `6` | Local hour the "logical day" rolls over for the triage re-check on refresh. Out-of-range → default. Read by `config.rs`. |

Every field is optional; a missing field, a missing `config.json`, or a blank
`root` all fall back to the defaults above. `root` is parsed by `paths.rs`; the
other three by `config.rs::Config` (both read the same file and ignore fields
they don't use).

## Resolution order (`paths.rs`)

`brain_root()` resolves the root like this:

1. Locate `config.json`: from the running exe
   (`<root>/target/release/brain`), walk up three parents to `<root>` and
   look for `config.json`. A missing file is **not** an error.
2. Parse it (`parse_config_root`): read the `root` field. An empty string
   is treated as unset (`None`). Invalid JSON **is** an error (surfaced
   with the file path).
3. If a non-empty `root` was found, expand its tilde
   (`expand_tilde` → `$HOME`-relative for `~`/`~/…`, verbatim otherwise).
   Only a *leading* `~` is a home reference; `/a/~/b` is left alone.
4. Otherwise fall back to `$HOME/brain`.
5. The resolved path must be an existing directory, or `brain_root` errors
   with `"<path> does not exist"`.

## Testing the loader

The IO-touching wrapper (`brain_root`, which reads the exe path, the file,
and `$HOME`) is deliberately thin. The decisions worth testing are pulled
into pure functions:

- `parse_config_root(text) -> Result<Option<String>>` — JSON parsing and
  the empty-string-is-unset rule.
- `expand_tilde_with_home(raw, home) -> PathBuf` — tilde expansion against
  an explicit home, so tests don't depend on the real `$HOME`.

See `src/paths.rs` unit tests and `tests/root_resolution.rs`.

## Changing the root

Point `brain` at a different second-brain location by editing
`config.json`:

```json
{ "root": "/Volumes/work/brain" }
```

No rebuild needed for a config change — the binary reads it at startup.
(Editing `.rs` files does trigger the wrapper's auto-rebuild.)

## Persistent state (`~/.cache/brain/state.db`)

`config.json` is the only *user-edited* config. The **persistent brain
shell** also keeps machine-managed state in a SQLite DB at
`~/.cache/brain/state.db` (created on first run; see `state.rs` and
[data-model.md](data-model.md)):

- `brain_sessions` — the Claude sessions brain has launched/adopted, with a
  per-session PID lock, used to resume the right conversation (lock +
  recency). Written by both `brain` and the SessionStart hook.
- `meta` — small key/value store; today just `panel_side` (`"left"` or
  `"right"`), the side the brain panel sits on, set by the palette's "Move
  brain panel…" command and read on startup.

You don't edit this file by hand. Deleting it is safe: brain recreates it,
starts a fresh Claude session, and reverts to the default right-side layout.
The one-shot subcommands never touch it.

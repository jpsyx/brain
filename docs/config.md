# Configuration

`brain` splits its persisted state across **two config stores**, by lifecycle:

| Store | Path | CLI | Synced? | Holds |
| --- | --- | --- | --- | --- |
| **brain env** | `~/.config/brain/env.json` (fixed XDG-style path, **outside** the brain root) | `brain env {list\|get\|set}` | **No** — machine-local, never rides whatever syncs the brain directory | `root`, `markdown_to_pdf_path`, `claude_cmd`, `codex_cmd`, the `sync` block |
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

## brain env (`~/.config/brain/env.json`)

Machine-local config. It lives at a fixed path — `$XDG_CONFIG_HOME/brain` or
`~/.config/brain` (`paths::machine_config_dir`) — that does **not** depend on
the brain root, so it can hold `root` itself without circularity, and it never
rides whatever syncs the brain directory (Backblaze, a cloud drive, etc.).
Everything in it is created on demand; a fresh checkout has none.

| Variable | Default | Meaning |
| --- | --- | --- |
| `root` | `~/brain` | Absolute or `~`-relative path to the brain (PARA) directory on **this machine**. Replaces the legacy `~/.config/brain-root` pointer file (still read for back-compat; see below). |
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
| `brain env set <name>=<value>` | Set a scalar variable or dotted nested path and persist it into `~/.config/brain/env.json`, preserving sibling values. |

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

### `root`: now an env key, with legacy back-compat

`root` used to be the one thing that couldn't live in *any* config store —
you'd need to know the root to read the setting that tells you the root. Now
that brain env lives at a fixed path *outside* the brain root, that
circularity is gone: `~/.config/brain/env.json` is found without knowing the
root, so `root` is a normal (if machine-local) env variable.

Resolution order (`paths::brain_root_path`):

1. the `root` field in `~/.config/brain/env.json`, if present and non-empty;
2. otherwise the legacy `~/.config/brain-root` one-line pointer file (kept for
   back-compat, tilde-expanded against `$HOME`), if present and non-empty;
3. otherwise the default `~/brain`.

`brain_root()` (used when a command actually needs the directory) creates the
resolved path and any missing parent directories on demand.
`brain_root_path()` (used to derive the config dir) remains side-effect-free, so
env and config lookups don't create or require a brain directory.

**Migration.** On every startup brain runs a one-time, idempotent migration
(`env::migrate`): if `env.json` has no `root` key yet and the legacy
`~/.config/brain-root` pointer file exists, its contents are folded into
`env.json`'s `root` key. The same pass relocates any `markdown_to_pdf_path`
still sitting in `config.json` (from before this split) into `env.json`, then
removes it from `config.json`. Both steps are idempotent (a value already
present in `env.json` is never overwritten) and never fatal — a failed
migration just leaves you on the pre-migration resolution path, it doesn't
block startup.

You can still hand-edit `~/.config/brain-root` (or have a dotfiles tool track
it there — safe, since brain only ever *reads* it), but the supported path
going forward is `brain env set root=<path>`.

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

Provider credentials are machine-local values in `~/.config/brain/env.json`.
`brain receiver setup` prompts for the credentials required by the selected
SMS, email, or both-channel configuration and stores them there. Existing
process environment variables such as `TWILIO_AUTH_TOKEN` remain supported as
overrides. `brain env list` and `brain env get` redact secret values.

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
default above. The brain directory itself is resolved by `paths::brain_root_path()`
from the brain-env `root` key (or the legacy pointer / `~/brain` default; see
above — it is *not* a `brain config` variable). The runtime knobs
(`daily_triage_name_pattern`, `linear_workspace`, `day_rollover_hour`) are read
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
- `env/` units — the env schema/vars (`root`, `markdown_to_pdf_path`,
  `claude_cmd`, `codex_cmd`), the
  migration `plan` (pointer→`root`, config→env `markdown_to_pdf_path`
  relocation), and the store round-trip.
- `sync::config` units — `SyncConfig` field defaults, `is_configured`,
  `watch_effective`; plus `sync::args` (the bisync argv per direction),
  `sync::remote` (creds land only in env, never the arg), `sync::run` (parsing
  rclone's transferred/deleted/error/abort output), `sync::verify` (outcome
  classification), `sync::conflicts` (friendly-name rewriting), and
  `sync::command` (hostname, direction/label mapping, status formatting). See
  [data-model.md](data-model.md).
- `paths::parse_root_key` — reading the `root` field out of a raw `env.json` body.
- `paths::resolve_root` — the `root` env key → legacy pointer → default precedence.
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

## Persistent state (`~/.cache/brain/state.db`)

Neither config store is the only *user-edited* state. The **persistent brain
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
The `brain config`, `brain env`, and `brain tasks {complete,doctor,--no-tui}`
utility paths never touch it.

# Configuration

`brain` keeps everything it persists under one **brain config dir**,
`~/.config/brain/` (or `$XDG_CONFIG_HOME/brain/`):

| File / dir | Holds |
| --- | --- |
| `config.json` | paths + runtime knobs (`root`, `claude_cmd`, `markdown_to_pdf_path`, …) |
| `personalization.json` | content *about you* (name, role, who you work for, tag styles) |
| `extensions/<skill>.md` | additive personalization of a bundled skill (see [features](features.md)) |
| `plugins/<name>/` | whole user-owned skills installed alongside the bundled cores |

The dir lives under `$HOME` — **not** inside the brain root, and **not** in any
dotfiles repo (`brain` never writes there). Everything in it is created on
demand; a fresh checkout has none. Whether and how the dir syncs across machines
is handled externally (e.g. a dotfiles setup); `brain` itself just reads and
writes `~/.config/brain/`. Machine-specific values that happen to sync (like a
discovered `markdown_to_pdf_path`) are re-validated and auto-healed on the
machine that needs them.

This document is mostly about the **config store**
(`~/.config/brain/config.json`). It is created on demand — you don't commit it,
and a fresh checkout has none. Manage it with `brain config` rather than
editing it by hand (though hand-editing is fine). For personalization see the
[Personalization](#personalization) section below and
[data-model.md](data-model.md).

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
| `root` | `~/brain` | The brain (PARA) directory `brain` operates on. A leading `~`/`~/` is expanded against `$HOME`. Read by `paths.rs`. |
| `linear_workspace` | *(unset)* | Linear workspace slug (e.g. `acme`). `config.rs` interpolates it into `https://linear.app/<slug>/issue/`, to which a task's `linear_issue` id is appended for the `Ctrl+O` "open link" action. Empty → no Linear links. |
| `markdown_to_pdf_path` | *(auto-discovered)* | Path to the `markdown-to-pdf` command brain spawns for the "Create PDF" action. See below. Read by `settings/`. |
| `daily_triage_name_pattern` | `Morning Triage` | Case-insensitive regex matched against habit *names* to find the habit that gates the tasks view's startup triage nudge. Empty (or invalid regex) disables it. Read by `config.rs`. |
| `day_rollover_hour` | `6` | Local hour (0-23) the "logical day" rolls over for the triage re-check on refresh. Out-of-range → default. Read by `config.rs`. |
| `claude_cmd` | `claude --dangerously-skip-permissions` | Command that launches the brain panel's `claude` session; brain appends `--resume`/`--session-id` after it, so the value is the base command plus any of its own flags. Interpreted by the shell, so brain never depends on a shell alias. Blank falls back to the default. Read by `config.rs` (`claude_command()`), used by `session::build_claude_command`. Also settable as `claude-cmd` (the dash normalizes to an underscore). |
| `skills_auto_sync` | `true` | When `true`, a `config`/`personalize` mutation re-renders and installs the bundled skills into the agent registry (`skills::resync_skills`). Default `true` since the B4 cutover; set `false` to manage the registry only via explicit `brain skills sync`. Read by `src/skills/`. |

Every variable is optional; a missing file or missing field falls back to the
default above. `root` is read by `paths.rs` (and honored by the persistent
shell, which resolves the brain directory through `paths::brain_root()`); the
runtime knobs (`daily_triage_name_pattern`, `linear_workspace`,
`day_rollover_hour`, `claude_cmd`) by `config.rs::Config`;
`markdown_to_pdf_path` by `settings/`. They all read the same file and ignore
fields they don't use.

## The `markdown-to-pdf` prerequisite

`markdown-to-pdf` is a **hard prerequisite** — brain spawns it for the "Create
PDF" command. Its location is not hardcoded (the repo is public), so:

1. On first run brain **auto-discovers** it, in order: an executable named
   `markdown-to-pdf` on `$PATH`; then conventional bin dirs (`~/.local/bin`,
   `/usr/local/bin`, `/opt/homebrew/bin`, `~/bin`); then the login shell, which
   resolves an autoloaded shell-function wrapper to the script it wraps.
2. The first hit is persisted to `markdown_to_pdf_path`.
3. At every startup the configured path is validated. If it is unset and
   discovery finds nothing, or it is set but missing/not executable, brain
   prints a red `❌` error and exits, telling you to run
   `brain config set markdown_to_pdf_path=/path/to/markdown-to-pdf`.

The `brain config …` command itself is exempt from this gate, so you can always
`config set` your way out of a bad path.

## Resolution order for `root` (`paths.rs`)

`brain_root()` resolves the root like this:

1. Read the `root` field from the config store. A missing file is **not** an
   error; invalid JSON **is** (surfaced with the file path).
2. An empty string is treated as unset. A non-empty `root` has its tilde
   expanded (`~`/`~/…` against `$HOME`; only a *leading* `~` counts — `/a/~/b`
   is left alone).
3. Otherwise fall back to `$HOME/brain`.
4. The resolved path must be an existing directory, or `brain_root` errors with
   `"<path> does not exist"`.

## Testing the loaders

The IO-touching wrappers are thin; the decisions worth testing are pure:

- `settings/` units — schema resolution, the `config list` table layout, the
  prerequisite message wording, shell-output path extraction, value coercion.
- `paths::parse_config_root` — reading the `root` field, empty-is-unset.
- `paths::expand_tilde_with_home` — tilde expansion against an explicit home.
- `config.rs` units — `linear_base_url` interpolation, `claude_command`
  (default, explicit override, blank falls back), defaults, ignoring unknown
  keys.

See those modules' unit tests and `tests/root_resolution.rs`.

## Personalization

Personalization is content *about you*, stored beside `config.json` in the brain
config dir at `~/.config/brain/personalization.json`. It is just another brain
config — it lives under `$HOME`, not inside the brain root. Manage it with
`brain personalize` (see [features.md](features.md)); the schema lives in
[data-model.md](data-model.md).

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

The config store is the only *user-edited* config. The **persistent brain
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
The `brain config` and `brain tasks {complete,doctor,--no-tui}` utility paths
never touch it.

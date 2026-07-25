# Brain Sync C1 — Brain env / brain config split — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a second config store — **brain env** (`~/.config/brain/env.json`, managed by a new `brain env {list|get|set}` CLI, machine-local, never Backblaze-synced) — holding `root`, `markdown_to_pdf_path`, and a parse-only `sync` block. Everything else stays **brain config** (`<brain-root>/.config/config.json`, `brain config` CLI, rides the brain-dir sync). This lets Sub-project C sync the brain dir without leaking machine-level paths/secrets.

**Architecture:** A new `env` module mirrors `settings` (store + schema + vars) but writes `~/.config/brain/env.json` and reuses `settings`'s render helpers. `paths::brain_root_path()` reads the `root` key from env.json (legacy `~/.config/brain-root` pointer as back-compat fallback). `markdown_to_pdf_path` moves out of `settings::VARS` into `env::VARS`, and the markdown-to-pdf gate reads/writes the env store. A one-time idempotent migration folds the legacy pointer into `env.root` and relocates `markdown_to_pdf_path` from `config.json` into `env.json`. `config.json`, personalization, extensions, and plugins do **not** move.

**Tech Stack:** Rust, `anyhow`, `serde`/`serde_json`, clap, the crate's pure-helper + thin-IO split, inline `#[cfg(test)]` unit tests. `cargo test --release` + `cargo clippy --release --all-targets`.

---

## Scope

Only phase **C1** of the [brain-sync spec](../specs/2026-07-24-brain-sync-design.md). No rclone, no `notify`, no `brain sync` command, no CSV merge, no triggers — those are C2–C5, each with its own plan. C1 is config plumbing + the `brain env` CLI, leaving the binary fully working.

## File Structure

| File | Responsibility | C1 change |
| --- | --- | --- |
| `src/paths.rs` | brain-root + machine-config-dir resolution | Add `machine_config_dir[_from]`; add `resolve_root` + `parse_root_key`; rewire `brain_root_path()` to read `env.json`'s `root` key then legacy pointer then default. |
| `src/env/mod.rs` (new) | brain-env module glue | Re-exports; module doc. |
| `src/env/store.rs` (new) | `env.json` location + read/write | `env_path()`, `load_map()`, `save_map()`. |
| `src/env/schema.rs` (new) | declared env `VARS` | `root`, `markdown_to_pdf_path`. |
| `src/env/vars.rs` (new) | env get/set/resolve | Reuses `settings::normalize_name`; builds `settings::Resolved`; `root` shows `brain_root_path()`. |
| `src/settings/mod.rs` | re-exports | `pub use schema::Resolved;` so `env` can render. |
| `src/settings/schema.rs` | config `VARS` | Remove `markdown_to_pdf_path` (now an env var). |
| `src/settings/markdown_pdf.rs` | md-to-pdf discovery/gate | Read/write via `crate::env` instead of `settings` get/set; messages say `brain env set`. |
| `src/cli.rs` | clap surface | Add `Env(EnvArgs)` + `EnvAction {List,Get,Set}`. |
| `src/main.rs` | startup + dispatch | `mod env;` `mod sync;`; `env_command`; dispatch `Env` before the gate; call `env::migrate()`. |
| `src/env/migrate.rs` (new) | one-time migration | Pure `plan` + thin `migrate()`. |
| `src/sync/mod.rs` + `src/sync/config.rs` (new) | parse-only `SyncConfig` | Reads the `sync` block from the env store. |
| `docs/*.md`, `README.md`, `AGENTS.md` | docs contract | brain-env vs brain-config nomenclature. |

---

### Task 1: `machine_config_dir()` — the fixed XDG path for `env.json`

**Files:**
- Modify: `src/paths.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/paths.rs`:

```rust
    #[test]
    fn machine_config_dir_prefers_xdg_config_home() {
        assert_eq!(
            machine_config_dir_from(Some("/xdg"), Path::new("/Users/x")),
            PathBuf::from("/xdg/brain")
        );
    }

    #[test]
    fn machine_config_dir_falls_back_to_home_dotconfig() {
        assert_eq!(
            machine_config_dir_from(None, Path::new("/Users/x")),
            PathBuf::from("/Users/x/.config/brain")
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --release paths:: 2>&1 | tail -20`
Expected: FAIL — `cannot find function machine_config_dir_from`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/paths.rs` (near `brain_root_file`):

```rust
/// The machine-local brain-env directory: `$XDG_CONFIG_HOME/brain` or
/// `~/.config/brain`. It holds `env.json` (brain env). Unlike the brain-internal
/// config dir it lives at a fixed `$HOME`-side path that does **not** depend on
/// the brain root, so it can hold `root` itself without circularity and never
/// rides the brain-dir sync.
#[must_use]
pub fn machine_config_dir() -> PathBuf {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty());
    let home = home_dir().unwrap_or_default();
    machine_config_dir_from(xdg.as_deref(), &home)
}

/// Pure core of [`machine_config_dir`]: `<xdg>/brain`, else `<home>/.config/brain`.
#[must_use]
pub fn machine_config_dir_from(xdg_config_home: Option<&str>, home: &Path) -> PathBuf {
    let base = xdg_config_home
        .filter(|s| !s.is_empty())
        .map_or_else(|| home.join(".config"), PathBuf::from);
    base.join("brain")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release paths:: 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/paths.rs
git commit -m "feat(env): add machine_config_dir (~/.config/brain) resolver"
```

---

### Task 2: The `env` store — `env.json` read/write

**Files:**
- Create: `src/env/store.rs`
- Create: `src/env/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing test**

Create `src/env/store.rs`:

```rust
//! The raw JSON brain-env store at `~/.config/brain/env.json`: locating it and
//! reading/writing the whole object. A broken or missing file never blocks
//! startup — it reads as an empty map. Brain env is machine-local (`root`,
//! `markdown_to_pdf_path`, the `sync` block) and is NOT Backblaze-synced.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Map, Value};

/// Absolute path to the brain-env JSON store.
#[must_use]
pub fn env_path() -> PathBuf {
    crate::paths::machine_config_dir().join("env.json")
}

/// Read the store as a JSON object. A missing/unreadable/non-object file yields
/// an empty map — a broken env never blocks startup.
#[must_use]
pub(crate) fn load_map() -> Map<String, Value> {
    std::fs::read_to_string(env_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

pub(super) fn save_map(map: &Map<String, Value>) -> Result<()> {
    let path = env_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_string_pretty(&Value::Object(map.clone()))?;
    std::fs::write(&path, format!("{body}\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_path_is_env_json_in_the_machine_config_dir() {
        let p = env_path();
        assert!(p.ends_with("brain/env.json"));
        assert_eq!(p.parent(), Some(crate::paths::machine_config_dir().as_path()));
    }
}
```

Create `src/env/mod.rs`:

```rust
//! Brain **env**: the machine-local config store at `~/.config/brain/env.json`,
//! managed by `brain env {list|get|set}`. Holds values that would be *wrong* on
//! another machine — `root`, `markdown_to_pdf_path`, and the Backblaze `sync`
//! block — so it is never Backblaze-synced (contrast `crate::settings`, the
//! brain **config** store that rides the brain-dir sync).

mod schema;
mod store;
mod vars;

pub use store::env_path;
pub use vars::{get, resolve_all, resolve_one, set};

pub(crate) use store::load_map;
```

> `schema` and `vars` are created in Task 4; this `mod.rs` will not compile until then. Add the `mod schema; mod vars;` lines and the `pub use vars::…` line in Task 4 — for Task 2 include only `mod store;` + `pub use store::env_path;` + `pub(crate) use store::load_map;`.

For Task 2, `src/env/mod.rs` is:

```rust
//! Brain **env**: the machine-local config store at `~/.config/brain/env.json`,
//! managed by `brain env {list|get|set}`. Holds values that would be *wrong* on
//! another machine — `root`, `markdown_to_pdf_path`, and the Backblaze `sync`
//! block — so it is never Backblaze-synced (contrast `crate::settings`, the
//! brain **config** store that rides the brain-dir sync).

mod store;

pub use store::env_path;

pub(crate) use store::load_map;
```

- [ ] **Step 2: Run test to verify it fails**

Register the module first — in `src/main.rs`, add `mod env;` (keep alphabetical: after `mod entry;`... actually after `mod cli;`/before `mod main_view;` — place it right after `mod cli;`). Then:

Run: `cargo test --release env::store 2>&1 | tail -20`
Expected: FAIL first with `unused` / then PASS shape — if it errors on `load_map` unused, that's fine; the test itself should compile and PASS. If clippy-style unused warnings block, proceed to Step 3.

- [ ] **Step 3: Write minimal implementation**

Already written in Step 1. Ensure `mod env;` is in `src/main.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release env::store 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/env/store.rs src/env/mod.rs src/main.rs
git commit -m "feat(env): brain-env JSON store at ~/.config/brain/env.json"
```

---

### Task 3: `root` resolution from `env.json` (+ `parse_root_key`, `resolve_root`)

**Files:**
- Modify: `src/paths.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/paths.rs`:

```rust
    #[test]
    fn resolve_root_prefers_the_env_key_over_the_legacy_pointer() {
        let home = Path::new("/Users/x");
        assert_eq!(
            resolve_root(Some("~/work-brain"), Some("~/old"), home),
            PathBuf::from("/Users/x/work-brain")
        );
    }

    #[test]
    fn resolve_root_falls_back_to_the_legacy_pointer_then_default() {
        let home = Path::new("/Users/x");
        assert_eq!(resolve_root(None, Some("/srv/brain"), home), PathBuf::from("/srv/brain"));
        assert_eq!(resolve_root(None, None, home), PathBuf::from("/Users/x/brain"));
    }

    #[test]
    fn parse_root_key_reads_the_string_field() {
        assert_eq!(parse_root_key(r#"{"root": "~/brain"}"#), Some("~/brain".to_owned()));
        assert_eq!(parse_root_key(r#"{"root": ""}"#), None);
        assert_eq!(parse_root_key(r#"{"markdown_to_pdf_path": "x"}"#), None);
        assert_eq!(parse_root_key("not json"), None);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --release paths:: 2>&1 | tail -20`
Expected: FAIL — `cannot find function resolve_root` / `parse_root_key`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/paths.rs` (`serde_json` is already a crate dependency, so call it fully-qualified — no new `use` line):

```rust
/// Pure brain-root precedence: the `root` env key, else the legacy
/// `~/.config/brain-root` pointer, else the `<home>/brain` default. Each
/// candidate is tilde-expanded against `home`.
#[must_use]
pub fn resolve_root(env_key: Option<&str>, legacy_pointer: Option<&str>, home: &Path) -> PathBuf {
    let pick = env_key
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| legacy_pointer.map(str::trim).filter(|s| !s.is_empty()));
    match pick {
        Some(raw) => expand_tilde_with_home(raw, home),
        None => home.join("brain"),
    }
}

/// Pull the `root` string field out of a raw `env.json` body. Pure: no IO.
/// A missing field, non-string value, empty string, or invalid JSON is `None`.
#[must_use]
pub fn parse_root_key(env_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(env_json)
        .ok()
        .as_ref()
        .and_then(|v| v.get("root"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
```

Now rewire `brain_root_path()` and add the env reader (replace the existing `brain_root_path` body):

```rust
#[must_use]
pub fn brain_root_path() -> PathBuf {
    let home = home_dir().unwrap_or_default();
    resolve_root(
        read_env_root().as_deref(),
        read_brain_root_file().as_deref(),
        &home,
    )
}

/// Read the `root` field from `~/.config/brain/env.json`, if any. A missing
/// file/field reads as `None` so resolution falls through to the legacy pointer.
fn read_env_root() -> Option<String> {
    std::fs::read_to_string(machine_config_dir().join("env.json"))
        .ok()
        .as_deref()
        .and_then(parse_root_key)
}
```

Update the module doc comment at the top of `src/paths.rs`: `root` is now the brain-env `root` key (`~/.config/brain/env.json`); the legacy `~/.config/brain-root` pointer is read only for back-compat.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release paths:: 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/paths.rs
git commit -m "feat(env): brain_root_path reads env.json root key, legacy pointer fallback"
```

---

### Task 4: env `schema` + `vars` + reuse `settings::Resolved`

**Files:**
- Create: `src/env/schema.rs`
- Create: `src/env/vars.rs`
- Modify: `src/env/mod.rs`
- Modify: `src/settings/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src/env/schema.rs`:

```rust
//! Declared brain-env variables: what `brain env list` prints, what
//! `brain env set` accepts, and their defaults.

pub(super) struct VarSpec {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) default: Option<&'static str>,
}

/// The brain-env schema, in `brain env list` order. `root` and
/// `markdown_to_pdf_path` are machine-local; the `sync` block is edited via
/// `brain sync setup` (C2), not raw `brain env set`.
pub(super) const VARS: [VarSpec; 2] = [
    VarSpec {
        name: "root",
        description: "Absolute or ~-relative path to the brain (PARA) directory on THIS machine. Defaults to ~/brain; a legacy ~/.config/brain-root pointer is migrated into this key.",
        default: Some("~/brain"),
    },
    VarSpec {
        name: "markdown_to_pdf_path",
        description: "Path to the markdown-to-pdf command on THIS machine. Auto-discovered on first run; required for the Create-PDF action.",
        default: None,
    },
];

pub(super) fn is_known(name: &str) -> bool {
    VARS.iter().any(|v| v.name == name)
}

pub(super) fn default_of(name: &str) -> Option<&'static str> {
    VARS.iter().find(|v| v.name == name).and_then(|v| v.default)
}

pub(super) fn known_names() -> String {
    VARS.iter().map(|v| v.name).collect::<Vec<_>>().join(", ")
}
```

Create `src/env/vars.rs`:

```rust
//! Reading and writing brain-env variables: get / set / resolve behind
//! `brain env`. Mirrors `settings::vars` but over the env store, and renders
//! into the shared `settings::Resolved` type.

use anyhow::{Result, bail};
use serde_json::Value;

use super::schema::{VARS, default_of, is_known, known_names};
use super::store::{load_map, save_map};
use crate::settings::Resolved;

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// The raw explicit value for `name` (no default fallback).
#[must_use]
pub fn get(name: &str) -> Option<String> {
    load_map().get(name).and_then(value_to_string)
}

/// The effective value for a known env variable: explicit override else default.
/// `root` resolves through [`crate::paths::brain_root_path`] so the shown value
/// matches what brain actually uses (including the legacy-pointer fallback).
#[must_use]
pub fn resolve_one(name: &str) -> Option<String> {
    if !is_known(name) {
        return None;
    }
    if name == "root" {
        return Some(crate::paths::brain_root_path().display().to_string());
    }
    get(name).or_else(|| default_of(name).map(str::to_owned))
}

/// Persist `name=value` into the env store. Unknown names are rejected.
pub fn set(name: &str, value: &str) -> Result<()> {
    if !is_known(name) {
        bail!("unknown env variable `{name}` (known: {})", known_names());
    }
    let mut map = load_map();
    map.insert(name.to_owned(), Value::from(value));
    save_map(&map)
}

/// Every declared env variable with its resolved value, in schema order.
#[must_use]
pub fn resolve_all() -> Vec<Resolved> {
    VARS.iter()
        .map(|v| Resolved {
            name: v.name,
            value: resolve_one(v.name),
            description: v.description,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_all_lists_root_and_markdown_to_pdf_path() {
        let rows = resolve_all();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.name == "root"));
        assert!(rows.iter().any(|r| r.name == "markdown_to_pdf_path"));
        // root always resolves (default ~/brain at minimum).
        assert!(rows.iter().find(|r| r.name == "root").unwrap().value.is_some());
    }

    #[test]
    fn set_rejects_unknown_env_variables() {
        assert!(set("linear_workspace", "acme").is_err());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Wire up the modules first. In `src/settings/mod.rs` add the re-export so `env` can name the type:

```rust
pub use schema::Resolved;
```

In `src/env/mod.rs`, add `mod schema;` and `mod vars;` and the `pub use vars::{get, resolve_all, resolve_one, set};` line (upgrade the Task-2 stub to the full version shown in Task 2 Step 1).

Run: `cargo test --release env::vars 2>&1 | tail -20`
Expected: FAIL — before adding the re-export/mods it won't compile (`Resolved` not found / modules missing).

- [ ] **Step 3: Write minimal implementation**

The code is in Step 1; the wiring is in Step 2. Confirm `settings::schema::Resolved` has `pub name/value/description` fields (it does) so `env::vars::resolve_all` can construct it.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release env:: 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/env/schema.rs src/env/vars.rs src/env/mod.rs src/settings/mod.rs
git commit -m "feat(env): brain-env schema + vars (root, markdown_to_pdf_path)"
```

---

### Task 5: The `brain env {list|get|set}` command

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing test**

`brain env` is an IO/CLI surface; assert its wiring with a headless smoke test rather than a unit test. Add this integration-style test at the bottom of `src/env/vars.rs` `tests` (it exercises the resolve path the CLI prints):

```rust
    #[test]
    fn root_row_reflects_the_resolved_brain_root() {
        let rows = resolve_all();
        let root = rows.iter().find(|r| r.name == "root").unwrap();
        assert_eq!(root.value.as_deref(), Some(crate::paths::brain_root_path().display().to_string().as_str()));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --release env::vars::tests::root_row 2>&1 | tail -20`
Expected: PASS already (resolve_all exists) — this test guards the value the CLI will print. If it compiles+passes, proceed; the CLI wiring itself is verified by the Step 4 smoke run.

- [ ] **Step 3: Write minimal implementation**

In `src/cli.rs`, add the `Env` command to the `Cmd` enum (beside `Config`):

```rust
    /// Read or change your machine-local brain env (`~/.config/brain/env.json`):
    /// `root`, `markdown_to_pdf_path`, and the Backblaze `sync` block.
    Env(EnvArgs),
```

And the arg/action types (beside `ConfigArgs`/`ConfigAction`):

```rust
#[derive(Args, Debug)]
pub struct EnvArgs {
    #[command(subcommand)]
    pub action: Option<EnvAction>,
}

#[derive(Subcommand, Debug)]
pub enum EnvAction {
    /// Print every env variable, its value, and its description as a table.
    List,
    /// Print the effective value of one env variable.
    Get {
        /// Variable name (e.g. `root`).
        name: String,
    },
    /// Set an env variable: `brain env set <name>=<value>`.
    Set {
        /// A single `name=value` assignment.
        assignment: String,
    },
}
```

In `src/main.rs`, import the new types (extend the `use crate::cli::{…}` line to include `EnvAction, EnvArgs`), dispatch `Env` before the prerequisite gate (right after the `Cmd::Config` block):

```rust
    // `brain env` manages the machine-local env store; like `config`, it runs
    // before the prerequisite gate so you can repair a broken environment.
    if let Some(Cmd::Env(args)) = &cli.command {
        return env_command(args);
    }
```

Add the handler (mirror `config_command`, reusing `settings`'s renderer):

```rust
/// Handle `brain env {list|get|set}`. Output goes to stdout; `get` on an unset
/// variable notes so on stderr. Bare `brain env` lists.
fn env_command(args: &crate::cli::EnvArgs) -> Result<()> {
    match args.action.as_ref().unwrap_or(&EnvAction::List) {
        EnvAction::List => {
            println!("{}", settings::render_list(&env::resolve_all(), settings::color_enabled()));
        }
        EnvAction::Get { name } => {
            let name = settings::normalize_name(name);
            match env::resolve_one(&name) {
                Some(v) => println!("{v}"),
                None => eprintln!("{name} is unset"),
            }
        }
        EnvAction::Set { assignment } => {
            if let Some((name, value)) = assignment.split_once('=') {
                let name = settings::normalize_name(name);
                env::set(&name, value)?;
                println!("{}", settings::set_confirmation(&name, value, settings::color_enabled()));
            } else {
                anyhow::bail!("expected `name=value`, got `{assignment}`");
            }
        }
    }
    Ok(())
}
```

Add the `unreachable!` arm in the final `match cli.command` (beside the others):

```rust
        Some(Cmd::Env(_)) => unreachable!("env is dispatched before the gate"),
```

- [ ] **Step 4: Run test + smoke the CLI**

Run: `cargo build --release && ./target/release/brain env list && echo "---" && ./target/release/brain env get root`
Expected: a table with `root` + `markdown_to_pdf_path`; `get root` prints the resolved brain root path.

Also confirm set works into env.json:

Run: `./target/release/brain env set markdown_to_pdf_path=/tmp/mdpdf && ./target/release/brain env get markdown_to_pdf_path`
Expected: prints `/tmp/mdpdf`; the value lands in `~/.config/brain/env.json` (not `config.json`). Undo with `./target/release/brain env set markdown_to_pdf_path=` only if it was previously unset, else restore the prior value.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs src/env/vars.rs
git commit -m "feat(env): brain env {list|get|set} command"
```

---

### Task 6: Move `markdown_to_pdf_path` from brain config to brain env

**Files:**
- Modify: `src/settings/schema.rs`
- Modify: `src/settings/markdown_pdf.rs`

- [ ] **Step 1: Write the failing test**

In `src/settings/markdown_pdf.rs` tests, the gate reads/writes the path. Add a test asserting the *config* store no longer declares it and the message points at `brain env`. First, in `src/settings/vars.rs` tests (or schema tests), add:

```rust
    #[test]
    fn markdown_to_pdf_path_is_no_longer_a_brain_config_variable() {
        // It moved to brain env; `brain config` must reject it.
        assert!(resolve_all_from(&Map::new()).iter().all(|r| r.name != "markdown_to_pdf_path"));
        assert!(set("markdown_to_pdf_path", "/x").is_err());
    }
```

And update the existing markdown_pdf message test (search for `brain config set markdown_to_pdf_path=`): change the expected substring to `brain env set markdown_to_pdf_path=`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --release 2>&1 | tail -25`
Expected: FAIL — `markdown_to_pdf_path` still in `settings::VARS` (so `set` succeeds and the row exists), and the gate message still says `brain config`.

- [ ] **Step 3: Write minimal implementation**

In `src/settings/schema.rs`, remove the `markdown_to_pdf_path` `VarSpec` entry and drop the array length by one (`VARS: [VarSpec; 7]`).

In `src/settings/markdown_pdf.rs`, switch the two store touchpoints from the config store to the env store, and fix the messages:

- Replace `use ...` for `get`/`set` (currently `super::vars::{get, set}` or similar) so the module calls `crate::env::get("markdown_to_pdf_path")` and `crate::env::set("markdown_to_pdf_path", …)`. (Grep the file for `get("markdown_to_pdf_path")` at line ~87 and `set("markdown_to_pdf_path", …)` at line ~165.)
- In the two user-facing messages (lines ~90 and ~121), change `brain config set markdown_to_pdf_path=<path>` to `brain env set markdown_to_pdf_path=<path>`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release 2>&1 | tail -25`
Expected: PASS. Confirm the gate still self-heals against the env store (the `gate_*` tests in markdown_pdf.rs pass).

- [ ] **Step 5: Commit**

```bash
git add src/settings/schema.rs src/settings/markdown_pdf.rs src/settings/vars.rs
git commit -m "feat(env): markdown_to_pdf_path is a brain-env var (moved from brain config)"
```

---

### Task 7: Parse-only `SyncConfig` schema (reads the env `sync` block)

**Files:**
- Create: `src/sync/mod.rs`
- Create: `src/sync/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing test**

Create `src/sync/config.rs`:

```rust
//! Typed, parse-only view of the `sync` block in `~/.config/brain/env.json`.
//!
//! C1 only *parses* this — no rclone, no transfers, no triggers. C2+ reads these
//! values to drive Backblaze sync. All fields are optional; an absent block ⇒
//! sync disabled and brain behaves exactly as before.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    pub enabled: bool,
    pub b2_bucket: String,
    pub b2_path: String,
    pub b2_key_id: String,
    pub b2_app_key: String,
    #[serde(default = "default_true")]
    pub on_start: bool,
    #[serde(default = "default_true")]
    pub on_exit: bool,
    #[serde(default = "default_true")]
    pub watch: bool,
    #[serde(default = "default_max_delete")]
    pub max_delete_percent: u8,
}

fn default_true() -> bool {
    true
}
fn default_max_delete() -> u8 {
    50
}

impl SyncConfig {
    /// Load the `sync` block from the brain-env store; defaults when absent.
    #[must_use]
    pub fn load() -> Self {
        crate::env::load_map()
            .get("sync")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    /// True when sync is switched on AND a bucket is configured.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.enabled && !self.b2_bucket.trim().is_empty()
    }

    /// Effective watcher state: on by default whenever sync is configured,
    /// unless explicitly disabled via `watch=false`.
    #[must_use]
    pub fn watch_effective(&self) -> bool {
        self.is_configured() && self.watch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> SyncConfig {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn absent_fields_default_and_disable_sync() {
        let c = parse("{}");
        assert!(!c.enabled && !c.is_configured() && !c.watch_effective());
        assert_eq!(c.max_delete_percent, 50);
        assert!(c.on_start && c.on_exit && c.watch);
    }

    #[test]
    fn configured_requires_enabled_and_a_bucket() {
        assert!(!parse(r#"{"enabled": true}"#).is_configured());
        assert!(!parse(r#"{"b2_bucket": "b"}"#).is_configured());
        assert!(parse(r#"{"enabled": true, "b2_bucket": "b"}"#).is_configured());
    }

    #[test]
    fn watch_defaults_on_when_configured_and_off_when_disabled() {
        assert!(parse(r#"{"enabled": true, "b2_bucket": "b"}"#).watch_effective());
        assert!(!parse(r#"{"enabled": true, "b2_bucket": "b", "watch": false}"#).watch_effective());
    }
}
```

Create `src/sync/mod.rs`:

```rust
//! Backblaze B2 cross-machine sync (Sub-project C). C1 ships only the parse-only
//! config schema; transport (rclone bisync), the CSV merge, triggers, and skill
//! integration land in C2–C5.

pub mod config;

pub use config::SyncConfig;
```

- [ ] **Step 2: Run test to verify it fails**

Add `mod sync;` to `src/main.rs` (after `mod state;`, before `mod tasks;`). Then:

Run: `cargo test --release sync::config 2>&1 | tail -20`
Expected: FAIL first if `mod sync;` missing (won't compile); after adding it, PASS.

- [ ] **Step 3: Write minimal implementation**

Code is in Step 1; ensure `mod sync;` is declared in `src/main.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release sync::config 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/sync/ src/main.rs
git commit -m "feat(sync): parse-only SyncConfig schema reading the env sync block"
```

---

### Task 8: One-time migration — pointer→`env.root`, config→env `markdown_to_pdf_path`

**Files:**
- Create: `src/env/migrate.rs`
- Modify: `src/env/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing test**

Create `src/env/migrate.rs` with the pure planner + tests:

```rust
//! One-time, idempotent migration into brain env: fold the legacy
//! `~/.config/brain-root` pointer into the `root` key, and relocate
//! `markdown_to_pdf_path` from brain config (`config.json`) into brain env.
//! Never fatal — a failed migration must not block startup.

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Plan {
    /// Write this value into the env `root` key (from the legacy pointer).
    pub(super) set_root: Option<String>,
    /// Write this value into the env `markdown_to_pdf_path` (from brain config).
    pub(super) set_md_pdf: Option<String>,
    /// Remove `markdown_to_pdf_path` from the brain-config store after moving it.
    pub(super) clear_config_md_pdf: bool,
}

/// Decide the migration plan. Pure: no IO.
pub(super) fn plan(
    env_has_root: bool,
    legacy_pointer: Option<&str>,
    env_has_md_pdf: bool,
    config_md_pdf: Option<&str>,
) -> Plan {
    let non_empty = |s: &str| -> Option<String> {
        let t = s.trim();
        (!t.is_empty()).then(|| t.to_owned())
    };
    Plan {
        set_root: (!env_has_root).then_some(legacy_pointer).flatten().and_then(non_empty),
        set_md_pdf: (!env_has_md_pdf).then_some(config_md_pdf).flatten().and_then(non_empty),
        clear_config_md_pdf: config_md_pdf.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_pointer_into_root_only_when_env_root_missing() {
        assert_eq!(plan(false, Some("/srv/brain"), true, None).set_root, Some("/srv/brain".to_owned()));
        assert_eq!(plan(true, Some("/srv/brain"), true, None).set_root, None);
        assert_eq!(plan(false, Some("  "), true, None).set_root, None);
        assert_eq!(plan(false, None, true, None).set_root, None);
    }

    #[test]
    fn relocates_md_pdf_only_when_env_lacks_it_and_config_has_it() {
        let p = plan(true, None, false, Some("/opt/mdpdf"));
        assert_eq!(p.set_md_pdf, Some("/opt/mdpdf".to_owned()));
        assert!(p.clear_config_md_pdf);
        // Already in env → don't overwrite, but still clear the stale config copy.
        let p = plan(true, None, true, Some("/opt/mdpdf"));
        assert_eq!(p.set_md_pdf, None);
        assert!(p.clear_config_md_pdf);
        // Nothing in config → nothing to do.
        assert!(!plan(true, None, false, None).clear_config_md_pdf);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Add `mod migrate;` to `src/env/mod.rs` and `pub use migrate::migrate;`. Then:

Run: `cargo test --release env::migrate 2>&1 | tail -20`
Expected: FAIL first (module referenced but `migrate()` not defined yet — add the thin shell in Step 3), then PASS for the pure `plan` tests.

- [ ] **Step 3: Write minimal implementation**

Append the thin IO shell to `src/env/migrate.rs`:

```rust
/// Run the one-time migration. Idempotent; swallows IO errors (never fatal).
pub fn migrate() {
    let env_map = super::load_map();
    let env_has_root = env_map.get("root").and_then(serde_json::Value::as_str).is_some_and(|s| !s.trim().is_empty());
    let env_has_md_pdf = env_map.get("markdown_to_pdf_path").and_then(serde_json::Value::as_str).is_some_and(|s| !s.trim().is_empty());

    let legacy_pointer = std::fs::read_to_string(brain_root_pointer_path())
        .ok()
        .and_then(|s| crate::paths::parse_brain_root_file(&s));

    let config_md_pdf = crate::settings::config_get("markdown_to_pdf_path");

    let p = plan(env_has_root, legacy_pointer.as_deref(), env_has_md_pdf, config_md_pdf.as_deref());

    if let Some(root) = p.set_root {
        let _ = super::set("root", &root);
    }
    if let Some(md) = p.set_md_pdf {
        let _ = super::set("markdown_to_pdf_path", &md);
    }
    if p.clear_config_md_pdf {
        let _ = crate::settings::config_remove("markdown_to_pdf_path");
    }
}

/// `$XDG_CONFIG_HOME/brain-root` or `~/.config/brain-root` (legacy pointer).
fn brain_root_pointer_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map_or_else(
            || std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
            std::path::PathBuf::from,
        );
    base.join("brain-root")
}
```

This needs two small helpers on the brain-config store. Add to `src/settings/mod.rs`:

```rust
pub use vars::{config_get, config_remove};
```

Add to `src/settings/vars.rs`:

```rust
/// Read a raw brain-config value (no default). Used by the env migration to find
/// a stale `markdown_to_pdf_path` to relocate.
#[must_use]
pub fn config_get(name: &str) -> Option<String> {
    get(name)
}

/// Remove a key from the brain-config store. Used by the env migration after
/// relocating a value into brain env. Absent key ⇒ no-op.
pub fn config_remove(name: &str) -> Result<()> {
    let mut map = load_map();
    if map.remove(name).is_some() {
        save_map(&map)?;
    }
    Ok(())
}
```

Call the migration early in `src/main.rs`, right after `personalization::init_tag_styles();`:

```rust
    // One-time, idempotent migration into brain env (fold the brain-root pointer
    // into env.root; relocate markdown_to_pdf_path from brain config). Never fatal.
    env::migrate();
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release 2>&1 | tail -20`
Expected: PASS. Smoke:

Run: `cargo build --release && ./target/release/brain env list`
Expected: `root` shows the resolved brain root; if a legacy `~/.config/brain-root` existed, `~/.config/brain/env.json` now has a `root` key (check with `cat ~/.config/brain/env.json`).

- [ ] **Step 5: Commit**

```bash
git add src/env/migrate.rs src/env/mod.rs src/settings/mod.rs src/settings/vars.rs src/main.rs
git commit -m "feat(env): one-time migration (pointer→env.root, config md_pdf→env)"
```

---

### Task 9: Full green + clippy, then docs & README

**Files:**
- Modify: `docs/config.md`, `docs/data-model.md`, `docs/decisions.md`, `docs/features.md`
- Modify: `README.md`
- Modify: `AGENTS.md` (docs-contract table)

- [ ] **Step 1: Run the full suite + lint**

Run: `cargo test --release 2>&1 | tail -15 && cargo clippy --release --all-targets 2>&1 | tail -15`
Expected: all tests PASS; clippy clean. Fix any fallout before docs.

- [ ] **Step 2: `docs/config.md`**

Rewrite the config section around the **brain env vs brain config** split:
- brain env → `~/.config/brain/env.json`, `brain env {list|get|set}`, machine-local, NOT Backblaze-synced: `root`, `markdown_to_pdf_path`, the `sync` block.
- brain config → `<brain-root>/.config/config.json`, `brain config {list|get|set}`, Backblaze-synced: `linear_workspace`, `daily_triage_name_pattern`, `day_rollover_hour`, `agenda_dir`, `calendar_id`, `claude_cmd`, `skills_auto_sync`.
- `root` is now an env key; the `~/.config/brain-root` pointer is legacy (read-only back-compat, auto-migrated).
- The `sync` block is parse-only in C1; behavior lands in C2+.

- [ ] **Step 3: `docs/data-model.md`**

Add the two-store schema and the `sync` block (fields, defaults, `is_configured`/`watch_effective`) mirroring `src/sync/config.rs` and `src/env/schema.rs`.

- [ ] **Step 4: `docs/decisions.md`**

Add a decision entry: brain env / brain config split; the "wrong-if-synced ⇒ env" rule of thumb; the partial reversal of "config in the brain root" and why (C makes leaking machine-level secrets/paths the dominant risk); the residual jpsyx mirror-write footgun for `env.json` (jpsyx-side seed/copy, not symlink), per spec §12.

- [ ] **Step 5: `docs/features.md` + `README.md`**

- `docs/features.md`: add `brain env {list|get|set}` beside `brain config`; note `markdown_to_pdf_path` is now an env var.
- `README.md`: add the brain-env / brain-config nomenclature and which values live where; update any `brain config set markdown_to_pdf_path` references to `brain env set markdown_to_pdf_path`.

- [ ] **Step 6: `AGENTS.md` docs-contract table**

Update the config/root row: `root` is a brain-env key in `~/.config/brain/env.json` (legacy pointer migrated in), `markdown_to_pdf_path` is a brain-env value, and add the brain-env-vs-brain-config split + the `brain env` command + the `sync` block to the relevant rows (pointing at `src/env/`, `src/sync/`).

- [ ] **Step 7: Commit**

```bash
git add docs/config.md docs/data-model.md docs/decisions.md docs/features.md README.md AGENTS.md
git commit -m "docs: brain env vs brain config nomenclature + sync schema (C1)"
```

---

## Self-Review

**Spec coverage (C1 slice):**
- Spec §3 brain env at `~/.config/brain/env.json`, `brain env` CLI → Tasks 2, 4, 5.
- Spec §3 `root` becomes an env key; pointer deprecated + back-compat + migration → Tasks 3, 8.
- Spec §3 `markdown_to_pdf_path` moves to brain env → Tasks 6, 8.
- Spec §3 brain config (`config.json`) stays put + synced → unchanged by design (Tasks 6/8 only remove `markdown_to_pdf_path`).
- Spec §3.1 `brain env {list|get|set}` mirroring `brain config` → Task 5.
- Spec §9 `sync` block parse-only schema (in env) → Task 7.
- Spec §13 C1 docs + README nomenclature → Task 9.
- Out of C1 scope: rclone, `notify`, `brain sync`, CSV merge, triggers, skill rows — absent here (C2–C5).

**Placeholder scan:** No TBD/TODO. Every code step shows complete code. Task 2's `mod.rs` has an explicit "for Task 2 use this reduced version" block so the module compiles before Task 4 adds `schema`/`vars`. Task 6 references existing line numbers (~87/~121/~165) as *grep anchors*, with the exact string replacements given.

**Type consistency:** `machine_config_dir[_from]`, `resolve_root`, `parse_root_key`, `env::{env_path, load_map, get, set, resolve_one, resolve_all}`, `settings::{Resolved, render_list, set_confirmation, color_enabled, normalize_name, config_get, config_remove}`, `SyncConfig::{load, is_configured, watch_effective}`, `env::migrate` / `migrate::plan` are used with identical names/signatures across tasks. `settings::Resolved` is re-exported in Task 4 so `env::vars` can construct it. `env::migrate` calls `settings::config_get`/`config_remove`, both added in Task 8.

**Ordering:** Execute in order. Task 4 depends on Task 3 (`brain_root_path` reads env) and the `settings::Resolved` export; Task 5 depends on Task 4; Task 8 depends on Tasks 3–7 (`env::set`, `settings::config_*`). Task 2's `mod.rs` stub is upgraded in Task 4 — don't skip that swap.

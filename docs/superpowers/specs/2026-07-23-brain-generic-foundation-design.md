# Brain: making it generic — Program overview + Sub-project A spec

- **Date:** 2026-07-23
- **Status:** Design — approved forks captured; ready for implementation planning of Sub-project A.
- **Scope of this document:** the program-level decomposition (A/B/C) plus the
  **full design for Sub-project A**. Sub-projects B and C each get their own
  spec when we brainstorm them.

---

## 1. Why

`brain` today is simultaneously (a) a public-intent Rust CLI + PARA/task
management system and (b) Pablo's personal setup, with personal data baked into
the binary and with the brain-related skills living in a private dotfiles repo.
The goal is to make the **repo 100% public and generic**
— anyone can clone `brain`, get the brain-related skills automatically, and
customize/extend without forking — while Pablo, as the primary user, **loses no
functionality or personalization** on his own machines.

Four user goals drove this:

1. Brain is the full PARA management system, not just a CLI; its location must
   be configurable (default `~/brain`).
2. The brain-related skills (todo, second-brain, triage, tasks, email-triage,
   zotero, …) should ship *with* brain yet remain **global and updateable** so
   they work in *any* Claude session, without manual install.
3. Personal/irrelevant bits in those skills (stocks-assistant hook, zotero,
   "CEO of Avandar") must become **configurable personalization/plugins**, not
   committed content. Skills read a config and/or are re-rendered with the
   user's values.
4. The brain *contents* should sync across the user's machines (local-first,
   optional Backblaze cloud), independent of big-tech (no Dropbox).

## 2. Decomposition (A → B → C)

Each is a separate spec → plan → build cycle.

- **A — The personalization & config foundation (this spec).** The store,
  `brain personalize`, first-run onboarding, plus the root-authority and
  code-depersonalization fixes that are its first consumers. Everything
  downstream reads/renders against this, so it exists first.
- **B — Brain skill pipeline: bundle + render + config + plugins.** brain owns
  its core skills as templates; a `brain skills` command renders them against
  personalization + user plugins and installs them into the global skill
  registry (updateable on brain update). Depersonalize the skills; move Pablo's
  personal bits into his private config/plugins.
- **C — Brain sync: local change ledger + Backblaze cross-machine sync.**
  Watcher + append-only ledger of every filesystem mutation, a B2 backend,
  `brain sync` push/pull, multi-machine reconciliation. Standalone; depends on
  nothing in B.

## 3. Cross-cutting invariants (apply to A, B, and C)

These are laws for the whole program, not just A.

1. **The repo is 100% public and generic.** No personal identity, taxonomy, org
   names, path assumptions, or personal skill dependencies committed anywhere.
2. **No-regression migration (hard law).** Every personal element removed from
   the repo to make it generic is *simultaneously* re-added to Pablo's **local**
   personalization/config/plugins, so his machines behave exactly as they do
   today. By the end of the program: public repo **and** zero personal loss.
   Every sub-project's plan includes a "migration" step that populates Pablo's
   local values for whatever it extracted.
3. **brain never writes the user's private dotfiles repo.** The dotfiles manager
   stops *owning* the brain skills but still *syncs* them: its update/sync also
   invokes brain's skill install/sync (detail lands in B). The private dotfiles
   repo remains free to adapt around brain.
4. **Two-store lifecycle seam.** Machine-local settings never sync; portable
   content-about-you does. (Concretized in A, §A.2.)
5. **Any config/personalize mutation re-renders skills.** Every `brain
   personalize set`, `brain config set`, and first-run onboarding calls a single
   `resync_skills()` hook after persisting. A defines the hook (no-op/warn); B
   fills in the real render pipeline. Never fatal.

---

# Sub-project A — The personalization & config foundation

## A.1 Scope / non-goals

**In scope:** the personalization store; the `brain personalize` command;
skippable first-run onboarding; depersonalizing the hardcoded tag taxonomy;
routing the remaining `$HOME/brain` bypasses through the configured root; the
`resync_skills()` seam; and defining (not yet applying) the skill-consumption
contract.

**Out of scope (deferred):** the actual skill render/install pipeline (B); moving
skills into the repo and depersonalizing skill *text* (B); the `mark_done.py`
CLI↔skill path coupling fix (B — flagged here); all sync (C).

## A.2 Two stores, two lifecycles

| Store | Path | Holds | Synced by C? |
|---|---|---|---|
| Machine-local config | `~/.config/brain/config.json` (XDG-aware, unchanged) | `root`, `claude_cmd`, `markdown_to_pdf_path`, runtime knobs | **No** |
| Personalization | `~/brain/.config/personalization.json` (under `root`) | identity + content-about-you | **Yes** — travels with the brain dir |

- The personalization store lives in a **hidden** `.config/` dir *inside the
  brain root* so it (a) syncs for free when C syncs the brain dir, (b) stays out
  of Finder (dot-prefixed), and (c) is already skipped by the picker's
  `collect()` (hidden-file skip). C's ledger will later share `~/brain/.config/`.
- Resolved relative to the configured `root` (via `paths::brain_root()`), never
  hardcoded to `$HOME/brain`.

## A.3 Personalization schema v1

```json
{
  "name": "",
  "role": "",
  "works_for": "",
  "tag_styles": {
    "ceo": { "label": "CEO", "emoji": "🕴" }
  }
}
```

- `name` — optional display name.
- `role` — free text ("CEO", "software engineer", "student", "PhD researcher").
  This is the personalized *who*, distinct from the generic *rule* "act as a
  personal assistant". The rule stays in the skill; the who/for-whom is
  personalized.
- `works_for` — org name, "myself", or empty.
- `tag_styles` — map of `tag → { label, emoji }`, replacing the hardcoded
  taxonomy in `src/tasks/render/style.rs`.
- All fields optional; a missing file or field falls back to generic defaults.
  Unknown top-level keys are ignored (forward-compat with B/C additions).
- The schema is intentionally small and additive; B and C append fields.

## A.4 Tag styles

- **Shipped generic defaults:** exactly `mit` (Most Important Task — a generic
  productivity concept), `personal`, and `work`, each with a tasteful emoji.
- **Fallback:** an unknown tag renders as its raw tag name, no emoji (optionally
  title-cased). No panics, no personal tags in the binary.
- **Override:** users add/replace styles in `personalization.json` (see A.5).
- **B follow-up:** when the `/todo` skill is examined in B, verify whether it
  semantically depends on any specific tag beyond `mit`; if so, promote those to
  generic defaults there.

## A.5 The `brain personalize` command

| Invocation | Effect |
|---|---|
| `brain personalize` | Onboarding if the store is empty/absent; else behaves like `show`. |
| `brain personalize show` | Print current personalization in a stable, Claude-readable keyed block (this is the runtime-lookup target skills reference). |
| `brain personalize get <field>` | Print one field's effective value (explicit or default). |
| `brain personalize set <field>=<value>` | Set, persist, **then call `resync_skills()`**. Unknown fields rejected. |
| `brain personalize edit` | Open `personalization.json` in `$EDITOR` (the path for editing `tag_styles`); on save, `resync_skills()`. |

- Field-name normalization mirrors `brain config` (lowercase, `-`→`_`).
- Nested `tag_styles` edited via `edit` in v1 (raw JSON); a dedicated
  `personalize tag set …` subcommand is a possible later nicety, not required.
- Like `brain config`, this command is exempt from the `markdown-to-pdf`
  startup gate so you can always fix your setup.

## A.6 First-run onboarding

- **Trigger:** first `brain` startup when `personalization.json` is absent/empty.
- **UX:** a short, **skippable** interactive prompt on `/dev/tty` — three
  questions (name, role, who you work for), Enter skips any. Writes the store,
  then runs the initial `resync_skills()` once.
- **Non-blocking:** brain works fully with empty personalization; skills just
  fall back to generic behavior until set.
- Re-runnable anytime via `brain personalize`.

## A.7 Root-authority fixes

Route these three `$HOME/brain` bypasses through `paths::brain_root()` (the
configured root):

1. `src/tasks/complete.rs:53` — resolve the brain dir via `brain_root()`, not
   `home.join("brain")`.
2. The `brain tasks` CSV default (`src/tasks/cli.rs`) — derive from
   `brain_root()` instead of a literal `~/brain/tasks/…`.
3. `src/tui/app_actions/commands.rs:230` — the prompt string sent to Claude
   interpolates the resolved root instead of the literal `~/brain/tasks/tasks.csv`
   and `~/brain/projects`.

**Flagged, not fixed here:** `complete.rs` execs `mark_done.py` from
`~/global-skills/todo/scripts/` — a hard CLI dependency on an installed skill at
a fixed path. This entanglement is the reason B exists; B formalizes it (bundled
skills + a resolved skills path). A leaves it as-is beyond noting it.

## A.8 The `resync_skills()` seam

- A single internal hook called after every personalization/config mutation and
  after onboarding.
- **In A:** a no-op that logs a one-line notice to stderr (or invokes an
  existing `brain skills sync` if one is present), never fatal — a render failure
  must not block a `config set`.
- **In B:** becomes the real render/install pipeline.

## A.9 Skill-consumption contract (defined here, applied in B)

- `brain personalize show` emits a **compact, stable, keyed block** suitable for
  a skill to read verbatim (exact format fixed in A so B can rely on it).
- Identity-dependent skills carry a one-line **standard preamble** pointing at
  that command/output ("Before acting as a personal assistant, load the user's
  brain personalization via `brain personalize show`…").
- A only *defines* the format and the preamble text; B wires the preamble into
  the actual skills.

## A.10 Migration (Pablo, no regression)

As part of A, populate Pablo's **local** `~/brain/.config/personalization.json`
so his machine is unchanged:

- `role: "CEO"`, `works_for: "Avandar"` (and `name`).
- `tag_styles`: re-add **all** tags currently hardcoded in `style.rs`
  (`ceo`, `aa`, `mit`, `finance`, `languages`, `code`, `personal`, `needs_attention`,
  and any others found in the full match) with their existing labels/emojis, so
  task rendering is byte-for-byte the same.

The repo ships only the generic defaults (§A.4); Pablo's extras live only in his
local store. Acceptance requires diffing his rendered task view before/after to
confirm no visual change.

## A.11 Testing (pure-function first, per house rules)

- personalization parse / merge-with-defaults / empty-is-unset.
- tag-style resolution incl. unknown-tag fallback.
- `personalize get`/`set` field normalization + value coercion (mirror
  `settings/vars` tests).
- `personalize show` block formatting (stable output).
- root interpolation into the `commands.rs` prompt (pure builder).
- `resync_skills()` is called on mutation (seam invoked; behavior stubbed).

## A.12 Docs to update (same change)

- `docs/config.md` — the two-store model; personalization vs machine-local.
- New personalization section (schema, command, onboarding, skill contract).
- `docs/features.md` — `brain personalize`, onboarding, tag-style config.
- `docs/data-model.md` — personalization schema.
- `docs/decisions.md` — two-store seam, hybrid model, auto-resync, `.config/`
  location choice.
- The docs-contract table in `AGENTS.md`/`CLAUDE.md` — add personalization rows.

## A.13 Acceptance criteria

1. `~/brain/.config/personalization.json` is the read/written store; hidden in
   Finder; skipped by the picker.
2. `brain personalize {show,get,set,edit}` work; `set`/`edit`/onboarding call
   `resync_skills()`.
3. First run shows the skippable prompt; brain runs fine when skipped.
4. The binary contains **no** personal tags; defaults are `mit`/`personal`/`work`
   with graceful fallback.
5. `complete.rs`, the tasks-CSV default, and the `commands.rs` prompt honor the
   configured `root`.
6. Pablo's local store reproduces his exact prior tag rendering and identity
   (no visual/functional change).
7. `cargo test --release` green; `cargo clippy --release --all-targets` clean.

## A.14 Deferred to B / C (explicit)

- **B:** move skills into the repo; depersonalize skill text; the render/install
  pipeline (`resync_skills()` real body); plugins/extensions; per-skill config
  (e.g. email-triage per-sender rules, the stocks-assistant hook, zotero); the
  `mark_done.py` coupling fix; the `<dotfiles> sync → brain skill sync` bridge.
- **C:** the change ledger, watcher, Backblaze backend, `brain sync`, and
  multi-machine reconciliation (sharing `~/brain/.config/`).

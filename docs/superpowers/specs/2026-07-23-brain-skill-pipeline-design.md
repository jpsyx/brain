# Sub-project B: brain skill pipeline — bundle + render + extensions + plugins

- **Date:** 2026-07-23
- **Status:** Design — program-level scope + phase plan approved via brainstorm;
  each phase (B1–B4) gets its own detailed spec before implementation.
- **Depends on:** Sub-project A (personalization store, `resync_skills()` seam).
- **Program context:** see `2026-07-23-brain-generic-foundation-design.md` for the
  A/B/C decomposition and the cross-cutting invariants (public repo; no-regression
  migration; brain never writes the user's private dotfiles repo; two-store seam;
  mutations re-render).

## B.1 Goal

Make the brain-related skills ship *with* the brain repo yet stay **global and
updateable** — installed into the shared agent skill registry so they work in any
Claude session, re-rendered on every personalization/config change, and
customizable per-user **without forking the repo**. Depersonalize the skills so
the repo is 100% generic, while Pablo loses nothing (his personal behavior moves
into his own extensions/plugins).

## B.2 Install model (decided)

`brain skills sync` renders the bundled skills (+ the user's extensions/plugins)
and installs them into the **same shared agent registry a symlink-based dotfiles
manager already fans out from**:

```
brain skills sync
  render (base skill + user extensions, identity via runtime lookup)
     └─▶ ~/.agents/skills/<name>          (brain-owned entries)
            └─▶ ~/.claude/skills, ~/.codex/skills, ~/.config/opencode/skills, ...
```

- **Public cloner:** `brain skills sync` (auto-run by `resync_skills()` and on
  first run) does everything; no dotfiles manager required.
- **Existing dotfiles-manager user:** the manager stops *owning* the brain
  skills. Its update/sync also invokes `brain skills sync`; its prune step is
  fixed so it never deletes brain-owned skills (ownership boundary in B4). brain
  never writes the private dotfiles repo.
- **Fan-out target set** is configurable (which frontends), defaulting to the
  installed ones, mirroring a dotfiles manager's behavior. **Codex is a required target**
  alongside Claude: even though the brain CLI only drives Claude, a user may open
  Codex separately and call a brain skill, so `brain skills sync` must install
  into Codex's skills location too (symlink, or whatever config Codex requires to
  register skills), plus the other installed frontends (OpenCode, Cursor, …).

## B.2a Build decisions (decided)

- **Embedding:** bundled skills are compiled into the binary with the
  `include_dir` crate (from the repo's `skills/` dir: `SKILL.md` + all scripts),
  so a public cloner needs no repo checkout — `brain skills sync` writes them out.
  One new dependency, justified in `docs/architecture.md`.
- **Live-env safety:** during B1–B3 the pipeline runs only against a **sandbox
  install root**; Pablo's real `~/.agents`/frontend dirs are untouched until the
  B4 cutover (see B.8).

## B.3 Bundle scope (decided)

**Bundled (public, generic):**

| Skill | Notes |
|---|---|
| `todo` | Task system; ships with its scripts (incl. `mark_done.py`, which the CLI execs — resolves the A-flagged coupling). |
| `second-brain` | PARA knowledge mgmt — **minus** "how to summarize" (→ `article-summarizer`) and **minus** zotero-sync (→ personal plugin). |
| `triage` | **Core = the plain 2-page agenda PDF**, as before email-triage existed. The email-triage call + custom `~/Downloads` PDF are removed from core. |
| `brain-knowledge-capture` | Core (but currently hardcodes "a busy CEO" and `~/global-skills/...` paths — needs B3 depersonalization). |
| `article-summarizer` | **New bundled skill:** the generic "how to summarize an article" logic, extracted from second-brain. Referenced by second-brain (summarizing into resources) and by the personal zotero-sync plugin. |

**Not bundled (personal → Pablo's plugins/extensions, never committed):**

- `email-triage` → Pablo's **plugin**.
- `habits` → Pablo's **plugin** (not a generic core): its body is a thin wrapper
  around his personal `~/scripts/zshrc/habits/control.py`, so there is no generic
  skill to bundle. Reclassified from the initial bundle set.
- `zotero-article-summary` → becomes Pablo's **`zotero-sync` plugin**: upload/sync
  docs, zotero↔brain-resources sync, and all zotero collection rules (priorities,
  read status, etc.). The "how to summarize" part does **not** live here — it
  references the bundled `article-summarizer`.

## B.4 Two customization mechanisms

Both are user-authored, stored with the brain (synced, personal), never committed
to the public repo.

### Plugins — whole user skills

A plugin is a complete skill the user owns, installed alongside the bundled cores
by the same pipeline. Storage: `~/brain/.config/plugins/<name>/SKILL.md` (+
scripts). Pablo's plugins: `email-triage`, `zotero-sync`.

### Extensions — additive personalization of a bundled skill

An extension changes a *bundled* skill's behavior without creating a new skill.
It is **rendered into the built/installed copy only** — the repo's core skill
source is **never** modified. The pipeline reads the pristine bundled skill,
injects the user's extension, and writes the result to the install target that
Claude/Codex read; the repo stays generic and untouched. (Structural change, per
A's hybrid model — identity stays a runtime lookup; behavior changes are
rendered.) Storage: `~/brain/.config/extensions/<skill-name>.md`.

Pablo's extensions:
- `triage`: at the start, call the `email-triage` plugin; generate the final
  agenda PDF into `~/Downloads` the custom way. (Without this extension, core
  triage just makes the plain 2-page PDF.)
- `second-brain`: when/how to call `zotero-sync` so resources sync both ways.

**Injection design (to be finalized in B3):** base skills declare a small set of
named extension points (e.g. `<!-- brain:ext triage:start -->`,
`<!-- brain:ext triage:agenda-pdf -->`) plus an always-available trailing
"Personal extensions" section. The renderer substitutes the user's extension
content at the matching points and appends the rest. Named points are what let an
extension run "at the start". If a skill has no extension file, it renders
unchanged (fully generic).

## B.5 Rendering model

- **Identity** (name/role/works_for) — runtime lookup via `brain personalize show`
  (from A). Base skills carry the standard preamble; not rendered per-user.
- **Extensions** — rendered/injected at install time (structural).
- **Plugins** — installed as-is (their own SKILL.md), no rendering needed beyond
  the identity preamble.
- `resync_skills()` (the A seam) gains its real body here: re-render + re-install
  on every `personalize`/`config` mutation, onboarding, and `brain skills sync`.

## B.6 Phase plan (reordered so no phase regresses Pablo)

Mechanism before migration, so depersonalizing a core and restoring Pablo's
behavior happen together (no window where he loses functionality).

- **B1 — Pipeline + fan-out.** Build `brain skills sync`: render step (byte
  passthrough for a skill with no extension), install into `~/.agents/skills`,
  fan out to the frontends (Claude, **Codex**, OpenCode, Cursor). Wire
  `resync_skills()` to it, gated OFF by default during B1–B3 so it never touches
  the live registry (see B.8). **Install root + frontend set are configurable**
  so dev/tests run against a temp sandbox. **Pilot skill:** no existing skill is
  drop-in generic (each hardcodes personal bits), so B1 pilots on either a
  from-scratch minimal generic skill **or** `brain-knowledge-capture`
  depersonalized as part of B1 (folding a little B3 in to validate the full
  chain on a real skill). Decide at B1 kickoff.
- **B2 — Extension + plugin mechanism.** Extension points + injection; plugin
  discovery + install. Testable with a synthetic extension/plugin on `habits`.
- **B3 — Migrate + depersonalize the cores** (todo, second-brain, triage,
  brain-knowledge-capture) into the repo, extract `article-summarizer`, and — in
  the same phase — move each removed personal bit into Pablo's extension/plugin
  files, so nothing regresses. Split second-brain (summarize→article-summarizer,
  zotero→zotero-sync plugin) and triage (email-triage/custom-PDF→triage extension).
- **B4 — dotfiles-manager bridge + cutover.** The dotfiles manager delegates to
  `brain skills sync`; fix its prune to spare brain-owned skills; flip the live
  registry over from dotfiles-manager-owned to brain-owned; final no-regression
  verification.

Each phase: its own spec → plan → RED/GREEN build → docs, and ships green.

## B.7 Migration (Pablo, no regression) — carried across phases

- `email-triage` and `zotero-sync` become Pablo's plugins under
  `~/brain/.config/plugins/`.
- `triage` and `second-brain` extensions under `~/brain/.config/extensions/`
  restore his email-triage-first + custom-PDF and zotero-sync behavior.
- Acceptance: after B4, Pablo's installed skills behave byte-for-byte as before
  (same triage PDF flow, same zotero sync, same email triage), while the repo
  contains only generic skills.

## B.8 Safety (live-environment)

Installing/symlinking into `~/.agents/skills` and the frontend dirs mutates
Pablo's **live** working setup and can collide with the dotfiles manager (which
currently owns these skills). Therefore:

- The install root/target set is a parameter; **all dev and tests run against a
  temp sandbox**, never the real registry.
- **No cutover of Pablo's live registry happens before B4**, when the
  dotfiles-manager ownership boundary + prune fix are in place. Until then brain's pipeline is
  proven in the sandbox only.
- Pure logic (render, link-plan computation, prune-safety classification) is
  unit-tested per the house pure/impure rule; the symlink/FS shell stays thin.

## B.9 Open questions (resolved per-phase, not now)

- Exact extension-point syntax + renderer (B2).
- Plugin manifest/format and how scripts are installed/pathed (B2/B3).
- Dotfiles-manager prune ownership marker (a manifest of brain-owned names? a
  marker file?) (B4).

---
id: BR-3
title: Replace Notion & Linear MCPs with bundled in-house CLIs, gated by brain env
status: todo
priority: medium
assignee: jpsyx
labels: [feature, tech-debt]
estimate: 13
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-03
updated: 2026-08-03
---

# BR-3: Replace Notion & Linear MCPs with bundled in-house CLIs, gated by brain env

## Description

Today, any triage or second-brain workflow that touches Notion or Linear
depends on external MCP servers being connected in the agent frontend. That
makes the integration implicit, unversioned, and impossible for brain itself
to reason about: brain can't tell whether Notion/Linear are even reachable, so
it can't cleanly skip the steps that need them.

Replace that dependency with **in-house Notion and Linear CLIs** that brain
bundles and drives directly for every action we actually use (the specific
Notion/Linear operations invoked by triage and second-brain, not the full
API surface). Credentials for both services move into **brain env** — API
tokens and/or OAuth for Notion and Linear — so there is one machine-local,
brain-owned place to configure them.

With config in brain env, brain gains a first-class **capability check**: if
no Notion (or Linear) configuration is present in brain env, brain can
**automatically skip** the corresponding Notion/Linear steps in triage and
second-brain instead of failing or hanging on a missing MCP. When configured,
the same steps run against the in-house CLI.

Net wins: no external MCP requirement, explicit versioned integration brain
controls, one config surface (brain env), and graceful degradation when a
service isn't set up on this machine.

## Acceptance criteria

- [ ] In-house Notion CLI covers every Notion action triage / second-brain
      currently rely on (enumerate the real call sites first).
- [ ] In-house Linear CLI covers every Linear action triage / second-brain
      currently rely on (enumerate the real call sites first).
- [ ] Neither the Notion MCP nor the Linear MCP is required for any brain
      workflow; the CLIs are the path.
- [ ] Notion and Linear credentials (API token and/or OAuth) are configurable
      via `brain env` (new schema rows), with tokens marked sensitive.
- [ ] Brain exposes a capability check: "is Notion configured?" / "is Linear
      configured?" derived from brain env.
- [ ] Triage and second-brain **automatically skip** their Notion/Linear steps
      when the respective config is absent, and run them when present — no
      error, no hang.
- [ ] Both LLM frontends (Claude and Codex) get equivalent behavior.
- [ ] Docs updated per the docs/ contract (env schema, features, integrations,
      and the affected skill surfaces).

## Notes

Open design questions to resolve during planning (do not pre-decide here):

- **Where do the CLIs live?** As new brain subcommands (`brain notion …`,
  `brain linear …`), separate bundled binaries, or standalone external tools
  brain shells out to. Bundling as brain subcommands keeps one config surface
  and one install, but weigh binary size / dependency growth (the dep set is
  small on purpose).
- **Bundled-vs-personal boundary.** The bundled `triage` / `second-brain`
  skills must stay generic and carry no personal data (guard test
  `bundled_skills_carry_no_personal_data`). Notion/Linear *destinations* are
  personal; today they live in the user's brain **extensions**
  (`triage:daily-linear`, `triage:weekly-linear`, and second-brain routing),
  not in core. The capability-gating mechanism can live in core (generic:
  "skip if unconfigured"), while the specific databases/teams stay in the
  extension. Keep this split intact.
- **OAuth vs. token.** Linear supports personal API keys and OAuth; Notion
  uses integration tokens / OAuth. Decide which brain env supports first
  (a static token is far simpler than an OAuth flow and may be enough).

### Pointers (as of 2026-08-03)

- `src/env/schema.rs` — the `VARS` array is the declared brain-env schema
  (currently 11 rows). Add `notion_*` / `linear_*` rows here following the
  existing `resend_*` / `twilio_*` pattern; mark tokens sensitive in
  `is_sensitive()`. This is also where a capability check would read from.
- `src/env/vars.rs` — `get` / `resolve_one` / `resolve_all` read env values;
  the `claude_command` / `codex_command` helpers show the "typed accessor over
  a raw env row" pattern to mirror for a `notion_configured()` /
  `linear_configured()` helper.
- `src/env/{store,migrate}.rs` — env store on disk (`~/.config/brain/env.json`)
  and migrations; new rows may need a migration entry.
- `skills/triage/SKILL.md` — the generic bundled triage skill. Its
  `brain:ext triage:daily-linear` / `triage:weekly-linear` markers (lines
  ~122, ~449) are the hook points where personal Linear behavior is injected;
  the skip-if-unconfigured gate wraps these.
- `skills/second-brain/SKILL.md` — bundled second-brain skill; Notion-related
  routing is personalized via extension, gate it the same way.
- `~/brain/.config/extensions/{triage,second-brain}.md` — the user's personal
  extensions that today carry the actual Notion/Linear destinations and steps;
  the personal half of the wiring lives here, not in core.
- `docs/config.md` + `docs/data-model.md` — env schema + `brain env` docs to
  update when adding rows (docs/ contract).
- `docs/integrations.md` + `docs/features.md` — where a new integration
  (in-house CLIs, capability-based skipping) and any new subcommand must be
  documented.
- `.claude/CLAUDE.md` (AGENTS.md) house rules — "CLI flag/subcommand for every
  action", both-frontends rule, docs/ contract, and the small-dependency-set
  rule all bear on how the CLIs are structured.

### Log

- 2026-08-03 created.
- 2026-08-03 triaged: priority `medium`, estimate `13`, moved to `todo`. No
  duplicates found. Left unassigned to a project (none exist yet) and no cycle
  (no active cycle). Estimate is deliberately at the top of the scale — this is
  large enough to warrant splitting into a project (Notion CLI, Linear CLI,
  env schema + capability gate, skill wiring) if/when it's pulled into a cycle.

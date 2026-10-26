# Brain workspace instructions and focused skills

Date: 2026-10-25
Status: Approved on 2026-10-25. Implementation is in progress.
Baseline: `b04a64c7`, Brain `0.105.3`.
Feature migration boundary: `0.106.0`. Later implementation commits increment
the patch version under the repository's commit policy.

## Outcome

Every Brain task receives a concise workspace contract. Detailed workflows load
only when needed. Task management and agenda planning have separate owners.
Upgrading Brain automatically reconciles workspace instructions, bundled skills,
legacy extension hooks, and capability selections. One channel policy controls
interactive, SMS, and email responses across every skill and registered frontend.

This changes the instructions installed into a **knowledge workspace**. It does
not replace this software repository's contributor `AGENTS.md`.

## Review and scope

The [baseline review](../../investigations/2026-10-25-brain-skills-review.md)
records existing contracts, incompatibilities, and regression cases. The current
umbrella contains 1,250 lines; `/todo` contains 1,773. Their size is evidence of
mixed responsibilities, not a target for arbitrary line-count reductions.

The work includes the instruction lifecycle, content split, compatibility,
launch integration, channel presentation, tests, and corresponding documentation.
It does not replace the task store, agenda renderer, sync transport, receiver
outbox, authentication, or skill-session completion protocol.

## Approach selection

| Approach | Benefit | Cost / decision |
| --- | --- | --- |
| Managed region in workspace `AGENTS.md`, focused skills, compatibility routes | Native instruction discovery; preserves custom text; supports automatic updates | Requires region-aware ownership and coordinated publication. **Selected.** |
| Entirely generated `AGENTS.md` plus a separate personal file | Simple generated ownership | Changes the established user-ownership contract and risks displacing custom rules. Rejected. |
| Keep a mandatory, smaller `second-brain` loader | Small migration | Keeps shared rules behind skill activation and does not satisfy the requested architecture. Compatibility use only. |

## Global constraints

- Feature migration boundary is `0.106.0`; managed compatibility revision is `2`.
- Every production behavior change starts with a failing test observed before implementation.
- No new dependency and no unsafe Rust are required.
- Every registered frontend participates: Claude, Codex, OpenCode, and pi.
- Channels are exactly `interactive`, `sms`, and `email`, selected from `ActorContext`.
- SMS final replies contain at most 480 Unicode characters after Markdown removal.
- Core instructions contain no personal schedule, personal habit ID, private integration, or extension-specific output path.
- An absent extension is a correct no-op; extensions and plugins remain user-owned sources.
- Help and version are side-effect free; ordinary commands reconcile before dispatch.
- Missing registered roots remain pending and are never invented by this migration.
- Unknown user bytes, modified managed content, and filename collisions are never silently overwritten.
- Publication, recovery, and downgrade must leave a coherent instruction-and-skill generation before ordinary dispatch.
- One failed workspace does not prevent attempts on the other registered workspaces.
- No real SMS, email, or external integration is invoked by verification.

## 1. Instruction and skill ownership

### Always-present workspace contract

The managed region contains workspace/actor resolution, assistant purpose, a
short PARA map, common naming rules and exceptions, canonical data ownership,
capture protection, archive-first behavior, link repair, relevant neighbor
search, cleanup, and a concise skill routing table. Resolve the acting person
from the launched actor rather than assuming the machine's default person.

Keep shared rules concise. Detailed schemas, worked examples, project lifecycle
procedures, and agenda scheduling rules belong in focused references. Define
shared output conventions through the central response policy described below.

| Skill | Owns | Does not own |
| --- | --- | --- |
| `projects` | Project creation, metadata, completion, IP extraction, archiving, task/project consistency | Daily scheduling or general inbox processing |
| `process-capture` | Inspect every inbox item, regroup by idea, route notes and actions, account for failures | Alternate filing rules or alternate task mutations |
| `organize` | PARA placement, resource/media ingestion, note/media pairing, retrieval, cross-links, local reindex decisions | Cloud synchronization or project lifecycle |
| `brain-sync` | Cloud sync, conflict inspection, semantic resolution, verification | Local lookup rebuilding disguised as cloud sync |
| `todo` | Task/habit CRUD, assignment, recurrence, chunks, backlog, canonical mutation commands | Agenda composition and time-block placement |
| `agenda` | Date selection, scheduling inputs, capacity, anchors, calendar occupancy, agenda generation/update/rendering | A second implementation of task mutation rules |
| `triage` | Daily and weekly review orchestration; managed triage completion | Duplicate capture or agenda procedures |
| `contacts` | Deterministic contact lookup and maintenance | General workspace policy or channel selection |
| Knowledge capture | Distill durable conversation knowledge and provenance | A second PARA classification procedure |
| Article summarizer | Faithful source distillation and its document structure | Implicitly saving every summary or choosing a channel |
| `brain-skills` | Workspace plugin/extension authoring and syncing | Editing generated copies as persistent sources |
| `second-brain` | Compatibility routing for existing invocations and links | Always-required policy or duplicated workflows |

Knowledge capture retains the name `brain-knowledge-capture`; article summarizer
retains `article-summarizer`. Use focused `references/` files where an operation
requires substantial detail.
Each reference has one canonical owner. Calling another skill is warranted only
when the requested operation actually needs that skill. For example, reading a
contact does not load project or agenda procedures.

When moving contradictory existing text, use these normative decisions:

- Project slugs follow native namespace validation. Resource summaries paired
  with indexed media use colocated `notes.md` and canonical `Summary`/`Notes`
  headings; arbitrary standalone notes keep descriptive names.
- An explicit filing, processing, moving, or renaming request authorizes routine
  placement decisions. Ask only for unresolved consequential choices, destructive
  disposition, or a new configured namespace not already authorized by the request.
- Use native project/task/habit commands, including keyed managed-triage
  completion. Native skip/defer, chronic detection, and agenda synchronization
  behavior supersede stale prose. Hard-deadline review precedes group mutation.
- Preserve explicit triage maintenance exceptions and all daily/weekly hooks;
  chaining into daily triage enters the whole daily procedure. Plain capture
  processing does not complete a weekly habit.
- Use frontend-neutral question instructions. Contact memory helps choose the
  query but does not replace the native lookup. An existing article abstract is
  source material, not an exception to canonical saved-summary structure.

### Agenda behavior

Preserve date handling, actual task/habit inputs, chunk scheduling, deadline and
capacity checks, deliberately unfilled time, existing-agenda updates, automatic
task-state synchronization, PDF generation, and the existing agenda document
structure. Resolve Markdown and PDF destinations from the selected workspace's
configuration. The current `/tmp/<date>.md` prose must not override
`agenda_markdown_dir`.

Daily anchors, personal scheduling windows, and optional integrations come from
explicit workspace data or extension hooks. Remove the existing hard-coded
personal timetable and habit identifiers from generic core. Do not copy those
defaults into every workspace during migration. Explicit user customizations
remain active through the extension mapping below. This is an intentional
correction to the existing generic-core violation, not a new scheduling policy.

`/todo agenda`, `/todo plan-day`, and existing natural-language agenda requests
remain compatible through a short handoff to `/agenda`. `/todo today`, `/todo
what`, `/todo next`, and task-count requests keep their task-list/recommendation
semantics and do not create an agenda or PDF. First-party prompts, including
Generate agenda, invoke `/agenda` directly. Task listing through the native CLI
does not start generating an agenda merely because the skill was split.

## 2. Central response policy

Author channel rules once under `templates/workspace/response/`. A pure
`response_policy` module selects the matching template and owns the SMS limit.
The workspace contract's response section is generated from these sources;
`AgentController` also supplies the selected policy as trusted launch context.
There is no per-skill SMS/email branch and no channel inference from the user's
message text, quoted transcript, or attachment.

| Channel | Presentation contract |
| --- | --- |
| Interactive | Readable Markdown, structured lookup/task results, usable file references, and the existing final changes/additions table when workspace material changes |
| SMS | Short plain-text answer, requested facts first, no Markdown tables, no filesystem paths or local-file links, no additions table, full web URLs when useful |
| Email | Self-contained answer with supporting content, appropriate headings/tables, safe rendered HTML plus readable plain text, no local paths or local-file links |

Skills describe **what facts or artifacts to produce**. They may provide an
artifact template, but they do not dictate a terminal-only final reply. Contact
field requirements remain in `contacts`; presentation and channel overrides do
not. An agenda document can retain its full structure even when the answer sent
to the user is a concise SMS. Generating a file never implies permission to send
it somewhere or widen delivery recipients.

Keep deterministic delivery shaping at the existing reply boundary. Share the
480-character constant with prompt policy. Preserve Markdown stripping, safe
email HTML, threading, frozen authorized recipients, durable delivery and
retry behavior. Produce a readable email plain-text alternative through the same
central boundary. Do not add another LLM formatting pass or require every skill
to emit a new JSON protocol. The long-SMS fallback must make no unconditional
promise to email when email delivery is unavailable.

No-local-path behavior is an instruction contract with regression examples;
generic prose is not passed through an unsafe blanket path-deleting regex.
Existing HTML URL restrictions continue to block unsafe clickable links. Tests
must distinguish deterministic delivery guarantees from model-facing behavior.

### Launch integration

Add a frontend-neutral instruction payload separate from `AccessPolicy`.
Access restrictions must keep their meaning; unrestricted mode still needs
workspace and channel instructions. Before spawning, the controller loads the
reconciled workspace document, composes its shared rules with the selected
response policy and existing access/capability boundary, then passes the same
payload to each adapter's supported instruction mechanism.
Keep user-authored document text identified as workspace guidance; it does not
become authority to change access, actor, channel, or recipients. Trusted access
and channel selection come only from the resolved request and remain distinct
from the document bytes. A quoted instruction cannot select another policy.

Use the current Claude/pi append-system-prompt mechanisms, Codex developer
instructions, and OpenCode Brain-agent prompt. Cover fresh launches, resumes,
receiver reconstruction/recovery, manual tabs, and skill sessions. Do not depend
on unverified native `AGENTS.md` discovery, particularly for pi. Do not remove
global frontend instructions or change lifecycle/completion metadata.

Bound the composed instruction payload and account for it in the existing
shell-command budget. Preserve receiver message, transcript and attachment
budget guarantees. Oversize instructions fail before spawning with an actionable
diagnostic; silently truncating user rules is not acceptable.

The renderer adds one short shared prelude to installed Brain skills: resolve
the selected workspace and read its `AGENTS.md` if it has not already been
loaded. This covers explicitly invoked skills outside the root. The prelude
contains no channel branches. It does not install Brain policy globally.

## 3. Extension and plugin compatibility

Migrate **effective hook destinations**, while retaining original extension and
plugin source bytes. A versioned pure compatibility map is used by ordinary
rendering, actor capability rendering, installer preflight/apply, and downgrade.
This avoids attempting to rewrite arbitrary user prose or scripts.

| Legacy extension hook | Canonical destination |
| --- | --- |
| `second-brain:company-context` | Shared workspace instructions, `workspace:context` |
| `second-brain:reference-manager` | `organize:reference-manager` |
| `todo:anchors` | `agenda:anchors` |
| `todo:calendar` | `agenda:calendar` |
| `todo:cutoff` | `agenda:cutoff` |
| `todo:agenda-after-build` | `agenda:after-build` |
| `todo:linear-backlog`, `todo:linear` | Remain in `todo` |
| `contacts:fallback` | Remains in `contacts` |
| All existing `triage:*` hooks | Remain in `triage` at the same semantic phase |

New authors use `.config/extensions/workspace.md`, `agenda.md`, and the focused
skill's extension file. Legacy file names and hook IDs remain valid aliases.
Each legacy block reaches its new owner exactly once. Identical legacy and
canonical definitions are deduplicated; distinct nonempty definitions of the
same logical hook are a reported conflict, with the previous generation retained.

Legacy `second-brain` catch-all and unknown-hook content was effectively global;
preserve it in the shared contract's legacy-customization section. Legacy `todo`
catch-all and unknown-hook content may combine task and agenda preferences;
preserve it as one logical generated reference, packaged with both `todo` and
`agenda` so an agenda-only capability selection is self-contained. Both copies
carry the same content identity and an instruction to reuse the already-loaded
context across a handoff, without repeating completed side effects. Load it only
when nonempty. Do not guess its domain from keywords.
Other skills retain existing unmatched-hook behavior. Do not silently drop a
block because its original skill body became a router.

Compatibility includes old relative skill links and documented heading anchors.
Keep small forwarding reference/anchor sections under old paths, pointing to
the new owner. Retain `/second-brain reindex`, `cloud-sync`, `resolve-conflicts`,
project and capture handoffs, and `/todo` agenda handoffs. First-party callers
use canonical names. Arbitrary plugin text is preserved; compatibility routes
carry its old invocations. A user plugin colliding with a new bundled skill name
is reported before publication; it is never overwritten or silently renamed.

Compatibility remains for this release. Future removal requires a separate
explicit migration and review; there is no automatic time-based deletion.

## 4. Automatic workspace migration

### Managed region and ownership

Use exactly one region delimited by:

```text
<!-- brain:managed-workspace:start schema=1 -->
...generated workspace contract...
<!-- brain:managed-workspace:end -->
```

The region is owned; text outside it remains user-owned. The receipt records the
region digest separately from unowned prefix/suffix bytes. Currentness therefore
does not reject a legitimate edit outside the region. Publication still checks
the entire observed before-image to detect a concurrent edit.

Adopt an exact historical template by replacing it with the managed contract.
In a customized document, remove only complete byte-identical recognized legacy
template sections, preserve all other bytes, and add the managed region. Preserve
unknown instructions that still mention `second-brain`; the compatibility router
keeps them usable. Do not infer authorship from headings alone. Retain a verified
local backup before the first adoption, including the original absence state.

Reconcile absent documents and absent owned regions. A modified owned region,
malformed/duplicate markers, unexpected symlink, or conflicting file type is an
ownership conflict. A region synchronized from another machine can be adopted
with a missing or stale local receipt only when its complete generated bytes match
a supported render for the observed sources. A sync can deliver rules and sources in separate
steps: defer classification while that verified pull is incomplete, then retry
under the sync boundary. Do not label a partial delivery a user edit, or wait
indefinitely once the pull completed. No self-declared digest proves ownership.

Persist a UUID-scoped receive-in-progress record before a verified pull can
change portable instructions or sources; clear it only after receive completion
and reconciliation. The completed-run journal alone cannot prove an interrupted
receive. On restart, an explicit `brain sync --pull` for that root can complete
the interrupted receive through a recovery-only startup phase before instruction
reconciliation. Run other required prerequisites first; do not open ordinary
dispatch, agent launches, or a push while the package is incoherent. Retry once
under the existing sync lock, then reconcile the complete package and finish the
normal migration checks. Failed transport remains recoverable; a mismatch after
a completed receive is a conflict. This route must not authorize overwriting
arbitrary modified AGENTS text merely because reconciliation failed.

Preserve an existing `CLAUDE.md -> AGENTS.md` link. Leave independent user-owned
frontend documents intact; Brain launches receive the canonical instructions
explicitly, and installed skills carry the common read-AGENTS prelude.

### One coherent publication

Extend the existing managed installation transaction with typed artifacts for
the AGENTS region and supported configuration-field patches. Do not permit
arbitrary workspace paths in the transaction. A package includes skill trees,
frontend links, instructions, and capability migration. Its journal
authenticates all before/after states. Advance the compatibility
receipt only after the whole package is installed and verified.

Upgrade the managed manifest/journal schema with readers for the previous
schema. Recover an existing interrupted installation before adopting the new
schema. A pre-commit interruption restores the prior complete generation;
a post-commit interruption finishes verification, stamping, and cleanup.
Post-crash unexpected edits preserve evidence and report a conflict.

Use the existing workspace sync exclusion boundary while publishing portable
AGENTS/config changes, then the managed-install lock. Preserve the installer's
existing removal-operation-before-task lock ordering where those locks are
needed. Never acquire the sync lock after an install lock. Handle contention
within the existing bounded startup policy and do not stamp a busy root current.

### Ordering and entry points

The historical `0.102.25` migration already renders the **running** bundle. A new
migration appended after it would run too late. Compatibility preparation must
therefore run inside the shared refresh coordinator, before the first source
render/currentness decision. Its single `RefreshAttempt` result must include
the complete workspace package. Register `up`/`down` in a focused
`startup_migration/brain_skills.rs` module without creating a second refresh.

Cover ordinary startup, explicit selected/all-workspace sync, sandbox sync,
create, attach, repair, installer transitions, and actor-scoped capability
rendering. Preserve fresh-root initialization order: managed AGENTS must not
cause an otherwise empty workspace to skip PARA/task seeding. Nonmanifested
sandboxes receive instructions and skills without inventing portable metadata.

Named native `env set agent_capabilities` and `agent_capabilities.skills` corrections
use the same one-shot setup boundary under ordinary invocation ownership. Validate
the candidate declarations and selected identity before saving, then require
mandatory reconciliation before success. Unrelated env commands retain ordinary
ordering. Read-only receipt/pending observation has bounded memory and stable
private WAL-aware copies, without imposing a whole-database size ceiling.

### Capability names

Preserve logical access when a skill is decomposed. For explicitly selected
`second-brain`, retain the compatibility name and add `projects`,
`process-capture`, `organize`, and `brain-sync`. For `todo`, retain it and add
`agenda`. Preserve order deterministically and deduplicate. An empty allowlist
stays empty. Do not add unrelated skills or external connectors. Update default
selections for newly created workspaces. Compatibility shims and composed
workflows respect unavailable capabilities rather than bypassing access policy.

Persist the migrated logical list as a narrow field patch, preserving unrelated
config. Record before/after list values for reversible migration. Existing
plugin-owned capability names are not reclassified as bundled without proof.
Do not broaden a canonical-only selection during down: for example, `projects`
alone cannot become the broader old `second-brain`. Preflight refuses a selection
the target cannot represent at equivalent scope. A missing config field uses the
new default without unnecessarily writing that field to disk.

### Cross-machine safety

AGENTS and portable settings synchronize, whereas generated skill trees do not.
The workspace manifest is also excluded from bisync; raising its local minimum
would not fence an existing peer. This refactor does not introduce a new remote
manifest protocol or raise an existing workspace's `minimum_brain_version`.
Keep the portable instruction contract backward compatible instead: its routing
table names the canonical skill and an explicit legacy fallback when that skill
is not installed (`second-brain` for its decomposed operations; `todo` for agenda).
The fallback is conditional on trusted capability availability, not a filename
existence check or an always-required umbrella. A same-named user plugin is not
silently treated as a bundled successor.

On the new binary, recheck instruction/compatibility source revisions after a
sync pull and before a new agent launch. Project generated skills locally through
the shared coordinator. Accept only known compatible managed-region revisions;
never downgrade unknown newer instructions. Preserve custom text received through
sync. Old binaries and already-running old agent sessions do not acquire new
features retroactively; regression cases demonstrate the legacy fallback for the
existing operations. Canonical new hook names require the new renderer until an
explicit downgrade writes their legacy projection as described below.
An unchanged valid package keeps the existing no-write fast path. Ordinary
optional extension edits still respect `skills_auto_sync`; mandatory source
inspection occurs for a package/version transition, missing managed artifact,
received changed managed region, or explicit skill sync. Do not turn every
startup into an unconditional customization rebuild.

Never change the workspace UUID, receiver ingress UUID, or an independently
required minimum version. Backups and transaction journals remain machine-local
and never synchronize. This choice avoids claiming a local floor is a distributed
release fence.

Missing roots stay pending and retry after becoming available, including when
the machine version stamp is already current. Validating one root never falls
back to another. Newer unsupported workspace revisions fail clearly before writes.

### Downgrade and installer

The current installer owns preflight and downgrade. Add an audited `0.105.3`
target profile alongside the already-supported `0.101.0` and exact current
profile. Preflight must model the target instructions, extension projection,
capabilities and managed-install manifest schema in the private replica, not
only collect target skill trees. It must not mutate real sources to make its
simulation work. Apply publishes the exact validated target package.

Both upgrade and downgrade must run migration and complete package apply before
executable replacement. The existing Rust helper owns finalization under one
exclusive machine invocation guard through installed/candidate digest revalidation,
atomic rename and final verification. Ordinary commands retain shared ownership
through their full lifetime, including waiting work and TUIs; contention refuses
immediately. After shared acquisition, compiled-versus-disk executable validation
refuses stale processes loaded before replacement. Help/version remain side-effect
free, and hidden maintenance acquires protection without an environment bypass.
Conservative bounded no-follow historical TUI/server evidence also refuses an
active transition. Do not terminate processes or remove their evidence; request
that Brain sessions close and active commands finish before retry.

Protected finalization starts at coordinator 0.106.14. Modern-split helpers from
0.106.0 through 0.106.13 must refuse downgrade before live changes and print an
exact current-installer-first command. Upgrade to current remains automatic with
the new helper. Historical pre-split fallback retains historical behavior;
already-running uninstrumented historical ordinary CLI processes remain limited
to their existing workspace locks. Keep the existing sync, operation/task, install
lock order. No persistent transition ticket, second installer, dependency or unsafe
code is needed.

Down restores the old complete skill generation and compatible instructions,
removes only owned split outputs, retains user extras and all extension/plugin
sources, and exports receipts/manifests readable by the target. Restore only
migration-owned config/list changes; preserve later unrelated user edits.
If a later user edit cannot be reversed without ambiguity, refuse downgrade
before binary replacement and retain the current coherent state. Never replay a
whole stale config backup over current user settings.

Canonical new extension hooks authored after upgrade must project back into the
legacy hook locations for supported target rendering. Persist those narrowly
scoped legacy-source additions during down, in the same journaled package, so a
later ordinary old-version skill sync does not lose them. Preserve original
source bytes and before-images; reject conflicting duplicate definitions before
publication. Retain the canonical new files as well. Re-upgrade deduplicates exact
equivalent projections using provenance. Legacy-only downgrade leaves sources
unchanged. New plugins and unrelated custom files remain present even if an older
binary cannot select them. An existing workspace floor incompatible with the
target remains an honest downgrade refusal, not a reason to lower it silently.

Retain provenance for each projected legacy block: original source bytes/hash,
canonical source identity, inserted span, and expected post-image. If an older
client subsequently edits that block, re-upgrade preserves the edit and treats
different canonical/legacy definitions as a conflict; it never restores a stale
backup over the edit. All affected registered roots must be available for an
explicit downgrade. Refuse before replacement if one is unavailable, since the
old target cannot execute this new migration automatically when it remounts.
Ordinary **upgrades** continue to allow unavailable roots as pending.

## 5. Validation and release criteria

The implementation plan assigns red/green cycles to these obligations:

1. Pure content/hook projection, alias coverage, source preservation, and conflicts.
2. Whole-document adoption, exact-section migration, user suffix edits, deletion
   repair, malformed markers, links/types, and cross-machine adoption.
3. Fault injection before/after every meaningful publication and receipt boundary.
4. Upgrade, downgrade, re-upgrade, same-version repair, all-root enumeration,
   unavailable roots, disabled optional sync, initial setup, and installer replicas.
5. Capability selection retains its previous scope and every advertised route has
   an installed target or an explicit unavailable result.
6. Four-frontends by three-channels by fresh/resume launch coverage, including
   receiver recovery, access modes, shell quoting, and bounded prompts.
7. SMS and email delivery normalization with fake/local providers only.
8. Skill scenarios: task-only, agenda-only, mixed capture, project completion/IPs,
   contacts by name/role, notes/media, sync conflicts, daily/weekly triage, and
   existing extensions/plugins invoking old names.
9. Static relative-link/anchor integrity and an independent behavioral review of
   representative rendered skills. Text presence alone does not prove behavior.
10. `cargo fmt --check`, `cargo test --release`,
    `cargo clippy --release --all-targets -- -D warnings`, existing personal-data
    guard, and the relevant installer/shell checks.

Update architecture, features, integrations, config, data model, decisions,
testing, glossary, and keybinding docs where the existing contracts require it.
There is no keyboard binding change; agenda-related wording and command ownership
must nevertheless agree across prompts, palette documentation, and skills.

## Review boundary

The high-level split was approved in conversation. This written design adds the
concrete ownership, compatibility, and migration decisions needed for safe
execution. The accompanying implementation plan is a reviewable draft. Product
implementation follows the written-spec review required by the requested
Superpowers workflow.

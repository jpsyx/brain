# Core Brain skills: pre-refactor review

Date: 2026-10-25. Baseline: `b04a64c7`, version `0.105.3`.
Method: static inspection of all seven bundled skill entrypoints, six supporting
references, the workspace template, and relevant implementation/caller contracts.
Independent reviews covered workflow preservation and migration safety.
No product code, installed skills, live workspace, or external service was changed.
No claim of a passing implementation test suite is made by this document.

## Assessment

Proceed with the focused-skill design, subject to its migration and compatibility
tests. A text-only split would break existing workspaces. The important risks are
instruction ownership, extension reachability, capability allowlists, and installer
ordering. The existing managed transaction, native mutation commands, and durable
receiver delivery provide useful foundations to preserve.

| Current source | Responsibility / size |
| --- | --- |
| `skills/second-brain/SKILL.md` | Shared policy plus organizing/projects/capture/sync; 1,250 lines |
| `skills/todo/SKILL.md` | Task/habit management plus substantial agenda planning; 1,773 lines |
| `skills/triage/SKILL.md` | Review orchestration; 726 lines |
| `skills/contacts/SKILL.md` | Contact book; 180 lines |
| `skills/brain-knowledge-capture/SKILL.md` | Conversation distillation; 224 lines |
| `skills/article-summarizer/SKILL.md` | Source distillation; 106 lines |
| `skills/brain-skills/SKILL.md` | Workspace skill authoring; 40 lines |

## Refactor requirements

| ID | Finding and source | Required disposition |
| --- | --- | --- |
| M1 | `workspace/templates.rs` seeds AGENTS once and protects every existing document | Introduce explicit region ownership, exact legacy adoption, preserved custom bytes, and reversible backups |
| M2 | The historical `0.102.25` callback renders the running bundle before a later migration can prepare sources | Put compatibility preparation inside the first shared refresh pass; retain its once-only result |
| M3 | `skills/install_transaction/validation.rs` permits only skill-tree artifacts | Add typed instruction/config artifacts and a coherent journal, not an arbitrary path bypass |
| M4 | Manifest schema 1 rejects unknown fields; older targets cannot read a naive schema extension | Read old schemas, finish old recovery first, and export target-readable state during down |
| M5 | Installer preflight precedes live migration; replica currently captures only skill output | Project instructions, config, hook aliases, and target profiles in the private replica too |
| M6 | Skill sources are loaded by skill basename; unknown hooks append to that same skill | Route all 16 hooks explicitly and preserve catch-all/unknown content without guessing its meaning |
| M7 | Actor capability rendering silently ignores unreadable extensions while canonical install uses checked reads | Use one checked source resolver for both paths |
| M8 | Default and explicit allowlists name the old skills | Expand only decomposed selected capabilities; preserve empty/custom selections and detect machine-name collisions |
| M9 | New bundled names may already belong to plugins or user skills | Preflight collisions and preserve user ownership before any live replacement |
| M10 | Generated skill trees do not sync; AGENTS and extensions do | Keep portable rules compatible with older inventories and reconcile local output after sync |
| M11 | Workspace manifest is excluded from bisync; a local minimum-version increase does not fence peers | Do not claim such a fence; use conditional legacy routing for this compatible content split |
| M12 | Newly written AGENTS can make a fresh root appear nonempty to PARA seeding | Preserve initialization intent or recognize only managed scaffolding as empty |
| M13 | Native discovery of workspace rules is not proven for every frontend path | Deliver the common instruction payload through AgentController for all four frontends |
| M14 | A missing root cannot be repaired by an old binary that does not contain the new down migration | Downgrade requires all affected registered roots available; refuse before replacement otherwise |
| M15 | Startup reconciliation can block the pull needed to finish a previously interrupted receive | Persist pending-receive evidence and allow a narrow recovery pull before reconciliation; keep agents blocked |

## Existing instruction defects to correct while moving affected text

These are baseline inconsistencies, not behavior worth preserving as regressions.
Native command behavior and explicit user intent are authoritative.

| ID | Finding | Resolution in the proposed split |
| --- | --- | --- |
| B1 | Contacts requires tables without exception; second-brain forbids them in SMS | Central response policy owns presentation; skills specify facts |
| B2 | Agenda prose and palette hard-code `/tmp` despite `agenda_markdown_dir` | Resolve configured destinations and share the native file contract |
| B3 | Generic agenda contains personal schedules, required personal activities, and fixed habit IDs | Use actual workspace data and explicit extensions; no invented personal defaults |
| B4 | Todo uses ordinary habit skip for managed triage, which current native code rejects | Delegate to the keyed managed-triage helper and current triage workflow |
| B5 | Template naming contradicts namespaced project validation; media-note naming conflicts with indexing | Follow native validation and use indexed `notes.md` for resource summary pairs |
| B6 | Placement/move confirmation rules conflict across four sources | Explicitly requested moves/filing authorize routine placement; ask only for unresolved consequential choices or a new configured namespace |
| B7 | Project extraction sometimes instructs hand-editing metadata despite native command ownership | Use native project mutations throughout |
| B8 | Task references contain outdated chronic-window, skip/defer, and reindex rules | Use current detectors and native command semantics; do not copy stale mini-workflows |
| B9 | Triage hard-deadline screening appears after bulk changes | Screen before changing the group; preserve deliberate maintenance exceptions |
| B10 | Persona prose assumes the machine's person in authenticated shared-workspace sessions | Use the effective actor and preserve membership boundaries |
| B11 | Frontend-specific question-tool names leak into generic workflows | Describe the decision and use the available question surface or a concise textual fallback |
| B12 | Contact memory-first wording can bypass mandatory canonical lookup | Memory may guide a query; native lookup verifies the answer |
| B13 | Existing source-summary reuse conflicts with fixed summary headings | Reuse source content within the canonical saved structure, preserving provenance and quotation limits |
| B14 | Skill-authoring text blurs durable plugin source and installed output | Recommend `.config/plugins`; preserve existing unmarked user-authored skill directories |
| B15 | Relative references include a missing triage project-link target and a placeholder naming URL | Give each shared reference one owner and validate every real relative target/anchor |

## Contracts that must survive

- Projects: namespaces, canonical metadata, task reverse links, extraction before
  completion, staged-IP disposition, archive history, and native task removal with
  preview/fingerprint/partial-result behavior.
- Capture: explicit processing, every item accounted for, grouping by idea,
  task-plus-note links, no premature source removal, honest unreadable-file outcomes.
- Organizing: actionability-based PARA, recursive existing-home discovery,
  retained resource originals, paired media/notes, canonical summary headings,
  useful cross-links, link repair, retrieval across all applicable buckets.
- Tasks: native-only mutations, recurrence/skip/defer distinctions, assignments,
  chunks and blockers, backlog behavior, no automatic execution of a captured task.
- Agenda: fresh dates, first-build versus update versus native-sync behavior,
  realistic capacity, configured anchors/calendar/cutoff, preserved narrative and
  completed-item appendix, configured output paths and native PDF rendering.
- Triage: task-only cleanup, pre-mutation deadline decisions, daily/weekly phases,
  managed-habit on/off behavior, generic extension outputs, and the existing
  completion signal with runtime `require` paths.
- Sync: cloud versus local reindex distinction, structured conflict groups,
  canonical writes before loser cleanup, one batch resolve, and honest partial failure.
- Delivery: immutable actor/channel/recipient authority, SMS normalization,
  safe email HTML, durable outbox, threading and no unrequested recipient expansion.

## Compatibility inventory

| Group | Existing identifiers |
| --- | --- |
| Shared/resource context | `second-brain:company-context`, `second-brain:reference-manager` |
| Agenda extensions | `todo:agenda-after-build`, `todo:anchors`, `todo:calendar`, `todo:cutoff` |
| Task extensions | `todo:linear-backlog`, `todo:linear` |
| Contacts | `contacts:fallback` |
| Daily triage | `triage:daily-open`, `triage:daily-subagents`, `triage:daily-linear`, `triage:daily-merge`, `triage:daily-required-outputs` |
| Weekly triage | `triage:weekly-inboxes`, `triage:weekly-linear` |

Preserve old commands, heading anchors, reference paths, session prompts, and
plugin callers. First-party callers use canonical names. `/todo today` retains
task-list/recommendation behavior; `/todo agenda` and `/todo plan-day` delegate
to `/agenda`. Every route respects capability restrictions.

## Regression matrix

| ID | Scenario | Required evidence |
| --- | --- | --- |
| R1 | No customization, fresh root | Current instructions and catalog; proper seeding; no personal or external dependency |
| R2 | Exact old/custom/arbitrary AGENTS | Exact safe adoption and preserved unknown bytes |
| R3 | Deleted/edited region, malformed markers, unexpected links/types | Repair missing ownership; diagnose modified ownership without overwrite |
| R4 | Multiple roots, unavailable root, wrong UUID | Isolation, independent outcomes, remount retry, no invented root |
| R5 | Every hook plus mixed/unknown content | Correct semantic destination once; empty hooks are no-ops |
| R6 | Duplicate/colliding/unreadable sources | Lossless refusal before partial publication |
| R7 | Plugin old paths/anchors and new-name collisions | Compatibility links resolve; user source survives |
| R8 | Task-only, agenda-only, mixed requests | Correct owner and no unsolicited agenda artifacts |
| R9 | Configured agenda dirs and native mutations | One Markdown target; existing PDF refresh semantics preserved |
| R10 | Diverse habits and explicit personal extension | No global personal defaults; actual customization honored |
| R11 | Project conversion, unresolved IPs, archive open tasks | Native safety gates and durable partial outcomes |
| R12 | Mixed capture, unreadable media, interrupted processing | Every input accounted for and no premature deletion |
| R13 | Triage on/off, renamed managed habit, missing required output | Correct helper and completion timing |
| R14 | Contacts/tasks/retrieval/summary in all three channels | Same facts, central presentation, artifact/reply separation |
| R15 | Four frontends, fresh/resume/recovery, both access modes | Shared instructions and selected policy reach actual launch adapters |
| R16 | Old/custom/empty capability lists | Only equivalent permissions added; no silent source reclassification |
| R17 | Fault at each publish/stamp/disposal phase | Entire old or new generation recovered |
| R18 | Upgrade/down/re-upgrade and later source edits | Compatible target startup and customization persistence after old sync |
| R19 | Shared rules arrive before sources; crash/restart during receive | Bounded recovery pull before reconciliation; no false user-edit diagnosis or startup deadlock |
| R20 | Help/version and optional auto-sync disabled | No help/version writes; mandatory migration remains active |

## Result

The [design](../superpowers/specs/2026-10-25-brain-skills-design.md) and
[implementation plan](../superpowers/plans/2026-10-25-brain-skills.md) must make
each requirement testable. Static review identifies preservation obligations; it
does not guarantee future behavior. Release requires isolated migration tests,
representative rendered-skill scenarios, and the repository's full checks.

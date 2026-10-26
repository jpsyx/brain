# Focused Brain skills release verification

Release: 0.106.16. Task 12 base commit: `4441ccb24e64f4f77744a449d4781997c2fe3fdc`.
The final change is the Git commit introducing this record; resolve its exact identity
with `git log --diff-filter=A --format=%H -- docs/verification/brain-skills-2026-10-26.md`.
The source/index freeze and default release outcomes are recorded below. No live
installation, model/frontend session, external message, push, merge or PR is part
of this verification.

## Independent synthetic behavior

Two independent `gpt-6.1-sol` agents received rendered guidance, raw requests,
isolated artifacts and supplied native observations. They did not receive the
spec, expected answers, evaluation criteria, repository access or live tools.
They wrote complete replies, action traces and artifacts. The implementation
agent independently inspected them against the approved contract. The preserved
[readable run index](brain-skills-2026-10-26/behavior-summary.json) lists every reply;
[compressed raw evidence](brain-skills-2026-10-26/behavior.json.gz) contains inputs,
rendered guidance, complete traces and artifact text/hashes. Decompress with
`gzip -dc docs/verification/brain-skills-2026-10-26/behavior.json.gz`.

There are 38 runs, including 12 contact and 12 agenda/mutation runs labelled across
Claude, Codex, OpenCode and pi with interactive/SMS/email policies. These labels
exercise the same shared semantic contract. They do not represent real native
models. All 17 SMS replies contain at most 326 Unicode characters, no Markdown
tables or local paths. Email replies remain self-contained without local paths;
interactive artifact changes retain their changes table. Files preserve the
agenda structure and completed-task reference table independently of reply shape.

| Scenario | Observed result | Evidence scope |
| --- | --- | --- |
| Contact by name, stale remembered phone | Canonical simulated lookup returns Casey's actual fixture phone/email; old memory is not used as the answer | 12 channel/frontend-labelled runs |
| Contact by role | Native role-query intent identifies accountant and exact phone | Additional SMS run |
| Task-only listing with only todo selected | One incomplete actor-owned task; no agenda or PDF | No mutation/artifact effect |
| Todo planning with agenda unavailable | Reports unavailable planning capability; no task/file change | Capability denial, no implicit grant |
| Todo-to-agenda and subsequent completion | Builds configured plan, runs one native completion, reports committed task plus stale printable warning, no completion retry | 12 runs; PDF bytes are labelled synthetic stand-ins |
| Opaque legacy task context | Same context reused after handoff; exactly one audit append per run | Actual isolated audit files, not a source string-count claim |
| Portable and sandbox moved resource bases | Reads original extension resource and uses 09:00-12:00; sandbox reference resolves from owning SKILL.md and ignores two 18:00 decoys | Additional sandbox handoff; exact rendered origin guidance |
| Canonical-only projects selection | Projects workflow remains usable; unavailable task completion is reported, with reuse/identity decisions pending | No legacy umbrella requirement or permission expansion |
| Captured selected catalog outside workspace | Reads exact generated agenda definition/resources; no sibling-cache read, original-source read, catalog write or script execution | Separate generated-directory fixture, no additional skills/recipients |
| Mixed capture | Files note, creates one simulated native task, retains unreadable input and reports partial completion; no weekly habit completion or actual send | Original source retained/accounted for |
| Project completion/IP staging | Initial request stages copies and preserves source while awaiting IP/task choices; supplied plural CLI observations are correctly rejected as unsupported | Initial fixture grammar gap retained honestly |
| Approved project follow-up | Uses singular native project commands, promotes approved checklist, completes T101, archives entire project/staging and repairs provenance links | User choices and native outcomes supplied; fresh metadata readback absent |
| Legacy sync alias with canonical brain-sync only | Resolves through available focused workflow; reports one failed conflict cleanup and unverified final cloud sync | Simulated partial failure, no false all-clear |
| Triage, no outputs required | Uses Riley rather than machine-local Morgan; skips disabled managed habits; final completion carries empty require list | No invented extension artifacts |
| Triage, required output present | Verifies actual synthetic file; completion carries that exact requirement | Runtime output contract |
| Triage, required output missing | Keeps session open, preserves requirement, does not invent a placeholder or claim completion | Missing-output negative case |

Native calls in these scenarios were simulated from supplied observations. The
raw task fixture uses `pending` as an unfinished status, outside the native enum;
reviewers did not silently normalize or claim native CSV validation. Synthetic
PDFs do not prove renderer layout or frontend compliance. Initial project native
observations used an incorrect plural command; a separate follow-up supplies
actual singular grammar and explicit decisions, preserving the original refusal.
The approved archive flow has no fresh post-completion readback in its fixture;
its completion/archive claims are limited to the supplied committed outcomes.

## Native-schema semantic follow-up

A fresh blind `gpt-6.1-sol` reviewer received three sequential requests for task
listing, agenda construction and post-agenda completion. This isolated follow-up
uses actual native CSV/schema observations (`not_started`/`done`, native UUIDs)
produced by the default-profile 0.106.16 executable. It closes the original
synthetic task-enum limitation without relabelling the earlier traces. The
[compressed follow-up evidence](brain-skills-2026-10-26/native-schema-behavior.json.gz)
includes the exact raw inputs, rendered guidance, native command observations,
model traces and artifacts. The executable itself is represented by its SHA-256.

The reviewer counted one incomplete task, scheduled its 60 minutes at 09:00 while
leaving 10:00-12:00 and the rest of the day free, and kept calendar occupancy and
PDF generation explicitly unverified. It noticed that the initially supplied
completion observation came from a different agenda draft. The implementer then
passed the reviewer's exact draft, with matching SHA-256, through actual native
`brain tasks complete T1` once in the disposable workspace. A follow-up supplied
those raw observations without expected conclusions. The reviewer correctly
reported zero incomplete tasks and the completed reference row while preserving
the native edit's stale summary narrative. It did not retry completion or replace
the native result with a freehand rewrite. All final replies follow SMS limits;
the final follow-up is 227 Unicode characters. This is semantic review with actual
native CLI observations, not actual Claude/Codex/OpenCode/pi model transport.

## Deterministic coverage and scoped fixes

| Obligation | Deterministic evidence |
| --- | --- |
| Link, anchor, alias and asset integrity | `brain_skill_contract`: 20 passing tests; complete file inventories for bundled and installed empty/legacy/canonical/mixed/plugin variants, with Markdown-only anchor checks and negative missing-asset/anchor cases |
| Valid state DB and WAL above 64 MiB | Real ordinary CLI regression with a 67,108,865-byte unrelated SQLite blob; patch startup preserves source hashes/mtimes and package bytes, same-version startup preserves availability and logical data |
| WAL receipt and pending receive | Existing direct WAL-only tests verify committed evidence with no source sidecars; private-image and unstable-capture controls pass |
| Native env recovery | Real setters repair malformed declarations and bundled-name collision, restore deleted managed skill output, and dispatch once; unrelated setters and expected-UUID mismatch remain refused |
| Failed correction/reconciliation | Invalid values do not commit or expose raw values; successful correction followed by modified managed output reports committed repair plus failed mandatory reconciliation and preserves the user edit |
| Unicode append file | 6,000-emoji Claude append file reports actionable 20 KiB limit before UTF-8 decoding, without truncation; all 10 Claude instruction adapter tests pass |
| Actor selection | Triage guidance uses effective BRAIN_ACTOR_ID; shared-workspace synthetic cases distinguish Riley from machine-local Morgan |
| Four-frontends transport | Full suite includes decoded argument/payload matrix for all four adapters, channels, access modes and fresh/resume; controller budgets, recovery reconstruction, lifecycle and native selected-path cases remain separate from semantic review |
| Ownership/downgrade/source persistence | Full migration/installer suites and the four audited historical-binary cases below retain user prefix/suffix, canonical/legacy projections, narrowed capability scope, receiptless refusal and later ordinary historical sync |
| Personal data and executable command vocabulary | Existing complete-suite guards are run without suppression |

The former SQLite cap was reproduced before implementation. It is replaced by
8 KiB streaming buffers, private 0700/0600 files, bounded retries and repeated
SHA-256/file-identity/time observations. I/O remains proportional to database/WAL
size and requires temporary disk space; there is no arbitrary state size ceiling.
Same-version startup's pre-existing historical receiver schema callbacks open the
live Db and may checkpoint/remove WAL. That is distinct from the no-write
compatibility observation guarantee, so the real CLI regression tests both
boundaries explicitly. No historical schema callback was changed.

The native repair is limited to named `agent_capabilities` and
`agent_capabilities.skills` setters, inside ordinary invocation ownership and the
existing one-shot setup boundary. It validates the candidate before saving and
requires mandatory reconciliation before success. It is not a general startup
bypass. Explicit value omission retains the existing interactive prompt.

Scoped commands used `CARGO_PROFILE_RELEASE_LTO=false`,
`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`; initial unit covering checks used
`RUST_TEST_THREADS=8`, while later focused checks used default threads as recorded
in the execution report. Observed RED: initial CLI
integration 0/2, Unicode 0/1, triage actor 0/1 and correction redaction 0/1.
Observed GREEN/covering: integration 4/4, structural 20/20, Claude 10/10,
snapshot 2/2, receipt 1/1 and pending receive 2/2. The first streaming integration
rerun exposed historical same-version WAL checkpoint behavior; it was diagnosed
from startup callback routing, not hidden by changing production code or timeouts.

## Concurrent release-gate findings

The second frozen default attempt passed all 3,396 library tests and 76 preceding
completed targets before `workspace_cli` reported 27 passes and one failure. Across
77 completed targets the result was 4,369 passed, one failed and four expected
ignores; later workspace/doc targets and subsequent checks were not reached.
The actual child reported committed workspace setup followed by an
`unreadable_source [reconcile]` compatibility failure. An unchanged exact default
20-writer rerun passed in 60.68s. The initial fixture inherited child output, so
its per-child failure message had empty stdout/stderr.

A deterministic owner-release test independently proved that snapshotting live
`sync.lock` contents could fail with ENOENT between metadata and content reads.
The fix validates cache ancestry and non-following file kind, then uses the
unchanged ownership primitive and deadlines. RED was 0/1, GREEN 1/1; controls pass
2/2 including active-owner, symlink, directory and socket refusal. This is a
concrete fix, while attribution of the original full-run error remains qualified.

Two complete focused workspace targets separately reported a registry transaction
contention error with a configured 10,000ms budget. That printed budget is not a
measurement of elapsed wait, and the persistent owner sidecar is not proof of the
current holder. Captured output now drains every child concurrently and reports all
failed writers. A temporary instrumented run passed 28/28: maximum acquisition
wait 6.640s, hold 607.86ms (both workspace-create transactions), and PRAGMA duration
904µs, with no acquisition errors. Maximum child stderr was 1,019 bytes; the
measurements do not support claiming pipe backpressure caused the earlier failure.
The probe-free focused workspace target then passed 28/28 in 83.24s.

A later default full attempt passed formatting/strict Clippy and 4,371 tests,
then the same workspace target reported two registry refusals. A focused failing
reproduction measured PRAGMA at 11.25µs and BEGIN IMMEDIATE returning DatabaseBusy
after 10.5675s. Nineteen successful create transactions held the registry for
10.4597s across an 11.3783s span; the longest create hold was 4.2824s, whose
individual filesystem-operation timing was not measured. The actual refusal is
therefore confirmed as bounded BEGIN contention, not an immediate PRAGMA error.

The integration's all-20-success expectation imposed an undocumented throughput
assumption. The corrected test retains all 20 simultaneous requests and checks
every result: successful stdout/status and exact record/root/manifest identity;
only the exact native exit-1 pre-commit LockTimeout is permitted as a refusal,
with no refused row or root. Initial top-level fields, record, manifest and config
remain exact, and final names equal the initial workspace plus successful creators.
At least two successes keep this fixture's lost-update oracle non-vacuous. Negative
controls reject other exit codes, foreign lock paths, committed output/setup
failures and unrelated diagnostics. No retry, writer reduction, production registry
change or deadline change was introduced. All temporary probes were removed;
normalized logs and per-child timelines are retained below.

## Resume-boundary fixture investigation

The fifth default attempt passed formatting and strict Clippy, then the library
reported 3,397 passes and one failure in 505.74s. No integration targets ran.
The resume-validation expiry test observed `Retrying` instead of `Claimed`.
Its original assertion did not retain the retry reason or reached boundary, so
that attempt's path remains unmeasured. The unchanged exact default rerun passed
in 0.38s. This is not evidence that the expiry authorization itself failed.

The test uses a unique malformed-session command, initially absent from the
OpenCode compatibility cache. Compatibility runs before resume validation;
a refusal there legitimately schedules a planning retry before the test's
injected clock advances. Assertion diagnostics now include the stored retry
reason/count and require the intended validation boundary exactly once.

A diagnostic covering run passed all 298 app tests in 363.35s, including both
validation-error cases. A separate unseeded unavailable-command control exercises
the pre-boundary planning-retry path. Only the two malformed-session fixtures
seed their unique command's successful compatibility report through a test-only
helper using the existing cache. Actual malformed session discovery, clock
advance, durable authorization and final claim/launch assertions remain intact.
There is no runtime switch, production availability change or deadline change.
Dedicated real compatibility-probe tests retain their coverage.

## Historical executable provenance

| Profile | Audited source commit | Audited executable SHA-256 |
| --- | --- | --- |
| 0.101.0 | `fe477ec664ae830bbe806d8470153088318c07be` | `3d70889bb51c91acafac8f3c1dc82ba6c875b49bee72dc7e72535800b171d5de` |
| 0.105.3 | `b04a64c79e98ef4542aaf885355213fb328396f0` | `a52bc82a749cf549755afadc3a76ec9fcc0327b9da192c30577f7ff55b1cbdd0` |

Both audited source workspace templates and bundled frozen
`templates/workspace/legacy/AGENTS-v0.105.3.md` are 10,367 bytes with SHA-256
`218712096b2f41d9443c644d32b82fb482a40faf4188ea009415ae724dcd9439`.
The historical binaries were previously built with LTO disabled and 16 codegen
units. They are reused unchanged, not rebuilt for this gate. The two original
`task_deletion_skill_refresh::actual` tests consume 0.101.0. The two newer complete
package/receiptless tests exercise both 0.101.0 and 0.105.3. Exactly four tests are
ignored by the ordinary suite and explicitly run separately against final code.
Task 11's prior passes are historical evidence, not substituted for those final
covering results.

## Final default release gate

The first frozen attempt passed formatting, then `cargo test --release` returned
3395 passed and one failed library test after 496.41s (813.80s including build).
Integration, binary and doc targets were not reached. The failure was the existing
late-revocation fixture's one-second final-result busy-spin assertion, followed by
worker SendError after its receiver dropped. The original worker result and exact
slow phase cannot be recovered. An unchanged exact default-profile rerun passed
in 0.12s. Temporary test-only phase probes then showed all nine late-revocation
cases passing; the successful case reached CAS at 16.15ms and returned Ok at
52.47ms. These focused timings do not establish the original failure's cause.

The fixture now joins its released worker and evaluates its actual Result. It
retains all phase barriers, CAS-under-control-mutex and persisted-job assertions,
and production handoff deadlines. Temporary probes were removed. The final
successful default run passed on the sixth freeze. Commands used no profile or
RUST_TEST_THREADS overrides:

```sh
cargo fmt --check
cargo test --release
cargo clippy --release --all-targets -- -D warnings
sh -n install.sh
git diff --check
```

The four ignored historical cases then run on the default-profile final build:

```sh
BRAIN_TEST_LEGACY_BINARY=/tmp/brain-skills-audited-targets/bin/brain-0.101.0 BRAIN_TEST_105_BINARY=/tmp/brain-skills-audited-targets/bin/brain-0.105.3 cargo test --release --test brain_skill_migration --test task_deletion_skill_refresh -- --ignored
```

Earlier task gates recorded two process-probe timeouts and one server-election
startup failure, each passing unchanged focused reruns. Their cause was not
proved and no production deadline was weakened. The final default run revisits
those cases and passed all three. The server recovery fixture now retains child stdout/stderr log
before temporary-home cleanup if startup fails.

## Final results and retained evidence

| Check | Final result |
| --- | --- |
| `cargo fmt --check` | Pass, 1.21s |
| `cargo clippy --release --all-targets -- -D warnings` | Pass, 17.62s |
| `cargo test --release` | 4,481 passed, zero failed, four historical ignores across 87 targets; 1,678.47s including build |
| Library target | 3,399 passed in 488.75s |
| `workspace_cli` target | 29 passed in 90.15s, including all 20 classified concurrent writers and the refusal-classifier negative controls |
| `sh -n install.sh` | Pass |
| `git diff --check` and `git diff --cached --check` | Pass |
| Four audited historical cases, same final default build | 4 passed, zero failed; 109.55s; both executable hashes rechecked unchanged |

All 87 target results were inspected. The total including the separately run
historical tests is 4,485 passes. The bundled-command vocabulary guards, neutral
README identity guard, canonical schema personal-data guard and bundled build-litter
guards passed in the full run. Actual model/frontend execution remains outside
these deterministic and synthetic checks.

| Earlier frozen attempt | Preserved outcome and unrun work |
| --- | --- |
| 1 | Library 3,395 passed/one failed; integrations unrun; final-result harness assumption investigated above |
| 2 | 4,369 passed/one failed/four ignored over 77 completed targets; later workspace/doc targets unrun; ephemeral lock race independently reproduced |
| 3 | Formatting passed; strict Clippy rejected test-only needless collection; full test not started; eager waiter construction corrected |
| 4 | 4,371 passed/one failed/four ignored over 77 completed targets; later workspace/doc targets unrun; bounded registry refusal measured and test contract corrected |
| 5 | Library 3,397 passed/one failed; integrations unrun; original resume-boundary path unmeasured, independent fixture isolation corrected |

Freeze 6 ran from 2026-10-26 14:54:17 UTC until verification at 15:25:09 UTC.
Its complete staged-diff SHA-256 remained
`0bc5780999359b4f7c09335282a701b94ebb2e8db08225197b3b396f18436a9a`.
The source/test/skill/template/version/script diff SHA-256 remained
`02fe7124fe2a025cb7ab6080a917cde495eb5592a0e61d30146764492eb9610b`.
There were no unstaged changes through the gate. Only outcome documentation and
evidence files changed afterward; the source/test/version subset remains the
same tested content. Strict Clippy ran before the expensive full test, followed
by shell/diff checks and then the historical invocation. No other heavy validation
ran concurrently.

The [compressed release evidence](brain-skills-2026-10-26/release-logs.json.gz)
contains exact commands, complete outputs for RED/GREEN and every frozen attempt,
per-target final results, freeze hashes, measured registry timelines, independent
registry diagnosis and the execution report. The
[decoded evidence audit](brain-skills-2026-10-26/evidence-audit.json) records file
hashes and personal-data checks for every complete new artifact plus added/changed
product-doc text. Existing illustrative examples outside the change are excluded
from that additions audit. The post-outcome `cargo test --release --test workspace_docs`
check passed all 28 tests, including neutral README identity. Decode with
`gzip -dc docs/verification/brain-skills-2026-10-26/release-logs.json.gz`.
Host-specific repository, main-checkout and HOME paths are replaced by
`<REPOSITORY>`, `<MAIN_CHECKOUT>` and `<HOME>`. Archived text is explicitly not
byte-identical to the original logs; each original local log's SHA-256 is retained.
Synthetic fixture values, failure diagnostics, commands and outcomes are preserved.
No real environment/configuration dump, credentials or personal data are included.

The native-schema semantic follow-up used an earlier default build of 0.106.16
before the subsequent lock and test-fixture corrections. Its executable hash is
retained; task/agenda/schema behavior was unchanged by those later corrections.
At Task 12 completion, final whole-branch review and publication remained pending
controller ownership. The dated closure below records the subsequent review.

## Final review closure, 2026-10-26

All twelve implementation tasks are complete and independently reviewed.
Whole-branch review of `b04a64c7..192bdc2c` is PASS within the verification scope
recorded above, with zero Critical or Important findings and two Minor documentation
findings. The final cleanup removes stale SQLite size-refusal wording while retaining
malformed, unsafe and unstable-evidence refusal, and closes the spec, plan and
verification status consistently. These are documentation-only corrections, with the
required patch version bump to `0.106.17`; no functional source, skill, template,
schema, migration, test or dependency change is included.

The pre-existing registry diagnostic wording remains a separate, non-blocking
follow-up: the timeout describes a configured wait budget, and the sidecar records
the last successful writer rather than an atomic live-owner observation. This review
established no registry exclusion or lost-update defect and requests no production
change for that wording.

Historical attempts, hashes, versions and synthetic/native limitations above remain
unchanged. At this review checkpoint, publication and local installation had not
yet run; they are separate controller release steps after scoped re-review.

Final cleanup checks at `0.106.17`: `cargo fmt --check` passed;
`cargo test --release --test workspace_docs` passed all 28 tests with the default
release profile (1m 49s build, 0.33s tests), including the README identity guard;
`git diff --check` passed. Version/lock agreement and the exact documentation-plus-
version diff were checked. Existing identity guards cover README and the canonical
schema rather than these edited docs; added prose was separately checked for known
private identity/path values and none were found. The full release suite was not
rerun for these documentation-only corrections; its retained results above stand.

## Documentation consistency

Architecture, features, integrations, config, data model, decisions and testing
cover managed ownership, extension aliases/collisions, pending roots, target
profiles, conditional legacy fallback, immutable selected catalogs, scalable
observation and validated native repair. Glossary main-view/panel meanings and
keybinding `/agenda` ownership were audited and remain accurate; no shortcut or
palette command changed. Repository AGENTS now identifies brain-sync as the
canonical sync owner and no longer claims the complete suite takes under a second.

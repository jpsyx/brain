# Brain sync C4 — auto-sync triggers (start / exit / watcher) — design

- **Date:** 2026-07-25
- **Status:** Design — forks resolved with the user; ready for plan + build. Phase C4 of Sub-project C.
- **Scope:** make sync **automatic**. Sync on the persistent shell's **start** and
  **exit**, and via a debounced **filesystem watcher** (`notify` crate) over the
  brain root — all gated on `sync.on_start` / `sync.on_exit` / `sync.watch`
  (default-on when sync is configured). Every trigger reuses the existing
  `sync_once` machinery, never blocks or slows the shell, and coexists with the
  one shared brain HTTP server. Builds on C2 (transport, journal, `sync_once`) and
  C3 (CSV merge — the CSVs sync through the same `sync_once`, so the watcher covers
  them automatically).

---

## 1. What C4 delivers

After C4, on a machine with sync configured (`brain sync setup` done):

- Opening the shell (bare `brain`) kicks a **background pull** so you start on the
  latest brain (gated `sync.on_start`, default on).
- Quitting the shell spawns a **detached, fire-and-forget** `brain sync` so your
  last edits push without the shell ever waiting on the network (gated
  `sync.on_exit`, default on).
- While the shell is open, a **debounced filesystem watcher** auto-syncs a few
  seconds after you stop editing `~/brain` (gated `sync.watch`, default on).
- All triggers **coalesce** through a machine-wide advisory lock, so concurrent
  triggers (start + watcher + another shell + a manual `brain sync`) never run two
  rclone syncs at once — the extras skip cleanly.
- With no `sync` block configured, **nothing changes**: no watcher thread, no
  start/exit sync, brain behaves exactly as after C3.

Not in C4 (later): the `/second-brain sync` + `resolve-conflicts` skill rows (C5);
an idle-pull timer (deferred, §11); a standalone always-on sync daemon (rejected,
§10).

## 2. Resolved design forks (settled with the user at kickoff)

| Fork | Decision |
|---|---|
| **Watcher execution model** | **In-process background thread** inside the persistent shell (spawned in `run_tui`). Dies when the shell closes; `on_exit` fires the final push. **Not** a new daemon, **not** folded into the brain HTTP server — sync stays decoupled from the `/habits` server. |
| **Exit sync** | **Detached fire-and-forget.** `on_exit` spawns `brain sync` as a fully detached child and lets the shell exit instantly; the child finishes the push in the background under the lock. Quitting never waits on the network. |
| **Watcher timing** | **Event-driven** (`notify` → OS FSEvents/inotify; zero cost when idle), **3s debounce** (quiescence window from the last event; coalesces edit bursts + the sync's own writes into one sync). Configurable via `sync.debounce_ms` (default 3000). **No idle-pull timer** in C4. |
| **Concurrency** | A new **machine-wide advisory sync lock** (`~/.cache/brain/sync/sync.lock`). One sync at a time; other triggers **skip** (best-effort, non-blocking) rather than queue. The lock + a "sync in progress" guard also suppress the watcher's self-trigger from the pull's own file writes. |

## 3. Why in-process, not a daemon

- The parent spec frames C4 as **"TUI lifecycle hooks + debounce"** (§13) — the
  watcher's lifetime is the shell's lifetime.
- A standalone daemon means a whole second lifecycle to build and own (spawn,
  PID/port record, `status`/`kill`, stale-record reaping) — the `src/server/`
  machinery, duplicated for sync. Not worth it when the shell is the default
  `brain` invocation and is almost always open.
- Folding a watcher into the brain HTTP server couples sync to the `/habits`
  server being up (it can be killed independently) and mixes two unrelated
  concerns in one daemon.
- **Accepted tradeoff:** no live sync while *no* shell is open. `on_start` (next
  open), `on_exit` (last close), and manual `brain sync` cover the gap. If
  always-on sync is ever wanted, the deferred daemon (§11) is a clean add — the
  `sync_once`-under-lock core built here is exactly what it would reuse.

## 4. Module layout (`src/sync/` + one TUI seam)

C4 adds a small, **pure-first** trigger layer. The rule holds: pure decision logic
(debounce, whether-to-fire) in tested functions; the `notify`/thread/`Command`
shells stay thin.

| File | Responsibility | Pure? |
|---|---|---|
| `src/sync/lock.rs` *(new)* | The machine-wide advisory lock at `~/.cache/brain/sync/sync.lock`: `try_acquire() -> Option<Guard>` (non-blocking; `None` when another sync holds it), `Guard` releases on drop + reaps a stale lock (dead-PID record). Pure staleness decision (`is_stale(pid_alive, age)`) + thin FS/PID shell. | pure decision + thin IO |
| `src/sync/trigger.rs` *(new)* | The shell-facing entry points: `sync_in_background(reason)` — acquire the lock, run `sync_once`, journal, drop the lock, all on a spawned thread (no-op + return if the lock is held or sync unconfigured); and `spawn_detached_sync()` — spawn a fully detached `brain sync` child for `on_exit`. Reuses `command::sync_once`. | thin (wraps tested core) |
| `src/sync/watch.rs` *(new)* | `Debouncer` — the **pure** debounce state machine: fold a stream of `(event, now)` into "fire now?" decisions with a 3s quiescence window + coalescing while a sync is in flight. Plus `spawn_watcher(root, cfg) -> WatcherHandle`: the thin `notify` shell that feeds events into the `Debouncer` and calls `trigger::sync_in_background` when it says fire. `WatcherHandle` stops the thread on drop. | pure `Debouncer` + thin `notify` shell |
| `src/sync/config.rs` *(extend)* | Add `debounce_ms` (default 3000) to `SyncConfig`; `debounce()` helper returning a `Duration`. | pure |

**TUI seam (`src/tui/event_loop/setup.rs`, `run_tui`):**
- After the startup pull-baseline work and **before** `event_loop`: if
  `cfg.on_start`, call `trigger::sync_in_background("on_start")`; if
  `cfg.watch_effective()`, `let _watcher = watch::spawn_watcher(root, cfg)` (held
  for the shell's lifetime).
- After `event_loop` returns, on the way out: if `cfg.on_exit`,
  `trigger::spawn_detached_sync()` (detached — the shell then finishes teardown and
  exits). The watcher handle drops here, stopping the watch thread.

No changes to `main.rs`'s `brain sync` dispatch surface — C4 adds triggers, not new
subcommands. `brain sync status` (§8) is extended to show watcher/trigger state.

## 5. The lock (coalescing, non-blocking)

- **One sync at a time, machine-wide.** `~/.cache/brain/sync/sync.lock` holds the
  owning PID + start time. `try_acquire`:
  - no lock file, or a **stale** one (recorded PID not alive, or age past a
    generous cap) → take it, write our PID, return a `Guard`.
  - a live lock held by another process → return `None`; the caller **skips** this
    sync (logs a one-line debug note to stderr, never blocks).
- **`Guard` drops** → remove the lock file. A crash leaves a stale file that the
  next `try_acquire` reaps via the PID-liveness check (same `pid_alive` probe the
  server lifecycle already uses).
- **Why skip, not queue:** triggers are frequent and idempotent — if a sync is
  already running, the in-flight run (or the next debounce fire) will pick up
  whatever changed. Queuing would just stack redundant rclone runs. The watcher's
  `Debouncer` re-arms after a skip so pending changes aren't lost (§6).
- This lock also fixes a latent C2/C3 gap: today two concurrent `brain sync`
  invocations could race. C4's lock wraps **all** sync entry points (manual
  `run_sync` in `main.rs` included), so manual + auto never collide.

## 6. The watcher (`notify` + pure `Debouncer`)

- **`notify`** watches `brain_root()` recursively via the OS-native backend
  (FSEvents on macOS, inotify on Linux) — **push-based**: the thread blocks on a
  channel and consumes zero CPU when nothing changes. No polling.
- **Excludes (don't watch / don't trigger on):** `.git/`, `.cache/`, `.DS_Store`,
  and `*(conflict *)*` copies — the same cruft the bisync filter drops (§ C2.5), so
  a conflict copy or a VCS write never kicks a sync. The watcher applies these as a
  pure path predicate (`is_watch_relevant(path)`), tested independently.
- **`Debouncer` (pure).** Folds `(relevant_event, now)` into fire decisions:
  - an event arms/re-arms a timer for `now + debounce`;
  - `poll(now)` returns **fire** once `now` passes the armed deadline with no newer
    event (quiescence reached), then disarms;
  - while a sync is **in flight** (the lock is held / a fire is outstanding), new
    events set a **pending** flag instead of firing; when the sync completes, a
    pending flag re-arms one follow-up sync. This is the coalescing that collapses
    an edit burst — and the sync's own pull writes — into a single sync and
    guarantees the last change is never stranded.
- **Self-trigger suppression.** The pull inside `sync_once` writes files into the
  root, which the watcher sees. The lock guard means those writes land while the
  lock is held → the `Debouncer` marks them *pending* rather than firing, and the
  single follow-up sync after release is a cheap no-op bisync (nothing new). No
  infinite loop, at most one redundant no-op pass.
- **Thin shell.** `spawn_watcher` owns the `notify::Watcher`, the event channel,
  and a loop that pushes events into the `Debouncer` and calls
  `trigger::sync_in_background` on fire. `WatcherHandle::drop` signals the loop to
  exit and joins it (bounded), so shell teardown is clean.

## 7. Start / exit hooks

- **`on_start` (default on).** In `run_tui`, before the event loop:
  `trigger::sync_in_background("on_start")` — a background pull+push on a spawned
  thread under the lock. The shell renders its first frame immediately; the pull
  lands whenever it finishes. If a sync is already running (another shell), it
  skips. Never blocks startup (matches the existing "never block startup" rule for
  onboarding / triage checks).
- **`on_exit` (default on).** After the event loop returns:
  `trigger::spawn_detached_sync()` spawns `brain sync` as a **fully detached**
  child (new session/process group, stdio to `/dev/null`) and returns at once. The
  shell finishes terminal teardown and exits; the child pushes the final state
  under the lock. Detached (not an in-process thread) so it survives the parent
  exiting. If the child can't spawn, exit proceeds anyway — best-effort.
- Both hooks are **no-ops** when their flag is off or sync is unconfigured, so an
  unconfigured brain starts and quits exactly as before.

## 8. `brain sync status` extension

`brain sync status` gains a **triggers** line reflecting the effective state:
`on_start`, `on_exit`, and `watch` (on/off, with watch showing the effective value
per `watch_effective()`), plus the debounce window. Pure formatter
(`format_triggers` already exists — extend it), themed via `Theme` tokens. Surfaces
"is auto-sync actually on?" at a glance, the human-friendly counterpart to the env
flags.

## 9. Testing (pure-first, per house rules)

- **`Debouncer`** (the crown jewel of C4): single event → fires after the window;
  a burst → fires **once** at quiescence; events during in-flight → coalesced into
  exactly one follow-up; empty stream → never fires; idempotent `poll` after fire.
  Deterministic — `now` is injected, no real clock, no sleeps.
- **`is_watch_relevant` / exclude predicate:** `.git/`, `.cache/`, `.DS_Store`,
  conflict copies excluded; a normal note/CSV included.
- **`lock::is_stale`** classifier: live PID → not stale; dead PID or over-age →
  stale/reapable. Lock round-trip (`try_acquire` twice in-process → second is
  `None`; drop the first → third acquires) against a temp `HOME`/cache.
- **`config`:** `debounce_ms` default 3000; `debounce()` → `Duration`; absent
  block still disables everything.
- **`format_triggers`:** on/off rendering incl. `watch_effective` interaction;
  plain-text assertion via `Theme::dark(false)`.
- **Shell/thread/`notify` shells stay thin and are not unit-tested directly** (per
  strategy — no mocking the terminal or the FS). A single **gated** integration
  test may exercise `spawn_watcher` end-to-end against a throwaway `HOME`/root with
  rclone's local backend (like C2/C3's gated `tests/sync_local.rs`), touching a
  file and asserting one sync fires — **never** against the real B2 bucket.

## 10. Concurrency & safety invariants

- **Never block the shell.** Start = background thread; exit = detached child;
  watcher = its own thread. The event loop is never made to wait on a sync.
- **One sync at a time** via the lock; extras skip. Manual `brain sync` is wrapped
  too, closing the pre-existing race.
- **Best-effort everywhere.** A failed spawn, a held lock, an absent rclone, or a
  watcher error is logged (stderr, one line) and swallowed — a sync trigger never
  crashes or hangs the shell. rclone/network errors still journal via `sync_once`'s
  existing verification path.
- **Decoupled from the brain server.** The watcher thread lives in the shell, not
  the HTTP daemon; killing/restarting the server has no effect on sync, and vice
  versa.

## 11. Deferred (noted, not built in C4)

- **Idle-pull timer.** A periodic pull while the shell sits open with no local
  edits, to pick up other machines' changes without reopening. `on_start` + manual
  cover it for now; add a `sync.idle_pull_secs` later if it's missed. (Parent spec
  §7's "idle timer".)
- **Standalone always-on sync daemon.** For sync while no shell is open. The
  `sync_once`-under-lock core built in C4 is what it would reuse; §3 explains why
  it's not worth it yet.
- **Watcher batching stats in `status`** (last watcher fire time, coalesced count)
  — nice-to-have, journal already records each run.

## 12. Docs to update (same change)

- `docs/features.md` — auto-sync on start/exit + the live watcher (default-on when
  configured, off with `sync.watch=false`); the `status` triggers line.
- `docs/architecture.md` — the `notify` **dependency** (justify it: the only
  correct, cross-platform, OS-native FS-event crate; the alternative is a polling
  loop we explicitly rejected as wasteful) + the C4 trigger modules
  (`lock`/`trigger`/`watch`) and the `run_tui` lifecycle seam.
- `docs/integrations.md` — the sync lock at `~/.cache/brain/sync/sync.lock`; the
  detached `on_exit` child; the watcher's exclude set.
- `docs/data-model.md` — the `debounce_ms` env field; the lock-file record shape.
- `docs/config.md` — `sync.on_start` / `on_exit` / `watch` / `debounce_ms` (what
  each does, defaults, how to disable); note these are **brain env** `sync` fields.
- `docs/decisions.md` — in-process-thread over daemon; detached fire-and-forget
  exit; event-driven `notify` over polling; skip-not-queue coalescing; the
  machine-wide sync lock (and that it also closes the manual-sync race).
- The docs-contract table in `AGENTS.md`/`CLAUDE.md` — a C4 row: "the auto-sync
  triggers (start/exit hooks, the `notify` watcher + debounce, the sync lock)" →
  `docs/features.md` + `docs/architecture.md` + `docs/integrations.md` +
  `docs/decisions.md` (modules `src/sync/{lock,trigger,watch}.rs`; the seam in
  `src/tui/event_loop/setup.rs`).

## 13. Acceptance criteria

1. On a configured machine, opening the shell triggers a background sync
   (`on_start`) without blocking startup; quitting spawns a detached `brain sync`
   (`on_exit`) and the shell exits immediately.
2. Editing a file under `~/brain` while the shell is open triggers exactly one sync
   ~3s after edits settle; a burst of edits coalesces into one sync; the pull's own
   writes do not cause an infinite re-sync loop.
3. Concurrent triggers (start + watcher + a second shell + manual `brain sync`)
   never run two rclone syncs at once; extras skip via the lock; a crash leaves a
   stale lock that the next run reaps.
4. `sync.watch=false` disables the watcher; `on_start=false` / `on_exit=false`
   disable those hooks; an unconfigured brain has no watcher thread and syncs
   nowhere. `brain sync status` shows the effective trigger state + debounce window.
5. Every auto-sync is journalled exactly like a manual one (reuses `sync_once`);
   the watcher/threads are best-effort and never crash or hang the shell.
6. The repo stays generic (no bucket/host/key/personal path); the watcher is
   decoupled from the brain HTTP server.
7. `cargo test --release` green; `cargo clippy --release --all-targets` clean.

## 14. Phase decomposition (for the C4 plan)

Each a self-contained RED→GREEN TDD slice:

- **C4.1 — The lock.** `src/sync/lock.rs`: `is_stale` classifier + `try_acquire`/
  `Guard` (reap-on-stale, release-on-drop) against a temp cache. Wire the lock
  around the existing manual `run_sync` in `main.rs` (closes the pre-existing
  race; smallest first slice with immediate value).
- **C4.2 — The pure `Debouncer` + exclude predicate.** `src/sync/watch.rs`
  `Debouncer` and `is_watch_relevant`, fully unit-tested (no `notify` yet).
- **C4.3 — `notify` dependency + `spawn_watcher`.** Add `notify` (justify in
  architecture.md); the thin shell feeding events into the `Debouncer`;
  `WatcherHandle` stop-on-drop. Gated local-backend integration test.
- **C4.4 — Triggers + config.** `src/sync/trigger.rs`
  (`sync_in_background` / `spawn_detached_sync`); `debounce_ms` +`debounce()` on
  `SyncConfig`.
- **C4.5 — TUI wiring.** The `run_tui` seam: `on_start` background sync, hold the
  watcher handle, `on_exit` detached spawn. `brain sync status` triggers line.
- **C4.6 — Docs.** Per §12.

## 15. Open questions (resolved in the plan / at kickoff)

- The exact detach mechanism for `spawn_detached_sync` on macOS/Linux
  (`setsid` / `process_group(0)` via `std::os::unix` + stdio to `/dev/null`) —
  pick the portable-unix spelling at C4.4; the shape is settled.
- Stale-lock age cap (a generous upper bound, e.g. 10 min, purely as a backstop
  behind the PID-liveness reap) — fix the constant at C4.1.
- Whether the `on_start` sync should be pull-biased (`--pull`) to prioritize
  landing remote changes fast, or a plain bidirectional `both` — lean plain `both`
  (bisync reconciles both directions anyway); confirm at C4.5.
- `notify`'s debounced vs. raw API: use the **raw** `RecommendedWatcher` and do
  debouncing in our own tested `Debouncer` (don't depend on `notify-debouncer-*`),
  keeping the decision logic pure and ours.

# Brain Sync Idle Pull Timer

## Goal

Pick up remote edits from another machine while a `brain` shell stays open for a
long time, without introducing an always-on daemon. Add an opt-in
`sync.idle_pull_secs` field: `0` or missing disables it; a positive value starts
one shell-lifetime timer that periodically runs a pull-biased sync under the
existing machine-wide sync lock.

## Scope

In scope:

- Parse `sync.idle_pull_secs` from the machine-local sync block.
- Surface the effective idle-pull state in `brain sync status`.
- Add a small `src/sync/idle.rs` timer shell over a pure interval decision.
- Wire the timer into the TUI lifecycle beside the existing C4 triggers.

Out of scope:

- A standalone always-on sync daemon.
- Any new public server endpoint or auth surface.
- Changing the default automatic trigger behavior.

## Behavior

- Missing `idle_pull_secs` defaults to `0`, so existing configured machines do
  not start periodic network work.
- A positive value is effective only when sync is configured.
- Each timer fire calls `trigger::run_locked_sync(Direction::Pull)`, so remote
  changes win only for same-file conflicts on that scheduled pull. Local pending
  edits still converge through the normal rclone/CSV machinery.
- The existing sync lock coalesces with manual sync, start sync, watcher sync,
  and exit sync. If another sync is already running, the timer fire skips.
- The timer is held only for the lifetime of the shell and is stopped on shell
  exit. The detached `on_exit` sync remains the final push.

## Tests

- RED/GREEN config tests for default/parsed/effective `idle_pull_secs`.
- RED/GREEN status formatter test that renders idle-pull `off` or `Ns`.
- RED/GREEN idle timer shell test using a tiny interval and injected callback.
- Full `cargo test --release` and `cargo clippy --release --all-targets`.

## Docs

Update `docs/config.md`, `docs/features.md`, `docs/architecture.md`,
`docs/integrations.md`, `docs/data-model.md`, `docs/decisions.md`, and the
running handoff.

# Brain Sync Idle Pull Timer Plan

## Goal

Add an opt-in `sync.idle_pull_secs` timer that pulls periodically while the
shell is open, reusing the C4 sync lock and trigger core.

## Steps

- [x] Config RED/GREEN: add tests for default `0`, parsed positive values,
  `idle_pull_effective`, and `idle_pull_interval`.
- [x] Status RED/GREEN: extend `format_triggers` so `brain sync status` shows
  `idle-pull off` or `idle-pull <Ns>`.
- [x] Timer RED/GREEN: add `src/sync/idle.rs` with `spawn_idle_puller_with`
  tested using an injected callback and a short interval.
- [x] TUI wiring: hold the idle-pull handle in `run_tui` beside the watcher and
  drop it on shell exit.
- [x] Docs: update config, feature, architecture, integration, data model, and
  decision docs in the same change.
- [x] Validate with `cargo test --release` and
  `cargo clippy --release --all-targets`, then commit, merge, delete branch,
  and update this handoff.

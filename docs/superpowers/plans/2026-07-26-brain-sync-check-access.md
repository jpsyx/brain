# Plan: Brain Sync `--check-access`

## Scope

Implement the spec in
`docs/superpowers/specs/2026-07-26-brain-sync-check-access.md`.

## Steps

1. **RED: argv guard.**
   Add a failing `src/sync/args.rs` unit test proving `bisync_args` includes
   `--check-access` and `--check-filename RCLONE_TEST`.

2. **GREEN: argv guard.**
   Add the flags in `bisync_args` and keep the existing excludes and
   `--max-delete` unchanged.

3. **RED: marker bootstrap.**
   Add pure/mostly-pure tests for a new marker helper:
   `check_access::marker_path`, `remote_marker_arg`, and local marker creation
   with generic content.

4. **GREEN: marker bootstrap.**
   Add `src/sync/check_access.rs`, export it from `src/sync/mod.rs`, and keep
   rclone transport thin: local write via `fs::write`; remote write via
   `rclone copyto` using existing `run_rclone_capture`.

5. **RED/GREEN: setup/init wiring.**
   Wire marker bootstrap before setup's initial resync and before
   `brain sync init` resync. Test the pure decision path where possible; keep
   the actual rclone call injected or thin.

6. **RED/GREEN: abort message.**
   Extend `AbortKind`/`parse_outcome`/`verify::classify` so check-access aborts
   tell the user to run `brain sync init`.

7. **Docs.**
   Update the docs listed in the spec, including the running handoff.

8. **Validation.**
   Run `cargo test --release` and
   `cargo clippy --release --all-targets`.

9. **Commit + merge.**
   Commit the feature, update the handoff with the commit SHA, merge to `main`
   with `--no-ff`, and delete `feat/check-access`.

# Plan: Native `brain tasks complete`

## Scope

Implement the spec in
`docs/superpowers/specs/2026-07-26-brain-native-complete.md`.

## Steps

1. **RED: native completion API.**
   Add failing tests for `complete_in_root_with_today`: completing a task marks
   it done/touched, and completing a habit appends the next occurrence.

2. **GREEN: native completion API.**
   Implement CSV read/write, row lookup, status/timestamp mutation, recurrence,
   counter update, and MIT migration in `src/tasks/complete.rs`.

3. **RED/GREEN: callers.**
   Rewire the TUI mark-complete path and `/habits` route to call the native API
   instead of spawning a script. Keep the CLI command as `brain tasks complete`.

4. **Remove script payload.**
   Stop bundling `skills/todo/scripts/mark_done.py` and delete the file.

5. **Reference audit.**
   Scan and update bundled brain skills, global skills, and global rules for
   direct `mark_done.py` references, replacing them with
   `brain tasks complete <id>` when they describe completion.

6. **Docs.**
   Update the docs listed in the spec, including the running handoff.

7. **Validation.**
   Run `cargo test --release` and
   `cargo clippy --release --all-targets`.

8. **Commit + merge.**
   Commit the feature, update the handoff with the commit SHA, merge to `main`
   with `--no-ff`, and delete `feat/native-complete`.

# Brain Webhook Capture Endpoint — Plan

## Steps

1. Add a failing pure router test for `POST /webhooks/capture`.
2. Add a failing handler test proving a non-empty JSON body is persisted under
   `scratch/webhooks/` and returns a relative JSON path.
3. Implement the route enum, dispatch, and `routes/webhooks` module.
4. Add an empty-body rejection test and implementation.
5. Update `docs/architecture.md`, `docs/features.md`, and
   `docs/integrations.md`.
6. Run `cargo test --release` and `cargo clippy --release --all-targets`.
7. Update `docs/superpowers/brain-sync-status.md`, commit, merge to `main`, and
   delete the feature branch.

## Test Notes

Use injected timestamps for deterministic filenames. Unit tests use a temp brain
root and never start the daemon or touch the user's real brain.


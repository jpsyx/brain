# Plan: Brain Sync `rclone crypt`

## Scope

Implement the spec in
`docs/superpowers/specs/2026-07-26-brain-sync-rclone-crypt.md`.

## Steps

1. **RED: config schema.**
   Add failing `src/sync/config.rs` unit tests proving crypt defaults are
   disabled, explicit fields parse, and `crypt_enabled()` depends only on a
   non-empty `crypt_password`.

2. **GREEN: config schema.**
   Add the optional crypt fields and `crypt_enabled()` helper.

3. **RED: layered remote builder.**
   Add failing `src/sync/remote.rs` tests proving the B2-only remote is
   unchanged and crypt-enabled configs append a `BRAINCRYPT` remote while
   returning `BRAINCRYPT:` as the argv target. Prove secrets stay in env, not
   argv.

4. **GREEN: layered remote builder.**
   Build the crypt env remote on top of the existing B2 env remote. Keep the
   function pure.

5. **RED/GREEN: setup preservation.**
   Add a pure helper around setup's `sync` JSON block and test that existing
   crypt fields survive a bucket credential refresh. Then wire setup through
   that helper.

6. **Docs.**
   Update the docs listed in the spec, including the running handoff.

7. **Validation.**
   Run `cargo test --release` and
   `cargo clippy --release --all-targets`.

8. **Commit + merge.**
   Commit the feature, update the handoff with the commit SHA, merge to `main`
   with `--no-ff`, and delete `feat/rclone-crypt`.

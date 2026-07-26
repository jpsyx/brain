# Brain Sync `--check-access` Guard

## Status

Planned from the §19 sync backlog. This feature adds rclone's `--check-access`
guard now that brain can own the marker-file lifecycle.

## Problem

`brain sync` already uses `--max-delete` to cap blast radius, but it does not
yet use rclone's path-symmetry guard. `rclone bisync --check-access` aborts
unless the configured check file exists on both sides of the sync. That catches
wrong-root or wrong-remote mistakes before a bisync run can treat the other side
as a valid peer.

C2 intentionally skipped this flag because brain did not create or maintain the
marker file. Turning it on without lifecycle ownership would make every setup
fail.

## Goals

- Create the rclone check-access marker during `brain sync setup`.
- Recreate/repair the marker during `brain sync init`.
- Pass `--check-access` and an explicit `--check-filename RCLONE_TEST` on every
  bisync run.
- Keep normal sync honest: do not silently create missing markers before a
  regular `brain sync`, `--push`, or `--pull`; if the guard fails, the user
  should run `brain sync init` to re-establish the baseline and marker.
- Surface check-access aborts as a clear recovery message.

## Non-Goals

- No new user-facing command or config knob.
- No secret material in the marker. The marker is a generic sync sentinel.
- No rclone crypt or remote-layout changes.

## Design

Use rclone's default check filename, `RCLONE_TEST`, explicitly in argv with
`--check-filename RCLONE_TEST` so the contract is visible in tests and docs.
The marker lives at the brain root and at the remote sync root. Its content is
stable generic text, e.g. `brain sync access marker\n`.

Setup/init call an explicit marker bootstrap before the resync run:

1. Ensure `<brain-root>/RCLONE_TEST` exists with the expected generic content.
2. Push that file to `<remote-root>/RCLONE_TEST` using rclone `copyto`.
3. Run the existing resync path, whose argv now includes `--check-access`.

Normal sync does not call the bootstrap. If either marker is missing, rclone
aborts. `run::parse_outcome` should recognize the check-access failure and
`verify::classify` should point the user at `brain sync init`.

## Docs Contract

Update:

- `docs/features.md` for the behavior and recovery path.
- `docs/integrations.md` for the rclone flags and marker lifecycle.
- `docs/architecture.md` for the new marker bootstrap module/path.
- `docs/data-model.md` for the marker file as root-level sync metadata.
- `docs/decisions.md` to replace the old "not yet" rationale with the shipped
  lifecycle.
- `docs/superpowers/brain-sync-status.md` when complete.

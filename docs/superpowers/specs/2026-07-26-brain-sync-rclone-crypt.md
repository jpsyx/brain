# Brain Sync `rclone crypt`

## Status

Planned from the §19 sync backlog. This feature layers optional client-side
`rclone crypt` encryption over the existing env-defined B2 remote.

## Problem

The shipped sync posture uses a private B2 bucket with Backblaze-managed
server-side encryption. That protects against public bucket exposure and plain
storage at rest, but Backblaze and anyone with a valid B2 application key can
still read object contents. `rclone crypt` adds zero-knowledge encryption before
objects leave the machine.

C2 deliberately left this as a later seam because it adds a passphrase the user
must not lose. brain should support the layer without changing the `brain sync`
command surface or writing a persisted `rclone.conf`.

## Goals

- Add optional `sync` block fields for rclone crypt:
  `crypt_password`, `crypt_password2`, `crypt_filename_encryption`, and
  `crypt_directory_name_encryption`.
- Keep crypt disabled when `crypt_password` is empty, preserving existing
  B2-only behavior.
- When crypt is enabled, build two rclone remotes from env vars only:
  the existing `BRAIN` B2 remote, and a `BRAINCRYPT` crypt remote whose
  `remote` points at `BRAIN:<bucket>/<path>`.
- Keep all existing sync code using `remote.arg`; the remote builder chooses
  whether that argv target is `BRAIN:...` or `BRAINCRYPT:`.
- Preserve existing crypt fields when `brain sync setup` rewrites the `sync`
  block for bucket credentials.
- Document that passwords must be rclone-obscured values and must be escrowed
  by the user outside brain.

## Non-Goals

- No new `brain sync` subcommands, flags, or command surface.
- No passphrase generation, storage, recovery, or password-manager integration.
- No migration of existing unencrypted remotes into encrypted remotes.
- No persisted `rclone.conf`.

## Design

`SyncConfig` gains optional parse-only crypt fields. The enabling predicate is
`crypt_enabled() == !crypt_password.trim().is_empty()`. `build_remote` keeps the
existing `BRAIN` B2 env vars. When crypt is enabled it appends:

- `RCLONE_CONFIG_BRAINCRYPT_TYPE=crypt`
- `RCLONE_CONFIG_BRAINCRYPT_REMOTE=<BRAIN arg>`
- `RCLONE_CONFIG_BRAINCRYPT_PASSWORD=<crypt_password>`
- optional `RCLONE_CONFIG_BRAINCRYPT_PASSWORD2=<crypt_password2>`
- optional filename/directory encryption knobs when non-empty or false

and returns `Remote.arg = "BRAINCRYPT:"`. Using a crypt remote with an empty path
means every brain-root-relative path is encrypted below the configured B2 root.
The underlying B2 root remains configurable via `b2_path`.

`brain sync setup` preserves the crypt fields from the existing config when it
rewrites bucket credentials. Users can add or rotate the crypt fields in the
machine-local `sync` block; `brain sync init` then establishes a new baseline
against the selected encrypted remote. The docs must be explicit that a lost
crypt password makes existing encrypted remote data unrecoverable.

## Docs Contract

Update:

- `docs/config.md` for the new `sync` fields.
- `docs/integrations.md` for the layered env remote and password handling.
- `docs/data-model.md` for the `SyncConfig` schema.
- `docs/decisions.md` for SSE-first, crypt-now rationale.
- `docs/superpowers/brain-sync-status.md` when complete.

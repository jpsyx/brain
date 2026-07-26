# Brain Webhook Capture Endpoint — Spec

## Goal

Add the first generic inbound webhook endpoint to the local brain server:
`POST /webhooks/capture`.

The endpoint captures arbitrary webhook bodies into the brain root so agents can
triage them later, without encoding any vendor-specific payload shape in the
public repo.

## Scope

- Add `POST /webhooks/capture` to the existing localhost-only brain server.
- Persist each request body as a file under
  `<brain-root>/scratch/webhooks/`.
- Return JSON with `{"ok": true, "path": "scratch/webhooks/<file>"}` on success.
- Reject empty bodies with a `400` JSON error.
- Keep routing pure and handlers thin.
- Keep the endpoint generic: no vendor names, private URLs, tokens, bucket
  names, or personal fields.

## Non-goals

- No public internet listener; the server still binds only to `127.0.0.1`.
- No webhook authentication yet. A future tunnel/public relay must add an auth
  layer before exposing this outside localhost.
- No task creation or CSV mutation in this slice. Captured payloads are triage
  input, not tasks.
- No new dependency.

## Behavior

`POST /webhooks/capture` writes the raw body to a timestamped file. If the body
looks like JSON, the file uses `.json`; otherwise it uses `.txt`. Filenames are
deterministic from an injected clock in tests and collision-resistant in normal
use.

The response path is brain-root-relative so it is safe to show to users and
portable across machines.


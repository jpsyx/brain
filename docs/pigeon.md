# Pigeon (work in progress)

> **Status: work in progress.** Pigeon is not part of the `brain` binary yet.
> No command, view, panel, or frontend selector launches it, and nothing in
> `src/` knows it exists. It lives in `pigeon/` so it can grow next to brain
> until it is ready to become the default agent for brain's TUIs.

Pigeon is brain's own agent harness: a branded build of the
[pi coding agent](https://pi.dev) whose job is brain work (notes, tasks,
knowledge management, personal assistance) rather than coding. It is built on
`@avandar/aiviary-core`, the library behind the other aiviary birds, so running
`pigeon` feels like its own program: its own command, home directory, theme,
ASCII-art header, and system prompt. Nothing about it says "pi".

## What exists

| Piece | Where |
| --- | --- |
| The `pigeon` command | `pigeon/src/main.ts` (calls `runBird` from `@avandar/aiviary-core`) |
| Look: picture, block-letter name, palette, working message | `pigeon/src/brand.ts` |
| Instructions, appended to pi's system prompt | `pigeon/SYSTEM.md` |
| Home: settings, sessions, logins, theme | `~/.pigeon/agent` (override with `PIGEON_HOME`) |

Pigeon is an independent agent. It shares code with the other aiviary birds
(`@avandar/aiviary-core` and the pi engine), never configuration: its settings,
logins, sessions, MCP servers, instructions, packages, and skills live only
in `~/.pigeon/agent`, and its environment settings only in `PIGEON_*`
variables (`PIGEON_OFFLINE=1` is pi's `PI_OFFLINE=1`, for Pigeon alone).
Plain pi's `~/.pi` and `PI_*` variables never reach it. Configure it inside
Pigeon (`/settings`, `/login`, `/mcp`) or in `~/.pigeon/agent`.

Pigeon has no extensions, skills, or prompt templates of its own yet. Adding
`pigeon/extensions/*.ts`, `pigeon/skills/`, or `pigeon/prompts/` is enough for
the launcher to load them.

Pigeon's skill policy is `own-and-project`: it loads its own skills and the
current project's `.agents/skills` (once the project is trusted), and never
the skills installed machine-wide in `~/.agents/skills`. Run inside a brain
workspace, the project's skills are the ones `brain skills sync` renders
there, so Pigeon sees brain's skills and nothing meant for other work.

## Running it

Pigeon is a pnpm project in `pigeon/`, separate from the Rust build. It needs
Node.js 22.19 or newer, pnpm, and a local checkout of aiviary with its
dependencies installed, because `@avandar/aiviary-core` is not published yet.

```sh
cd pigeon
AIVIARY_DIR=<path to aiviary> pnpm install   # installs, then links the core package
pnpm dev                                     # run Pigeon from source
pnpm dev --help                              # Pigeon's options (every pi option works)
pnpm check                                   # type-check and test
pnpm bird:link                               # put `pigeon` on your PATH (~/.local/bin)
pnpm bird:unlink                             # take it off again
```

`pnpm install` links `@avandar/aiviary-core` only when `AIVIARY_DIR` is set;
without it, it warns and skips. `pnpm link:core --aiviary-dir <path>` links it
at any time. The link points at the aiviary checkout, so an edit there takes
effect in Pigeon on the next run. On first run, use `/login` inside Pigeon to
connect a model provider.

## Still to do

- Depend on a published `@avandar/aiviary-core` (a pinned Git or registry
  version) instead of a local checkout, so a fresh clone of brain can run
  Pigeon.
- Give Pigeon its brain-specific skills and extensions.
- Make Pigeon a brain-panel agent frontend, following
  [adding-an-agent-frontend.md](adding-an-agent-frontend.md), and then the
  default agent for brain's TUIs. Until that change lands, the frontend
  docs, registry, and tests do not mention Pigeon on purpose.
- Bundle Pigeon to JavaScript before shipping it anywhere its dependencies
  sit inside `node_modules`, where Node will not strip TypeScript types.

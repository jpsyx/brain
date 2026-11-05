#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { runBird } from "@aiviary/core";
import { PIGEON_BRAND } from "./brand.ts";

/**
 * `pigeon`: Pigeon, brain's own agent harness on pi. Work in progress: see
 * `docs/pigeon.md`.
 *
 * Everything that makes Pigeon different lives in this directory:
 * `src/brand.ts` (how it looks), `SYSTEM.md` (how it works), and
 * `extensions/`, `skills/`, and `prompts/` when it has any. The launcher in
 * `@aiviary/core` hands them to pi.
 */
const packageDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const { version } = JSON.parse(
  readFileSync(join(packageDir, "package.json"), "utf8"),
) as { version: string };

process.exitCode = await runBird(
  { brand: PIGEON_BRAND, packageDir, version },
  process.argv.slice(2),
);

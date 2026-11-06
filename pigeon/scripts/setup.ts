#!/usr/bin/env node
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runBirdSetup } from "@avandar/aiviary-core";

/**
 * `pnpm bird:link` / `pnpm bird:unlink`: put the `pigeon` command on your PATH
 * or take it off, with `@avandar/aiviary-core`'s setup tool. Run
 * `node scripts/setup.ts --help` for the details.
 */
const PIGEON_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");

process.exitCode = await runBirdSetup({
  command: "node scripts/setup.ts",
  birds: [{ name: "pigeon", entryPath: join(PIGEON_DIR, "src", "main.ts") }],
  argv: process.argv.slice(2),
});

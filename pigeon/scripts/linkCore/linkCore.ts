#!/usr/bin/env node
import { existsSync, lstatSync, readFileSync } from "node:fs";
import { mkdir, rm, symlink } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline/promises";
import { fileURLToPath } from "node:url";
import { styleText } from "node:util";
import {
  buildLinkCoreHelp,
  CORE_PACKAGE_NAME,
  parseLinkCoreArgs,
} from "./linkCorePlan.ts";

/**
 * `pnpm link:core`: links `@aiviary/core` from a local aiviary checkout. Run
 * `pnpm link:core --help` for the details.
 *
 * It cannot use `@aiviary/core`'s own output helpers, because linking that
 * package is its job, so it styles its few lines itself.
 */
const PIGEON_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const COMMAND = "pnpm link:core";

function _say(format: "gray" | "greenBright" | "yellowBright" | "redBright", text: string): void {
  process.stderr.write(`${styleText(format, text, { stream: process.stderr })}\n`);
}

function _fail(message: string, fix: string): number {
  _say("redBright", `❌ ${COMMAND}: ${message}`);
  _say("yellowBright", `  ⚠️ ${fix}`);
  return 1;
}

async function _askForCheckout(): Promise<string | undefined> {
  if (process.stdin.isTTY !== true || process.stderr.isTTY !== true) {
    return undefined;
  }
  const terminal = createInterface({ input: process.stdin, output: process.stderr });
  try {
    const answer = (await terminal.question("Path to your aiviary checkout: ")).trim();
    return answer === "" ? undefined : answer;
  } finally {
    terminal.close();
  }
}

async function _main(): Promise<number> {
  const request = parseLinkCoreArgs({ argv: process.argv.slice(2), env: process.env });
  if ("error" in request) {
    process.stderr.write(buildLinkCoreHelp());
    return _fail(request.error, `Run ${COMMAND} --help for the options.`);
  }
  if (request.isHelp) {
    process.stdout.write(buildLinkCoreHelp());
    return 0;
  }
  const aiviaryDir =
    request.aiviaryDir ?? (request.isIfConfigured ? undefined : await _askForCheckout());
  if (aiviaryDir === undefined) {
    if (request.isIfConfigured) {
      _say(
        "yellowBright",
        `⚠️ AIVIARY_DIR is not set, so ${CORE_PACKAGE_NAME} was not linked. Pigeon will not run until it is: see docs/pigeon.md.`,
      );
      return 0;
    }
    process.stderr.write(buildLinkCoreHelp());
    return _fail(
      "no aiviary checkout was named",
      `Pass one: ${COMMAND} --aiviary-dir <path>, or set AIVIARY_DIR.`,
    );
  }

  const coreDir = resolve(aiviaryDir, "packages", "core");
  _say("gray", `… Linking ${CORE_PACKAGE_NAME} from ${coreDir}`);
  const manifestPath = join(coreDir, "package.json");
  const name = existsSync(manifestPath)
    ? (JSON.parse(readFileSync(manifestPath, "utf8")) as { name?: string }).name
    : undefined;
  if (name !== CORE_PACKAGE_NAME) {
    return _fail(
      `${coreDir} is not ${CORE_PACKAGE_NAME}`,
      "Point --aiviary-dir or AIVIARY_DIR at the root of an aiviary checkout.",
    );
  }
  if (!existsSync(join(coreDir, "node_modules"))) {
    return _fail(
      `${CORE_PACKAGE_NAME} has no dependencies installed`,
      `Run pnpm install in ${resolve(aiviaryDir)}, then try again.`,
    );
  }

  const linkPath = join(PIGEON_DIR, "node_modules", "@aiviary", "core");
  if (existsSync(linkPath) && !lstatSync(linkPath).isSymbolicLink()) {
    return _fail(
      `${linkPath} is a real directory, not a link`,
      "Remove it, then run this again.",
    );
  }
  await mkdir(dirname(linkPath), { recursive: true });
  await rm(linkPath, { force: true });
  await symlink(coreDir, linkPath);
  _say("greenBright", `✅ ${CORE_PACKAGE_NAME} is linked: ${linkPath} -> ${coreDir}`);
  return 0;
}

process.exitCode = await _main();

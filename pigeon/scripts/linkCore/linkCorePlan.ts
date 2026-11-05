import { parseArgs } from "node:util";

/** A parsed `link:core` command line. */
export type LinkCoreRequest = {
  aiviaryDir: string | undefined;
  isIfConfigured: boolean;
  isHelp: boolean;
};

/** The environment variable naming the aiviary checkout. */
export const AIVIARY_DIR_ENV_VAR_NAME = "AIVIARY_DIR";

/** The package Pigeon is built on. */
export const CORE_PACKAGE_NAME = "@aiviary/core";

/**
 * The `link:core` help text.
 *
 * @returns The help text.
 */
export function buildLinkCoreHelp(): string {
  return `pnpm link:core: link @aiviary/core from an aiviary checkout into Pigeon.

Usage:
  pnpm link:core [--aiviary-dir <dir>] [--if-configured]

Pigeon is built on @aiviary/core, which is not published yet. This links the
package from a local aiviary checkout into node_modules/@aiviary/core, so an
edit there takes effect in Pigeon at once. pnpm install runs it with
--if-configured.

Options:
  --aiviary-dir <dir>  The aiviary checkout (default: $${AIVIARY_DIR_ENV_VAR_NAME}). Run pnpm
                       install inside it first.
  --if-configured      Skip with a warning, instead of failing, when no
                       checkout is named.
  -h, --help           Show this help.

Examples:
  AIVIARY_DIR=~/src/aiviary pnpm install
  pnpm link:core --aiviary-dir ../../aiviary
`;
}

/**
 * Parses the `link:core` arguments. Help wins over everything else.
 *
 * @param options The arguments and the environment.
 * @returns The request, or an error message for an invalid one.
 */
export function parseLinkCoreArgs(options: {
  argv: readonly string[];
  env: Readonly<Record<string, string | undefined>>;
}): LinkCoreRequest | { error: string } {
  try {
    const { values, positionals } = parseArgs({
      args: [...options.argv],
      allowPositionals: true,
      options: {
        "aiviary-dir": { type: "string" },
        "if-configured": { type: "boolean" },
        help: { type: "boolean", short: "h" },
      },
    });
    if (values.help === true) {
      return { aiviaryDir: undefined, isIfConfigured: false, isHelp: true };
    }
    if (positionals.length > 0) {
      return { error: `unexpected argument "${positionals[0]}"` };
    }
    const fromEnv = options.env[AIVIARY_DIR_ENV_VAR_NAME];
    return {
      aiviaryDir:
        values["aiviary-dir"] ?? (fromEnv === "" ? undefined : fromEnv),
      isIfConfigured: values["if-configured"] === true,
      isHelp: false,
    };
  } catch (error: unknown) {
    return { error: error instanceof Error ? error.message : String(error) };
  }
}

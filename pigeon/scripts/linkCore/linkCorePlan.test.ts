import { describe, expect, it } from "vitest";
import { buildLinkCoreHelp, parseLinkCoreArgs } from "./linkCorePlan.ts";

describe("parseLinkCoreArgs", () => {
  it("takes the checkout from the flag before the environment", () => {
    expect(
      parseLinkCoreArgs({
        argv: ["--aiviary-dir", "/flag"],
        env: { AIVIARY_DIR: "/env" },
      }),
    ).toEqual({ aiviaryDir: "/flag", isIfConfigured: false, isHelp: false });
    expect(
      parseLinkCoreArgs({ argv: ["--if-configured"], env: { AIVIARY_DIR: "/env" } }),
    ).toEqual({ aiviaryDir: "/env", isIfConfigured: true, isHelp: false });
  });

  it("treats an empty environment variable as unset", () => {
    expect(parseLinkCoreArgs({ argv: [], env: { AIVIARY_DIR: "" } })).toMatchObject({
      aiviaryDir: undefined,
    });
  });

  it("lets help win and rejects anything unknown", () => {
    expect(parseLinkCoreArgs({ argv: ["--bogus", "-h"], env: {} })).toHaveProperty(
      "error",
    );
    expect(parseLinkCoreArgs({ argv: ["-h"], env: {} })).toMatchObject({ isHelp: true });
    expect(parseLinkCoreArgs({ argv: ["extra"], env: {} })).toHaveProperty("error");
  });
});

describe("buildLinkCoreHelp", () => {
  it("documents every option with examples", () => {
    const help = buildLinkCoreHelp();
    ["--aiviary-dir", "--if-configured", "--help", "AIVIARY_DIR", "Examples:"].forEach(
      (text) => {
        expect(help).toContain(text);
      },
    );
  });
});

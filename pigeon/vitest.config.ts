import { defineConfig } from "vitest/config";

/** Pigeon's tests live beside its source, in `src/` and `scripts/`. */
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "scripts/**/*.test.ts"],
  },
});

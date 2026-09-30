// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const here = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  resolve: {
    alias: [
      { find: "@pidash/api-client", replacement: resolve(here, "../../packages/api-client/src/index.ts") },
      { find: /^@pidash\/kit$/, replacement: resolve(here, "../../packages/kit/src/index.ts") },
      { find: "@pidash/edition", replacement: resolve(here, "src/core/edition/oss.ts") },
      { find: "@pidash/platform-target", replacement: resolve(here, "src/core/platform/web.ts") },
    ],
  },
  test: {
    // Node by default: F-04's platform codec test compares byte buffers
    // across realms, which jsdom breaks. DOM suites opt into jsdom with a
    // `// @vitest-environment jsdom` pragma on their first line.
    environment: "node",
    setupFiles: ["./src/test-setup.ts"],
    // Feature tests live under src. Named checks/ files only: other checks
    // carry node:test self-tests (e.g. coexistence-routes, run via
    // test:coexistence) that vitest must not sweep up.
    include: ["src/**/*.test.ts", "src/**/*.test.tsx", "checks/check-similarity.test.mjs"],
  },
});

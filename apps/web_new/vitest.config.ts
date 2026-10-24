// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

import { FULL_RUN_TEST_TIMEOUT_MS } from "./src/testing/testBudget";
import { FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY, FULL_RUN_MAX_WORKERS } from "./src/testing/testRunConcurrency";
import { resolveEditionModule } from "./tools/edition-module.mjs";

const here = dirname(fileURLToPath(import.meta.url));

// Same swap as vite.config.ts so tests can run against the cloud module.
const { modulePath: editionModule } = resolveEditionModule(process.env, here);

export default defineConfig({
  resolve: {
    alias: [
      { find: "@pidash/api-client", replacement: resolve(here, "../../packages/api-client/src/index.ts") },
      { find: /^@pidash\/kit$/, replacement: resolve(here, "../../packages/kit/src/index.ts") },
      { find: "@pidash/edition", replacement: editionModule },
      // Same public-sources alias as vite.config.ts (see there).
      { find: /^@web_new\/(.+)$/, replacement: `${resolve(here, "src")}/$1` },
      { find: "@pidash/platform-target", replacement: resolve(here, "src/core/platform/web.ts") },
    ],
  },
  test: {
    // Node by default: F-04's platform codec test compares byte buffers
    // across realms, which jsdom breaks. DOM suites opt into jsdom with a
    // `// @vitest-environment jsdom` pragma on their first line.
    environment: "node",
    setupFiles: ["./src/test-setup.ts"],
    // Keep the worker pool below Vitest's cpus - 1 default: the reporter and
    // coverage work share the main process, and a saturated main process can
    // miss the worker RPC's 60s deadline after every test has passed. See
    // src/testing/testRunConcurrency.ts.
    maxWorkers: FULL_RUN_MAX_WORKERS,
    // A full run is ~7 000 tests; on a busy host the tail of the per-test
    // distribution is queueing time, not test work (measured p99.9 8.4 s at
    // load 163). Keep the ceiling above that tail but below a minute so a
    // genuinely hung test still fails. See src/testing/testBudget.ts and
    // RUNBOOK.md ("Testing").
    testTimeout: FULL_RUN_TEST_TIMEOUT_MS,
    hookTimeout: FULL_RUN_TEST_TIMEOUT_MS,
    // Feature tests live under src. Named checks/ files only: other checks
    // carry node:test self-tests (e.g. coexistence-routes, run via
    // test:coexistence) that vitest must not sweep up. Under e2e/, vitest
    // tests are *.test.ts(x); Playwright specs are *.spec.ts and node:test
    // self-tests are *.test.mjs, so neither matches.
    include: [
      "src/**/*.test.ts",
      "src/**/*.test.tsx",
      "checks/check-similarity.test.mjs",
      "checks/check-headers.test.mjs",
      "tools/prerender.test.mjs",
      "tools/edition-module.test.mjs",
      "checks/check-oracle-build.test.mjs",
      "checks/check-boundaries.test.mjs",
      "checks/check-tokens.test.mjs",
      "checks/test-changed.test.mjs",
      "checks/test-impact.test.mjs",
      "checks/full-suite.test.mjs",
      "e2e/**/*.test.ts",
      "e2e/**/*.test.tsx",
    ],
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "json-summary", "lcov"],
      reportsDirectory: "coverage",
      include: ["src/**"],
      processingConcurrency: FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY,
    },
  },
});

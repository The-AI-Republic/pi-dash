// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Perf config (NEWFRONT-21). Serves the production bundles (dist/web and
// dist/desktop — build first with `pnpm --filter web_new build`) through
// serve.mjs and runs the budget specs against them. The specs mock every
// API call, so this needs browsers but no backend:
//
//   pnpm --filter web_new test:perf
import { defineConfig } from "@playwright/test";

const webUrl = process.env["PERF_WEB_URL"] ?? "http://localhost:3021";
const desktopUrl = process.env["PERF_DESKTOP_URL"] ?? "http://localhost:3022";

export default defineConfig({
  testDir: ".",
  // Serial: frame and load timings must not compete with each other for CPU.
  workers: 1,
  fullyParallel: false,
  // dist/ is git-ignored; the default test-results/ dir is not.
  outputDir: "../../dist/perf-results",
  timeout: 120_000,
  expect: { timeout: 15_000 },
  webServer: [
    {
      command: "node ./serve.mjs --dir ../../dist/web --port 3021",
      url: webUrl,
      reuseExistingServer: true,
      timeout: 30_000,
    },
    {
      command: "node ./serve.mjs --dir ../../dist/desktop --port 3022",
      url: desktopUrl,
      reuseExistingServer: true,
      timeout: 30_000,
    },
  ],
  projects: [{ name: "perf", testMatch: /perf\..*\.spec\.ts/ }],
});

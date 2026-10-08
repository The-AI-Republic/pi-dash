// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Playwright config for the parity suite (NEWFRONT-19). The same scenarios
// run twice: once against apps/web (the oracle, proving the scenario) and
// once against apps/web_new (proving parity). Frontends and the seeded
// stack come up through the runbook in stack/README.md; the config never
// starts servers itself. Results land as JSON for the parity report.
import { defineConfig } from "@playwright/test";

const oracleBase = process.env["PARITY_ORACLE_URL"] ?? "http://localhost:13000";
const newBase = process.env["PARITY_NEW_URL"] ?? "http://localhost:3010";

export default defineConfig({
  testDir: ".",
  testMatch: ["**/*.spec.ts"],
  workers: 1,
  // Generous: the oracle runs from a dev server that compiles routes on
  // first load, and several oracle runs share the scratch stack, so a
  // scenario can sit through minutes of rate-limit backoff and cold
  // hydration through no fault of the app. Serial workers keep one run
  // from adding to the pile.
  timeout: 300_000,
  expect: { timeout: 30_000 },
  reporter: [["list"], ["json", { outputFile: "../../test-results/parity-results.json" }]],
  projects: [
    {
      // One retry: the oracle shares its scratch stack and box with sibling
      // runs, so a scenario can lose its browser or sit through a
      // rate-limit burst through no fault of the app. Every scenario is
      // convergent, so a retry re-proves rather than papers over; a real
      // app regression fails deterministically across both attempts.
      name: "oracle",
      retries: 1,
      use: { baseURL: oracleBase },
    },
    {
      name: "new",
      use: { baseURL: newBase },
    },
  ],
});

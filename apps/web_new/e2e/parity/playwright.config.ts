// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
  // first load, so the first scenario of a run is slow through no fault of
  // the app.
  timeout: 180_000,
  expect: { timeout: 30_000 },
  reporter: [["list"], ["json", { outputFile: "../../test-results/parity-results.json" }]],
  projects: [
    {
      name: "oracle",
      use: { baseURL: oracleBase },
    },
    {
      name: "new",
      use: { baseURL: newBase },
    },
  ],
});

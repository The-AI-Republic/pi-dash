// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Smoke-test config (NEWFRONT-17). The parity harness (NEWFRONT-19) owns
// CI wiring; until then this runs on demand against a scratch Django
// seeded with packages/api-client/contract/seed-contracts.py:
//
//   export PIDASH_E2E_BASE_URL="http://127.0.0.1:8123"
//   export PIDASH_E2E_EMAIL="contract.tester@example.com"
//   export PIDASH_E2E_PASSWORD="Contract123!"
//   export PIDASH_E2E_WORKSPACE="contract-acme"
//   export PIDASH_E2E_PROJECT_ID="<seed handle project id>"
//   pnpm --filter web_new test:e2e
//
// Without PIDASH_E2E_BASE_URL every suite skips so CI stays green.
import { defineConfig } from "@playwright/test";

const appUrl = process.env["PIDASH_E2E_APP_URL"] ?? "http://localhost:3010";
const apiOrigin = process.env["PIDASH_E2E_BASE_URL"] ?? "http://localhost:8000";

export default defineConfig({
  testDir: "./e2e",
  // dist/ is git-ignored; the default test-results/ dir is not.
  outputDir: "./dist/e2e-results",
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: appUrl,
  },
  webServer: {
    command: "pnpm dev",
    url: appUrl,
    reuseExistingServer: true,
    timeout: 60_000,
    env: { PIDASH_API_ORIGIN: apiOrigin },
  },
  projects: [{ name: "smoke", testMatch: /smoke\..*\.spec\.ts/ }],
});

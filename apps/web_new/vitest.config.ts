// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

import { FULL_RUN_TEST_TIMEOUT_MS } from "./src/testing/testBudget";
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
    // A full run is ~7 000 tests; on a busy host the tail of the per-test
    // distribution is queueing time, not test work (measured p99.9 8.4 s at
    // load 163). Keep the ceiling above that tail but below a minute so a
    // genuinely hung test still fails. See src/testing/testBudget.ts and
    // RUNBOOK.md ("Testing").
    testTimeout: FULL_RUN_TEST_TIMEOUT_MS,
    hookTimeout: FULL_RUN_TEST_TIMEOUT_MS,
    // Feature tests live under src. Named checks/ files only: other checks
    // carry node:test self-tests (e.g. coexistence-routes, run via
    // test:coexistence) that vitest must not sweep up. The parity helper
    // tests are named likewise: Playwright specs (*.spec.ts) must never be
    // swept up here.
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
      "e2e/parity/exports/pdfFacts.test.ts",
      "e2e/parity/helpers/parity-env.test.ts",
      "e2e/parity/helpers/parity-guard.test.ts",
      "e2e/parity/helpers/api.test.ts",
      "e2e/parity/helpers/seed-settle.test.ts",
      "e2e/parity/helpers/asset-bucket.test.ts",
      "e2e/parity/drivers/web-toast.test.ts",
      "e2e/parity/drivers/web-sameness.test.ts",
      "e2e/parity/drivers/web-assistant-core.test.ts",
      "e2e/parity/drivers/web-new-gantt.test.ts",
      "e2e/parity/drivers/web-new-kanban.test.ts",
      "e2e/parity/drivers/web-new-modal.test.ts",
      "e2e/parity/drivers/web-new-comments.test.ts",
      "e2e/parity/drivers/web-new-layouts.test.ts",
      "e2e/parity/drivers/web-new-webhooks.test.ts",
      "e2e/parity/drivers/web-new-activity.test.ts",
      "e2e/parity/drivers/web-new-issue-widgets.test.ts",
      "e2e/parity/drivers/web-new-marketing.test.ts",
      "e2e/parity/drivers/web-new-notifications-prefs.test.ts",
      "e2e/parity/drivers/web-new-prompts.test.ts",
      "e2e/parity/drivers/web-new-project-settings.test.ts",
      "e2e/parity/drivers/web-new-runners.test.ts",
      "e2e/parity/drivers/web-new-assistant.test.ts",
      "e2e/parity/drivers/web-new-assistant-core.test.ts",
      "e2e/parity/drivers/web-new-assistant-plus.test.ts",
      "e2e/parity/drivers/web-new-billing.test.ts",
      "e2e/parity/drivers/web-new-cloud-session.test.ts",
      "e2e/parity/drivers/web-new-cloud-apps.test.ts",
      "e2e/parity/drivers/web-new-cloud-tabs.test.ts",
      "e2e/parity/drivers/web-new-workspace-settings.test.ts",
      "e2e/parity/drivers/web-new-workspace-settings-dom.test.ts",
      "e2e/parity/drivers/web-new-members.test.ts",
      "e2e/parity/drivers/web-new-drafts.test.ts",
      "e2e/parity/drivers/web-new-members-manage.test.ts",
      "e2e/parity/drivers/web-new-estimates-repository.test.ts",
      "e2e/parity/drivers/web-new-auth-card.test.ts",
      "e2e/parity/drivers/web-new-contract-surface.test.ts",
      "e2e/parity/drivers/web-new-nav-badge.test.ts",
      "e2e/parity/drivers/web-new-space-navigation.test.ts",
      "e2e/parity/drivers/web-new-members-writes.test.ts",
      "e2e/parity/drivers/web-new-add-runner.test.ts",
      "e2e/parity/drivers/web-new-profile.test.ts",
      "e2e/parity/drivers/web-new-analytics.test.ts",
      "e2e/parity/drivers/web-new-exports-history.test.ts",
      "e2e/parity/drivers/web-new-cycles-filters.test.ts",
      "e2e/parity/drivers/web-new-schedulers.test.ts",
      "e2e/parity/drivers/web-new-cycles-menu.test.ts",
      "e2e/parity/drivers/modal-copy.test.tsx",
      "e2e/parity/helpers/calendar-dates.test.ts",
      "e2e/parity/drivers/web-favorites.test.ts",
      "e2e/parity/drivers/web-modules-settle.test.ts",
      "e2e/parity/drivers/web-cycles-favorite.test.ts",
      "e2e/parity/drivers/web-new-archives-shell.test.ts",
      "e2e/parity/drivers/web-new-modules-shell.test.ts",
      "e2e/perf/mocks.test.ts",
    ],
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "json-summary", "lcov"],
      reportsDirectory: "coverage",
      include: ["src/**"],
    },
  },
});

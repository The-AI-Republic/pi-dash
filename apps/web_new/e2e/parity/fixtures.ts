// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared Playwright fixtures (NEWFRONT-19). Scenarios import `test` and
// `expect` from here instead of `@playwright/test` directly, so every
// scenario gets the seeded facts and the target-selected driver.
import { test as base } from "@playwright/test";
import { createDriver, type ParityDriver } from "./drivers/index";
import type { ParitySeedFacts } from "./drivers/parity-driver";
import { seedFactsFromEnv } from "./helpers/api";

interface ParityFixtures {
  driver: ParityDriver;
  seed: ParitySeedFacts;
}

export const test = base.extend<ParityFixtures>({
  // eslint-disable-next-line no-empty-pattern -- seed facts come from the environment, not from other fixtures.
  seed: async ({}, use) => {
    await use(seedFactsFromEnv());
  },
  driver: async ({ page }, use, testInfo) => {
    const fromProject = testInfo.project.name === "new" ? "web_new" : "web";
    const override = process.env["PARITY_TARGET"];
    const target = override === "web" || override === "web_new" ? override : fromProject;
    await use(createDriver(page, target === "web_new" ? "web_new" : "web"));
  },
});

export { expect } from "@playwright/test";

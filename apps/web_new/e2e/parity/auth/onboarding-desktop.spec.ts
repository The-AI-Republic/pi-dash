// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop onboarding legs for AUTH-030/031/043 (NEWFRONT-491 gate). These
// encode the NEWFRONT-156 target behavior — no CLI-install step on desktop,
// profile first with no back — which differs from current main until main
// PR 494 merges, so they run against apps/web_new only, on a desktop-mode
// dev server (PIDASH_TARGET=desktop, PARITY_NEW_URL pointing at it).
// Selected by file path in the desktop invocation, never in web runs.

import { expect, test } from "../fixtures.js";
import { browserCookies, signUpFreshUser } from "../helpers/api.js";
import { specTags, specTitle } from "../helpers/tags.js";

test(
  specTitle(["AUTH-030"], "desktop onboarding starts at the profile step (NEWFRONT-156 target)"),
  { tag: specTags(["AUTH-030"]) },
  async ({ driver }) => {
    const user = await signUpFreshUser("parity-desk-profile");

    await test.step("no CLI-install step; the profile step shows first with no back", async () => {
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await expect.poll(() => driver.pageText(), { timeout: 30_000 }).toContain("Create your profile");
      for (const heading of ["Connect your AI agents with Pi Dash", "Install the Pi Dash CLI"]) {
        expect(await driver.pageText()).not.toContain(heading);
      }
      expect(await driver.isOnboardingBackVisible()).toBe(false);
    });
  }
);

test(
  specTitle(["AUTH-031"], "desktop profile submit advances to the workspace step (NEWFRONT-156 target)"),
  { tag: specTags(["AUTH-031"]) },
  async ({ driver }) => {
    const user = await signUpFreshUser("parity-desk-advance");

    await test.step("completing the profile reaches workspace setup", async () => {
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await driver.submitProfileStep("Desk Profiler");
      await expect.poll(() => driver.pageText(), { timeout: 30_000 }).toContain("Create your workspace");
    });
  }
);

test(
  specTitle(["AUTH-043"], "desktop profile-first step shows no back control (NEWFRONT-156 target)"),
  { tag: specTags(["AUTH-043"]) },
  async ({ driver }) => {
    const user = await signUpFreshUser("parity-desk-back");

    await test.step("the first step has no back; later steps do", async () => {
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await expect.poll(() => driver.pageText(), { timeout: 30_000 }).toContain("Create your profile");
      expect(await driver.isOnboardingBackVisible()).toBe(false);
      await driver.submitProfileStep("Desk Back");
      await driver.awaitWorkspaceStep();
      expect(await driver.isOnboardingBackVisible()).toBe(true);
      await driver.clickOnboardingBack();
      await expect.poll(() => driver.pageText(), { timeout: 30_000 }).toContain("Create your profile");
    });
  }
);

// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the cloud frame, edition marker, upgrade pill and
// desktop update control (NEWFRONT-126). The seeded build suppresses the
// rail everywhere, never mounts the edition badge, and ships a web update
// control that renders nothing; the upgrade pill still marks the paywalled
// Cycles destination in its page header without touching navigation. Anything
// needing the matching cloud or desktop build is recorded as a gap in the
// inventory. Rows: SHELL-099 (suppression half), SHELL-100, SHELL-101,
// SHELL-102.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-099", "SHELL-100", "SHELL-101", "SHELL-102"];

test(
  specTitle(ROWS, "cloud frame, edition marker, upgrade pill, desktop update"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("rail-suppressed pages keep full content width", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      expect(await driver.railPresent()).toBe(false);
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      expect(await driver.railPresent()).toBe(false);
    });

    await test.step("no edition badge mounts in this build", async () => {
      expect(await driver.editionBadgePresent()).toBe(false);
    });

    await test.step("upgrade pills mark destinations without affecting navigation", async () => {
      // The pill mounts beside the paywalled Cycles header (the sidebar rows
      // that carry the small variant are dead or owned by other areas in this
      // build), so prove the header mount and that the page still navigates.
      await driver.page.goto(`/${seed.workspaceSlug}/active-cycles/`);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain("/active-cycles");
      await expect.poll(() => driver.activeCyclesHeaderVisible(), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.upgradePillCount(), { timeout: 15_000 }).toBeGreaterThan(0);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain(`/${seed.workspaceSlug}/`);
    });

    await test.step("no desktop update control renders in the web sidebar", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      expect(await driver.desktopUpdatePresent()).toBe(false);
    });
  }
);

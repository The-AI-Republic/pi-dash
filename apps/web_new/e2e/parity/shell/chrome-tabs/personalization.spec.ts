// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the sidebar personalization dialog (NEWFRONT-126).
// The dialog toggles personal entries with server persistence, switches
// the project list between accordion and tabbed rendering, and caps the
// listed project count with a digit-stripping, minimum-one input. Rows:
// SHELL-070, SHELL-071.
import { test, expect } from "../../fixtures";
import {
  getSidebarPreferences,
  getWorkspaceUserProperties,
  patchWorkspaceUserProperties,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-070", "SHELL-071"];

test(
  specTitle(ROWS, "sidebar personalization toggles and project list preferences"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const originalProps = await getWorkspaceUserProperties(seed.workspaceSlug, session);
    const originalMode = (originalProps["navigation_control_preference"] as string | undefined) ?? "ACCORDION";
    const originalLimit = originalProps["navigation_project_limit"] as number | null | undefined;

    await test.step("personal toggles hide entries and persist on the server", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.openPersonalizeDialog();
      expect(await driver.personalizeDialogOpen()).toBe(true);

      const before = await getSidebarPreferences(seed.workspaceSlug, session);
      const draftsBefore = before["drafts"]?.is_pinned ?? true;
      expect(await driver.personalItemChecked("Drafts")).toBe(draftsBefore);

      await driver.setPersonalItemEnabled("Drafts", !draftsBefore);
      await expect
        .poll(async () => (await getSidebarPreferences(seed.workspaceSlug, session))["drafts"]?.is_pinned, {
          timeout: 15_000,
        })
        .toBe(!draftsBefore);
      const sidebarText = await driver.page.locator("#main-sidebar").innerText();
      expect(sidebarText.includes("Drafts")).toBe(!draftsBefore);

      await driver.setPersonalItemEnabled("Drafts", draftsBefore);
      await expect
        .poll(async () => (await getSidebarPreferences(seed.workspaceSlug, session))["drafts"]?.is_pinned, {
          timeout: 15_000,
        })
        .toBe(draftsBefore);
    });

    await test.step("rendering mode switches and persists with a visible effect", async () => {
      expect(await driver.projectNavMode()).toBe(originalMode);
      const nextMode = originalMode === "TABBED" ? "ACCORDION" : "TABBED";
      await driver.setProjectNavMode(nextMode);
      await expect
        .poll(
          async () => (await getWorkspaceUserProperties(seed.workspaceSlug, session))["navigation_control_preference"],
          {
            timeout: 15_000,
          }
        )
        .toBe(nextMode);

      await driver.page.keyboard.press("Escape");
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      const stripInTabbed = nextMode === "TABBED";
      expect((await driver.projectHeaderText()) !== null).toBe(stripInTabbed);

      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.openPersonalizeDialog();
      await driver.setProjectNavMode(originalMode as "ACCORDION" | "TABBED");
      await expect
        .poll(
          async () => (await getWorkspaceUserProperties(seed.workspaceSlug, session))["navigation_control_preference"],
          {
            timeout: 15_000,
          }
        )
        .toBe(originalMode);
    });

    await test.step("reordering personal entries persists", async () => {
      const dialog = driver.page.locator('[role="dialog"]');
      const orderBefore = await getSidebarPreferences(seed.workspaceSlug, session);
      const yourWorkOrder = orderBefore["your_work"]?.sort_order ?? 0;
      const draftsOrder = orderBefore["drafts"]?.sort_order ?? 0;

      const yourWorkRow = dialog.getByText("Your work", { exact: true });
      const draftsRow = dialog.getByText("Drafts", { exact: true });
      await yourWorkRow.dragTo(draftsRow);
      await expect
        .poll(async () => (await getSidebarPreferences(seed.workspaceSlug, session))["your_work"]?.sort_order, {
          timeout: 15_000,
        })
        .toBe(draftsOrder);
      const draftsNow = (await getSidebarPreferences(seed.workspaceSlug, session))["drafts"]?.sort_order;
      expect(draftsNow).toBe(yourWorkOrder);

      await draftsRow.dragTo(yourWorkRow);
      await expect
        .poll(async () => (await getSidebarPreferences(seed.workspaceSlug, session))["your_work"]?.sort_order, {
          timeout: 15_000,
        })
        .toBe(yourWorkOrder);
    });

    await test.step("the project cap stores a count and sanitizes its input", async () => {
      const capBefore = await driver.projectCapEnabled();
      await driver.setProjectCap(true, 2);
      await expect
        .poll(async () => (await getWorkspaceUserProperties(seed.workspaceSlug, session))["navigation_project_limit"], {
          timeout: 15_000,
        })
        .toBe(2);
      expect(await driver.projectCapInput()).toBe("2");

      const dialog = driver.page.locator('[role="dialog"]');
      await dialog.locator('input[type="number"]').first().fill("a3b");
      expect(await driver.projectCapInput()).toBe("3");
      await dialog.locator('input[type="number"]').first().fill("0");
      await expect(dialog.getByText("Minimum value is 1")).toBeVisible({ timeout: 10_000 });

      await driver.setProjectCap(capBefore ?? false, typeof originalLimit === "number" ? originalLimit : 1);
      if (capBefore === false) {
        await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
          navigation_project_limit: originalLimit ?? null,
        });
      }
      await expect.poll(async () => driver.projectCapEnabled(), { timeout: 15_000 }).toBe(capBefore);
      await driver.page.keyboard.press("Escape");
    });
  }
);

// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the sidebar personalization dialog (NEWFRONT-126).
// The dialog toggles personal entries with an immediate visible effect,
// switches the project list between accordion and tabbed rendering, and
// caps the listed project count with a digit-stripping, minimum-one input.
// The toggle and reorder writes do not persist per member on the old app:
// the bulk preference endpoint answers 200 while updating the newest row
// across members instead of the signed-in member's, so the SHELL-070
// scenario proves the visible behavior only and must not become the parity
// target (bug NEWFRONT-137; web_new persists per member). Rows: SHELL-070,
// SHELL-071.
import { test, expect } from "../../fixtures";
import { getWorkspaceUserProperties, patchWorkspaceUserProperties, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS_070 = ["SHELL-070"];
const ROWS_071 = ["SHELL-071"];

test(
  specTitle(ROWS_070, "bug: personal toggles and reorder apply in the UI but do not persist per member (NEWFRONT-137)"),
  { tag: specTags(ROWS_070) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("personal toggles flip with an immediate sidebar effect", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.openPersonalizeDialog();
      expect(await driver.personalizeDialogOpen()).toBe(true);

      const draftsBefore = await driver.personalItemChecked("Drafts");
      expect(draftsBefore).not.toBeNull();

      await driver.setPersonalItemEnabled("Drafts", !draftsBefore);
      await expect.poll(() => driver.personalItemChecked("Drafts"), { timeout: 15_000 }).toBe(!draftsBefore);
      const sidebarText = await driver.page.locator("#main-sidebar").innerText();
      expect(sidebarText.includes("Drafts")).toBe(!draftsBefore);

      await driver.setPersonalItemEnabled("Drafts", draftsBefore ?? true);
      await expect.poll(() => driver.personalItemChecked("Drafts"), { timeout: 15_000 }).toBe(draftsBefore);
    });

    await test.step("reordering personal entries rearranges the dialog list", async () => {
      const namesBefore = await driver.personalItemNames();
      expect(namesBefore).toEqual(expect.arrayContaining(["Your work", "Drafts"]));

      // Dropping an entry onto another moves it into that slot, so the
      // expected order follows from whichever order we started from.
      const moved = (names: string[], dragged: string, target: string): string[] => {
        const next = names.filter((n) => n !== dragged);
        next.splice(names.indexOf(target), 0, dragged);
        return next;
      };

      await driver.movePersonalItem("Your work", "Drafts");
      await expect
        .poll(() => driver.personalItemNames(), { timeout: 15_000 })
        .toEqual(moved(namesBefore, "Your work", "Drafts"));

      await driver.movePersonalItem("Drafts", "Your work");
      await expect.poll(() => driver.personalItemNames(), { timeout: 15_000 }).toEqual(namesBefore);
      await driver.page.keyboard.press("Escape");
    });
  }
);

test(
  specTitle(ROWS_071, "project list rendering mode and cap persist per member"),
  { tag: specTags(ROWS_071) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const originalProps = await getWorkspaceUserProperties(seed.workspaceSlug, session);
    const originalMode = (originalProps["navigation_control_preference"] as string | undefined) ?? "ACCORDION";
    const originalLimit = originalProps["navigation_project_limit"] as number | null | undefined;

    await test.step("rendering mode switches and persists with a visible effect", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.openPersonalizeDialog();
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

    await test.step("the project cap stores a count and sanitizes its input", async () => {
      const capBefore = await driver.projectCapEnabled();
      await driver.setProjectCap(true, 2);
      await expect
        .poll(async () => (await getWorkspaceUserProperties(seed.workspaceSlug, session))["navigation_project_limit"], {
          timeout: 15_000,
        })
        .toBe(2);
      expect(await driver.projectCapInput()).toBe("2");

      // The input blocks exponent characters the number field would
      // otherwise accept, while plain digits flow through to the server.
      const dialog = driver.page.locator('[role="dialog"]');
      const input = dialog.locator('input[type="number"]').first();
      await input.click();
      await input.press("End");
      await input.press("e");
      expect(await driver.projectCapInput()).toBe("2");
      await input.press("5");
      expect(await driver.projectCapInput()).toBe("25");
      await expect
        .poll(async () => (await getWorkspaceUserProperties(seed.workspaceSlug, session))["navigation_project_limit"], {
          timeout: 15_000,
        })
        .toBe(25);

      // Below-minimum values are clamped on the server and flagged inline.
      await input.fill("0");
      await expect(dialog.getByText("Minimum value is 1")).toBeVisible({ timeout: 10_000 });
      await expect
        .poll(async () => (await getWorkspaceUserProperties(seed.workspaceSlug, session))["navigation_project_limit"], {
          timeout: 15_000,
        })
        .toBe(1);

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

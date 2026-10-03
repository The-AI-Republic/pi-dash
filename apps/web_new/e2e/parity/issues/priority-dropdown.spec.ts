// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): priority dropdown — static enum options
// with per-level icons, none as a real value, picks persist server-side.
// Rows: ISS-210 (priority dropdown).
import { test, expect } from "../fixtures";
import { serverCreateIssueFull, serverCleanupIssueWithSession, serverIssue, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

async function openOwnIssue(
  driver: {
    openEntry(): Promise<void>;
    signInWithPassword(e: string, p: string): Promise<void>;
    openIssueDetail(w: string, p: string, i: string): Promise<void>;
  },
  seed: { email: string; password: string; workspaceSlug: string; projectId: string },
  issueId: string
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
}

test(
  specTitle(["ISS-210"], "priority dropdown offers five icon levels and persists picks"),
  { tag: specTags(["ISS-210"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 priority ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    try {
      await openOwnIssue(driver, seed, issue.id);

      await test.step("all five levels render with an icon each", async () => {
        await driver.propertyOpenPicker("Priority");
        expect(await driver.pickerOptionTexts()).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
        for (const level of ["Urgent", "High", "Medium", "Low", "None"]) {
          const icons = await page.getByRole("listbox").getByRole("option", { name: level }).locator("svg").count();
          expect(icons).toBeGreaterThanOrEqual(1);
        }
        await driver.pickerClickOutside();
      });

      await test.step("picking urgent persists and renders in the row", async () => {
        await driver.propertyOpenPicker("Priority");
        await driver.pickerPick("Urgent");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, seed.projectId, issue.id, session)).priority, {
            timeout: 15_000,
          })
          .toBe("urgent");
        await expect.poll(() => driver.propertyValueText("Priority"), { timeout: 15_000 }).toContain("Urgent");
      });

      await test.step("none is a real value, not a clear action", async () => {
        await driver.propertyOpenPicker("Priority");
        await driver.pickerPick("None");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, seed.projectId, issue.id, session)).priority, {
            timeout: 15_000,
          })
          .toBe("none");
        await expect.poll(() => driver.propertyValueText("Priority"), { timeout: 15_000 }).toContain("None");
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

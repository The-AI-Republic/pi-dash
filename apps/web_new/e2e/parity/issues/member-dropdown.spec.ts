// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): member / assignee dropdown — current-user
// "You" first, name search, guest exclusion, multi-select staying open.
// Rows: ISS-208 (member dropdown).
import { test, expect } from "../fixtures";
import {
  serverCreateIssueFull,
  serverCleanupIssueWithSession,
  serverIssue,
  serverMe,
  serverPatchIssue,
  serverProjectMembers,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
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
  specTitle(["ISS-208"], "member dropdown search, multi-select and project scope"),
  { tag: specTags(["ISS-208"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 members ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, `${tag} issue`, session);
    const me = await serverMe(session);
    try {
      await openOwnIssue(driver, seed, issue.id);

      await test.step("current user renders first as You with an initial badge", async () => {
        await driver.propertyOpenPicker("Assignees");
        const options = await driver.pickerOptionTexts();
        expect(options[0]).toContain("You");
        expect(options.length).toBeGreaterThanOrEqual(1);
        const youOption = page.getByRole("listbox").getByRole("option", { name: "You" });
        const badgeTexts = await youOption.locator("div").allInnerTexts();
        expect(badgeTexts.some((t) => /^[A-Z]{1,2}$/.test(t.trim()))).toBe(true);
      });

      await test.step("search matches names and excludes guest members", async () => {
        await driver.pickerSearch("Parity");
        const options = await driver.pickerOptionTexts();
        expect(options.some((o) => o.includes("You"))).toBe(true);
        // The guest is a project member yet never offered as an assignee.
        await driver.pickerSearch("Parity Guest");
        expect(await driver.pickerOptionTexts()).toEqual([]);
        expect(await driver.pickerEmptyText()).toContain("No matching results");
        await driver.pickerPressEscape();
        expect(await driver.pickerSearchValue()).toBe("");
        await driver.pickerClickOutside();
        expect(await driver.pickerOpen()).toBe(false);
      });

      await test.step("options match the non-guest project members server-side", async () => {
        const members = await serverProjectMembers(seed.workspaceSlug, seed.projectId, session);
        const directory = await serverWorkspaceMembers(seed.workspaceSlug, session);
        const nameOf = (userId: string): string =>
          userId === me.id ? "You" : (directory.find((m) => m.userId === userId)?.displayName ?? userId);
        const expected = members.filter((m) => m.role !== 5).map((m) => nameOf(m.userId));
        await driver.propertyOpenPicker("Assignees");
        const options = await driver.pickerOptionTexts();
        expect(options.length).toBe(expected.length);
        for (const name of expected) expect(options.some((o) => o.includes(name))).toBe(true);
        await driver.pickerClickOutside();
      });

      await test.step("multi-select stays open across picks and persists server-side", async () => {
        await serverPatchIssue(seed.workspaceSlug, seed.projectId, issue.id, { assignee_ids: [] }, session);
        await driver.propertyOpenPicker("Assignees");
        await driver.pickerPick("You");
        expect(await driver.pickerOpen()).toBe(true);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, seed.projectId, issue.id, session)).assignee_ids, {
            timeout: 15_000,
          })
          .toEqual([me.id]);
        // The row renders the display name (avatar initial + full name); the
        // "You" marker lives only inside the picker options, proven above.
        await expect.poll(() => driver.propertyValueText("Assignees"), { timeout: 15_000 }).toContain(me.displayName);
        await driver.pickerPick("You");
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, seed.projectId, issue.id, session)).assignee_ids, {
            timeout: 15_000,
          })
          .toEqual([]);
        await driver.pickerClickOutside();
        expect(await driver.pickerOpen()).toBe(false);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

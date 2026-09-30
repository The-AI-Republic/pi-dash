// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the project switcher, the project header button and
// copy-location (NEWFRONT-126). The tab strip header offers a dropdown of
// joined projects that lands on each project's stored default tab, a
// truncating header with a role-gated quick-actions menu, and a copy entry
// that writes the active tab route to the clipboard with a notice. The
// strip only mounts in tabbed project-list mode, so these scenarios enable
// it through the server preference first and restore it after. Rows:
// SHELL-072, SHELL-073, SHELL-074.
import { test, expect } from "../../fixtures";
import {
  createProject,
  deleteProject,
  getWorkspaceUserProperties,
  patchWorkspaceUserProperties,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-072", "SHELL-073", "SHELL-074"];

const SECOND_NAME = "Chrome Tabs Switch Target With A Deliberately Very Long Name";

test(
  specTitle(ROWS, "project switcher, header actions and copy location"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const originalProps = await getWorkspaceUserProperties(seed.workspaceSlug, session);
    const originalMode = originalProps["navigation_control_preference"] as string | undefined;

    await test.step("enable tabbed mode for the strip", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      await expect.poll(() => driver.projectHeaderText(), { timeout: 30_000 }).not.toBeNull();
    });

    await test.step("the switcher lists joined projects and lands on the default tab", async () => {
      const created = await createProject(seed.workspaceSlug, session, { name: SECOND_NAME, identifier: "CTS" });
      const secondId = created["id"] as string;
      try {
        await driver.openProjectSwitcher();
        await expect
          .poll(() => driver.switcherOptionNames(), { timeout: 15_000 })
          .toEqual(expect.arrayContaining([seed.projectName, SECOND_NAME]));
        await driver.chooseSwitcherOption(SECOND_NAME);
        await expect.poll(() => driver.page.url(), { timeout: 15_000 }).toContain(secondId);
        expect(await driver.projectHeaderText()).toContain(SECOND_NAME);

        await test.step("long names truncate with full text on hover", async () => {
          expect(await driver.projectHeaderTruncated()).toBe(true);
          const header = driver.page.locator("main main").locator('button[aria-haspopup="listbox"]').first();
          const before = await driver.page.getByText(SECOND_NAME, { exact: false }).count();
          await header.hover();
          await expect
            .poll(() => driver.page.getByText(SECOND_NAME, { exact: false }).count(), { timeout: 15_000 })
            .toBeGreaterThan(before);
        });
      } finally {
        await deleteProject(seed.workspaceSlug, secondId, session);
      }
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
    });

    await test.step("the header truncates with full text on hover beside a role-gated menu", async () => {
      expect(await driver.projectHeaderText()).toContain(seed.projectName);
      await driver.openProjectActions();
      const entries = await driver.projectActionNames();
      expect(entries).toEqual(expect.arrayContaining(["Publish project", "Copy link", "Archives", "Settings"]));
      expect(entries).not.toContain("Leave project");
      await driver.page.keyboard.press("Escape");
    });

    await test.step("copy-location writes the active tab route with a notice", async () => {
      await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
      await driver.openProjectActions();
      await driver.clickProjectAction("Copy link");
      await expect
        .poll(() => driver.readClipboardText(), { timeout: 15_000 })
        .toBe(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`);
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/copied/i);
    });

    await test.step("restore the original rendering mode", async () => {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: originalMode ?? "ACCORDION",
      });
      await expect
        .poll(
          async () => (await getWorkspaceUserProperties(seed.workspaceSlug, session))["navigation_control_preference"],
          {
            timeout: 15_000,
          }
        )
        .toBe(originalMode ?? "ACCORDION");
    });
  }
);

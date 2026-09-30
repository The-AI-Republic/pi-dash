// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette — debounced, project-scoped server search (NEWFRONT-127).
// Row: SHELL-084 (a ~half-second idle pause fires one request; inside a
// project only its hits return until the footer scope toggle widens to the
// workspace; the results heading pulses mid-flight).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("command palette server search", () => {
  test.beforeEach(async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-084"], "typing fires one debounced, project-scoped request that the scope toggle can widen"),
    { tag: specTags(["SHELL-084"]) },
    async ({ driver, seed }) => {
      await driver.pressPaletteOpenChord();
      await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);

      await test.step("a burst of keystrokes coalesces into a single request", async () => {
        const count = await driver.countSearchRequests(async () => {
          await driver.typeInCommandPalette("Parity");
          await driver.page.waitForTimeout(900);
        });
        expect(count).toBe(1);
      });

      await test.step("the request is scoped to the current project", async () => {
        const params = await driver.lastSearchRequestParams();
        expect(params).not.toBeNull();
        expect(params?.["search"]).toBe("Parity");
        expect(params?.["project_id"]).toBe(seed.projectId);
        expect(params?.["workspace_search"]).toBe("false");
      });

      await test.step("a results heading is shown for the server search", async () => {
        await expect.poll(() => driver.paletteSearchResultsHeading()).not.toBeNull();
      });

      await test.step("the footer scope toggle is available inside a project", async () => {
        expect(await driver.paletteHasWorkspaceLevelToggle()).toBe(true);
        expect(await driver.isWorkspaceLevelToggleEnabled()).toBe(true);
      });

      await test.step("widening to workspace level re-issues the search unscoped", async () => {
        const count = await driver.countSearchRequests(async () => {
          await driver.toggleWorkspaceLevel();
          // nudge the query so the effect re-runs with the new scope
          await driver.typeInCommandPalette("!");
          await driver.page.waitForTimeout(900);
        });
        expect(count).toBeGreaterThanOrEqual(1);
        const params = await driver.lastSearchRequestParams();
        expect(params?.["workspace_search"]).toBe("true");
      });
    }
  );
});

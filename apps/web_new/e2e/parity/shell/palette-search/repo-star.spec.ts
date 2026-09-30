// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Repository-star action in the top bar (NEWFRONT-127). Row: SHELL-103
// (a theme-adaptive icon that opens the repository externally with safe
// link attributes).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("repository star action", () => {
  test.beforeEach(async ({ driver, seed }) => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-103"], "the repo-star link opens the repository externally with safe attributes"),
    { tag: specTags(["SHELL-103"]) },
    async ({ driver }) => {
      const attrs = await driver.repoStarLinkAttributes();
      expect(attrs).not.toBeNull();
      expect(attrs?.href).toContain("github.com/The-AI-Republic/pi-dash");
      expect(attrs?.target).toBe("_blank");
      expect(attrs?.rel).toContain("noopener");
      expect(attrs?.rel).toContain("noreferrer");
    }
  );

  test(
    specTitle(["SHELL-103"], "the star icon uses a theme-adaptive asset"),
    { tag: specTags(["SHELL-103"]) },
    async ({ driver }) => {
      const src = await driver.repoStarIconSrc();
      expect(src).not.toBeNull();
      expect(src).toMatch(/github-(white|black)\.png/);
    }
  );
});

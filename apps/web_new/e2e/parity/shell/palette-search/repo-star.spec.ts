// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository-star action in the top bar (NEWFRONT-127). Row: SHELL-103
// (a theme-adaptive icon that opens the repository externally with safe
// link attributes).
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

test.describe("repository star action", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-103"], "the repo-star link opens the repository externally with safe attributes"),
    { tag: specTags(["SHELL-103"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
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
      test.setTimeout(300_000);
      const src = await driver.repoStarIconSrc();
      expect(src).not.toBeNull();
      expect(src).toMatch(/github-(white|black)\.png/);
    }
  );
});

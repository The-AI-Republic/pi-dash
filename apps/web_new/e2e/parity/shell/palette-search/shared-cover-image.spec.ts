// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cover-image absence (NEWFRONT-127). Row: SHELL-105.
// Target alignment (NEWFRONT-295 gate, main#523): project cards
// carry no cover slot, so no request for cover data leaves the
// shell — no cover image, placeholder, shimmer, or default art
// renders anywhere in the shell, even when the server still stores
// a cover reference. No oracle: the old app still renders covers.
import { test, expect } from "../../fixtures";
import { serverProjectCover, serverUpdateProjectCover, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const REMOTE_COVER = "https://images.unsplash.com/photo-1493514789931-586cb221d7a7";

test.describe("cover-image absence", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
  });

  test(
    specTitle(["SHELL-105"], "cards render no cover slot with no stored cover"),
    { tag: specTags(["SHELL-105"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const projectsPath = `/${seed.workspaceSlug}/projects`;

      await test.step("cards render no cover slot with no stored cover", async () => {
        await serverUpdateProjectCover(seed.workspaceSlug, seed.projectId, session, null);
        expect(await serverProjectCover(seed.workspaceSlug, seed.projectId, session)).toBeNull();
        await driver.goToPath(projectsPath);
        await driver.awaitProjectCard(seed.projectName);
        expect(await driver.cardHasCoverImage(seed.projectName)).toBe(false);
        expect(await driver.projectCardCoverShimmerVisible()).toBe(false);
      });
    }
  );

  test(
    specTitle(["SHELL-105"], "a stored cover reference still renders no cover"),
    { tag: specTags(["SHELL-105"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const projectsPath = `/${seed.workspaceSlug}/projects`;

      await test.step("a stored cover reference still renders no cover", async () => {
        await serverUpdateProjectCover(seed.workspaceSlug, seed.projectId, session, REMOTE_COVER);
        expect(await serverProjectCover(seed.workspaceSlug, seed.projectId, session)).toBe(REMOTE_COVER);
        await driver.goToPath(projectsPath);
        await driver.awaitProjectCard(seed.projectName);
        // The stored reference round-trips on the server but the shell
        // renders nothing for it: no image, no placeholder, no shimmer.
        expect(await driver.cardHasCoverImage(seed.projectName)).toBe(false);
        expect(await driver.projectCardCoverShimmerVisible()).toBe(false);
        await serverUpdateProjectCover(seed.workspaceSlug, seed.projectId, session, null);
      });
    }
  );
});

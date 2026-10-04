// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Cover-image primitive (NEWFRONT-127). Row: SHELL-105. Project cards render
// their cover slot through one primitive: a missing source shows the
// shimmer placeholder (cards do not opt into default art); a stored remote
// URL renders as-is; clearing the stored reference returns the slot to the
// shimmer. The stored reference round-trips through the server.
import { test, expect } from "../../fixtures";
import { serverProjectCover, serverUpdateProjectCover, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const REMOTE_COVER = "https://images.unsplash.com/photo-1493514789931-586cb221d7a7";

test.describe("cover-image primitive", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
  });

  // Three tests, not one: each cover state needs its own goToPath plus
  // a 120s card poll (suite17 proved the projects route can take 63s to
  // boot under shared-host contention), and suite18 starved the third
  // step after slow hooks plus two 58s steps — the test timeout cut its
  // poll short with the cards not yet rendered. Same split precedent as
  // the 092 guest/archived pair. The empty-cover shimmer is a steady
  // placeholder (the card uses the shared primitive without default art),
  // so a direct toEqual([null]) is sound — no not-empty pre-poll needed.
  test(
    specTitle(["SHELL-105"], "a project without cover art shows the shimmer placeholder"),
    { tag: specTags(["SHELL-105"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const projectsPath = `/${seed.workspaceSlug}/projects`;

      await test.step("a project without cover art shows the shimmer placeholder", async () => {
        await serverUpdateProjectCover(seed.workspaceSlug, seed.projectId, session, null);
        expect(await serverProjectCover(seed.workspaceSlug, seed.projectId, session)).toBeNull();
        await driver.goToPath(projectsPath);
        await expect.poll(() => driver.projectCardCoverSrcs(), { timeout: 120_000 }).toEqual([null]);
        expect(await driver.projectCardCoverShimmerVisible()).toBe(true);
      });
    }
  );

  test(
    specTitle(["SHELL-105"], "a stored remote URL renders as-is on the card"),
    { tag: specTags(["SHELL-105"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const projectsPath = `/${seed.workspaceSlug}/projects`;

      await test.step("a stored remote URL renders as-is on the card", async () => {
        await serverUpdateProjectCover(seed.workspaceSlug, seed.projectId, session, REMOTE_COVER);
        expect(await serverProjectCover(seed.workspaceSlug, seed.projectId, session)).toBe(REMOTE_COVER);
        await driver.goToPath(projectsPath);
        await expect.poll(() => driver.projectCardCoverSrcs(), { timeout: 120_000 }).toEqual([REMOTE_COVER]);
        expect(await driver.projectCardCoverShimmerVisible()).toBe(false);
      });
    }
  );

  test(
    specTitle(["SHELL-105"], "clearing the reference returns the slot to the shimmer"),
    { tag: specTags(["SHELL-105"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const projectsPath = `/${seed.workspaceSlug}/projects`;

      await test.step("clearing the reference returns the slot to the shimmer", async () => {
        await serverUpdateProjectCover(seed.workspaceSlug, seed.projectId, session, null);
        expect(await serverProjectCover(seed.workspaceSlug, seed.projectId, session)).toBeNull();
        await driver.goToPath(projectsPath);
        await expect.poll(() => driver.projectCardCoverSrcs(), { timeout: 120_000 }).toEqual([null]);
        expect(await driver.projectCardCoverShimmerVisible()).toBe(true);
      });
    }
  );
});

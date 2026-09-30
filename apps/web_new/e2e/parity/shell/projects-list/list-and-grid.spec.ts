// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity scenarios (NEWFRONT-124): the projects list fetches the workspace
// collection and renders a responsive card grid, with skeletons while it
// loads. Green on apps/web through drivers/web first. Rows: SHELL-024
// (collection fetch + responsive grid), SHELL-025 (loading skeletons).
import { test, expect } from "../../fixtures";
import {
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  projectsDetails,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const GRID = ["SHELL-024"];
const SKELETON = ["SHELL-025"];

test(
  specTitle(GRID, "projects list renders every member project in a responsive grid"),
  { tag: specTags(GRID) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-grid");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `Grid WS ${suffix}`, slug: `grid-${suffix}` });
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);

    const names = ["Grid Alpha", "Grid Bravo", "Grid Charlie"];
    await createProjectViaApi(owner, ws.slug, { name: names[0]!, identifier: "GRDA" });
    await createProjectViaApi(owner, ws.slug, { name: names[1]!, identifier: "GRDB" });
    await createProjectViaApi(owner, ws.slug, { name: names[2]!, identifier: "GRDC" });

    await test.step("every project the server reports shows as a card", async () => {
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard(names[0]!);
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(expect.arrayContaining(names));

      const server = (await projectsDetails(owner, ws.slug)).map((p) => p.name);
      expect(new Set(server)).toEqual(new Set(names));
    });

    await test.step("grid widens from one to three columns on larger screens", async () => {
      await driver.setViewportWidth(500);
      await expect.poll(() => driver.gridColumnCount()).toBe(1);
      await driver.setViewportWidth(820);
      await expect.poll(() => driver.gridColumnCount()).toBe(2);
      await driver.setViewportWidth(1320);
      await expect.poll(() => driver.gridColumnCount()).toBe(3);
    });
  }
);

test(
  specTitle(SKELETON, "the list shows skeleton cards while the collection resolves"),
  { tag: specTags(SKELETON) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-skel");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `Skel WS ${suffix}`, slug: `skel-${suffix}` });
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);
    await createProjectViaApi(owner, ws.slug, { name: "Skeleton Only", identifier: "SKL1" });

    // Throttle the collection read so the shimmer is observable before cards.
    await driver.page.route("**/projects/details/**", async (route) => {
      await new Promise((r) => setTimeout(r, 3000));
      await route.continue();
    });

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));

    await test.step("shimmer cards show first, then the real card, never an empty flash", async () => {
      expect(await driver.isProjectsSkeletonVisible()).toBe(true);
      await driver.awaitProjectCard("Skeleton Only");
      await expect.poll(() => driver.visibleProjectCardNames()).toContain("Skeleton Only");
    });
  }
);

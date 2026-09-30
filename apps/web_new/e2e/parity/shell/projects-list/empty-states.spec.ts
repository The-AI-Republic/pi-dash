// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity scenarios (NEWFRONT-124): the projects-list empty states. Rows:
// SHELL-026 (first-run empty state with role-gated creation), SHELL-027
// (no-match vs empty-archive stay distinct). Green on apps/web first.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const EMPTY = ["SHELL-026"];
const DISTINCT = ["SHELL-027"];

test(
  specTitle(EMPTY, "first-run empty state explains projects and gates creation by role"),
  { tag: specTags(EMPTY) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-empty");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `Empty WS ${suffix}`, slug: `empty-${suffix}` });
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);

    await test.step("admin sees the empty state with an enabled create action", async () => {
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await expect.poll(() => driver.emptyStateHeading()).toBe("No active projects");
      expect(await driver.isEmptyStateCreateVisible()).toBe(true);
      expect(await driver.isEmptyStateCreateEnabled()).toBe(true);
    });

    await test.step("a guest sees the same empty state but the action is disabled", async () => {
      const guest = await seatFreshMember(owner, ws.slug, ROLE.GUEST, "parity-empty-guest");
      await markOnboardedForProjects(guest);
      await setLastWorkspaceForProjects(guest, ws.id);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(guest));
      await expect.poll(() => driver.emptyStateHeading()).toBe("No active projects");
      expect(await driver.isEmptyStateCreateVisible()).toBe(true);
      expect(await driver.isEmptyStateCreateEnabled()).toBe(false);
    });
  }
);

test(
  specTitle(DISTINCT, "no-match and empty-archive states stay distinct"),
  { tag: specTags(DISTINCT) },
  async ({ driver }) => {
    const owner = await signUpAuthedSession("parity-distinct");
    const suffix = uniqueSuffixForProjects();
    const ws = await createWorkspaceForProjects(owner, { name: `Distinct WS ${suffix}`, slug: `distinct-${suffix}` });
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);
    await createProjectViaApi(owner, ws.slug, { name: "Real Project", identifier: "REAL" });

    await test.step("a nonsense search shows the no-match variant", async () => {
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Real Project");
      await driver.openListSearch();
      await driver.typeListSearch("zzz-no-such-project-zzz");
      await expect.poll(() => driver.emptyStateHeading()).toBe("No matching results.");
    });

    await test.step("the archive view with nothing archived shows the congratulatory variant", async () => {
      await driver.openArchivedProjects(ws.slug);
      await expect.poll(() => driver.emptyStateHeading()).toBe("No projects archived");
    });
  }
);

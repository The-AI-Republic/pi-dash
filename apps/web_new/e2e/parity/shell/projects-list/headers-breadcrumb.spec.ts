// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity scenarios (NEWFRONT-124): the list headers and breadcrumb. Rows:
// SHELL-043 (desktop header: breadcrumb + search/sort/filter + responsive
// create button, gated by role and archive view), SHELL-044 (mobile header
// splits sort/filter into a touch bar at phone widths), SHELL-045 (breadcrumb
// primitive: terminal crumb is plain text, not a link). Green on apps/web.
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

const DESKTOP = ["SHELL-043"];
const MOBILE = ["SHELL-044"];
const CRUMB = ["SHELL-045"];

async function ownerWithOneProject(prefix: string) {
  const owner = await signUpAuthedSession(prefix);
  const suffix = uniqueSuffixForProjects();
  const ws = await createWorkspaceForProjects(owner, { name: `${prefix} WS ${suffix}`, slug: `${prefix}-${suffix}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  await createProjectViaApi(owner, ws.slug, { name: `${prefix} Card`, identifier: prefix.slice(0, 4).toUpperCase() });
  return { owner, ws };
}

test(
  specTitle(DESKTOP, "the desktop list header gates the create button by role and archive view"),
  { tag: specTags(DESKTOP) },
  async ({ driver }) => {
    const { owner, ws } = await ownerWithOneProject("deskhdr");
    await driver.setViewportWidth(1320);

    await test.step("an admin sees the create button and the Projects breadcrumb", async () => {
      await driver.openAuthenticatedAt(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await expect
        .poll(() => driver.breadcrumbLabels())
        .toEqual(expect.arrayContaining([expect.stringContaining("Projects")]));
      expect(await driver.isHeaderCreateButtonVisible()).toBe(true);
    });

    await test.step("the archive view adds an Archived crumb and hides the create button", async () => {
      await driver.openArchivedProjects(ws.slug);
      await expect
        .poll(() => driver.breadcrumbLabels())
        .toEqual(expect.arrayContaining([expect.stringContaining("Archived")]));
      expect(await driver.isHeaderCreateButtonVisible()).toBe(false);
    });

    await test.step("a guest gets no create button", async () => {
      const guest = await seatFreshMember(owner, ws.slug, ROLE.GUEST, "deskhdr-guest");
      await markOnboardedForProjects(guest);
      await setLastWorkspaceForProjects(guest, ws.id);
      await driver.openAuthenticatedAt(`/${ws.slug}/projects`, browserSessionCookies(guest));
      await driver.awaitProjectCard("deskhdr Card");
      expect(await driver.isHeaderCreateButtonVisible()).toBe(false);
    });
  }
);

test(
  specTitle(MOBILE, "the mobile list header shows a touch sort/filter bar at phone widths"),
  { tag: specTags(MOBILE) },
  async ({ driver }) => {
    const { owner, ws } = await ownerWithOneProject("mobhdr");

    await test.step("phone widths hide the desktop filter row and show the mobile bar", async () => {
      await driver.setViewportWidth(400);
      await driver.openAuthenticatedAt(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard("mobhdr Card");
      expect(await driver.isMobileListHeaderVisible()).toBe(true);
      expect(await driver.isDesktopFilterRowVisible()).toBe(false);
    });
  }
);

test(
  specTitle(CRUMB, "the terminal breadcrumb crumb is plain text, not a link"),
  { tag: specTags(CRUMB) },
  async ({ driver }) => {
    const { owner, ws } = await ownerWithOneProject("crumb");
    await driver.setViewportWidth(1320);

    await test.step("the archive view's terminal Archived crumb renders as text", async () => {
      await driver.openAuthenticatedAt(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.openArchivedProjects(ws.slug);
      await expect
        .poll(() => driver.breadcrumbLabels())
        .toEqual(expect.arrayContaining([expect.stringContaining("Archived")]));
      expect(await driver.breadcrumbTerminalIsLink()).toBe(false);
    });
  }
);

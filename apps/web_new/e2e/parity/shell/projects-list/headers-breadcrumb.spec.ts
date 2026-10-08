// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-124): the list headers and breadcrumb. Rows:
// SHELL-043 (desktop header: breadcrumb + search/sort/filter + responsive
// create button, gated by role and archive view), SHELL-044 (mobile header
// splits sort/filter into a touch bar at phone widths), SHELL-045 (breadcrumb
// primitive: terminal crumb is plain text, not a link). Green on apps/web.
import { test, expect } from "../../fixtures";
import {
  NETWORK,
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
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await expect
        .poll(() => driver.breadcrumbLabels())
        .toEqual(expect.arrayContaining([expect.stringContaining("Projects")]));
      await expect.poll(() => driver.isHeaderCreateButtonVisible()).toBe(true);
    });

    await test.step("the archive view adds an Archived crumb and hides the create button", async () => {
      await driver.openArchivedProjects(ws.slug);
      await expect
        .poll(() => driver.breadcrumbLabels())
        .toEqual(expect.arrayContaining([expect.stringContaining("Archived")]));
      await expect.poll(() => driver.isHeaderCreateButtonVisible()).toBe(false);
    });

    await test.step("a guest gets no create button", async () => {
      const guest = await seatFreshMember(owner, ws.slug, ROLE.GUEST, "deskhdr-guest");
      await markOnboardedForProjects(guest);
      await setLastWorkspaceForProjects(guest, ws.id);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(guest));
      // Readiness: the guest lands on cards or the empty state depending on
      // project membership; either way the create button must stay hidden.
      await expect
        .poll(
          async () => (await driver.visibleProjectCardNames()).length > 0 || (await driver.emptyStateHeading()) !== null
        )
        .toBe(true);
      await expect.poll(() => driver.isHeaderCreateButtonVisible()).toBe(false);
    });

    await test.step("the create button label shortens on small screens", async () => {
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.setViewportWidth(1320);
      await expect.poll(() => driver.headerCreateButtonLabel()).toContain("Add Project");
      await driver.setViewportWidth(500);
      await expect.poll(() => driver.headerCreateButtonLabel()).toBe("Project");
    });
  }
);

test(
  specTitle(MOBILE, "the mobile list header shows a touch sort/filter bar at phone widths"),
  { tag: specTags(MOBILE) },
  async ({ driver }) => {
    const { owner, ws } = await ownerWithOneProject("mobhdr");
    await createProjectViaApi(owner, ws.slug, {
      name: "mobhdr Zed",
      identifier: "MZED",
      network: NETWORK.PRIVATE,
    });

    await test.step("phone widths hide the desktop filter row and show the mobile bar", async () => {
      await driver.setViewportWidth(400);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard("mobhdr Card");
      await expect.poll(() => driver.isMobileListHeaderVisible()).toBe(true);
      await expect.poll(() => driver.isDesktopFilterRowVisible()).toBe(false);
    });

    await test.step("the mobile bar sorts the list", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Name");
      await expect.poll(() => driver.currentSortLabel()).toContain("Name");
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["mobhdr Card", "mobhdr Zed"]);
    });

    await test.step("the mobile bar filters the list", async () => {
      await driver.openFilterMenu();
      await driver.selectFilterOption("Private");
      await driver.closeMenu();
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["mobhdr Zed"]);
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
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.openArchivedProjects(ws.slug);
      await expect
        .poll(() => driver.breadcrumbLabels())
        .toEqual(expect.arrayContaining([expect.stringContaining("Archived")]));
      await expect.poll(() => driver.breadcrumbTerminalIsLink()).toBe(false);
    });
  }
);

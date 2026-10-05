// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity scenarios (NEWFRONT-124): sort, filter, applied-filter strip and
// live search on the projects list. Rows: SHELL-028 (sort keys + direction,
// manual disables direction), SHELL-029 (filter panel + value search +
// active badge), SHELL-030 (applied-filter pills + match count + clear all),
// SHELL-031 (search expand, live filter, escape/outside-click dismissal).
import { test, expect } from "../../fixtures";
import {
  NETWORK,
  ROLE,
  addProjectMembersViaApi,
  archiveProjectViaApi,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  projectByName,
  seatFreshMember,
  setLastWorkspaceForProjects,
  setProjectLeadViaApi,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const SORT = ["SHELL-028"];
const FILTER = ["SHELL-029"];
const STRIP = ["SHELL-030"];
const SEARCH = ["SHELL-031"];

async function ownerWithProjects(
  prefix: string,
  projects: Array<{ name: string; identifier: string; network?: number }>
) {
  const owner = await signUpAuthedSession(prefix);
  const suffix = uniqueSuffixForProjects();
  const ws = await createWorkspaceForProjects(owner, { name: `${prefix} WS ${suffix}`, slug: `${prefix}-${suffix}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  for (const p of projects) await createProjectViaApi(owner, ws.slug, p);
  return { owner, ws };
}

test(
  specTitle(SORT, "sorting reorders projects and manual order disables direction"),
  { tag: specTags(SORT) },
  async ({ driver }) => {
    const { owner, ws } = await ownerWithProjects("sort", [
      { name: "Zephyr", identifier: "ZEP" },
      { name: "Alpine", identifier: "ALP" },
      { name: "Meadow", identifier: "MEA", network: NETWORK.PRIVATE },
    ]);
    // Alpine Twin carries an extra member so member-count sorting can separate it.
    const mate = await seatFreshMember(owner, ws.slug, ROLE.MEMBER, "sort-mate");
    const alpineId = await createProjectViaApi(owner, ws.slug, { name: "Alpine Twin", identifier: "ALT" });
    await addProjectMembersViaApi(owner, ws.slug, alpineId, [{ member_id: mate.userId, role: ROLE.MEMBER }]);

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Alpine");

    await test.step("sorting by name orders the cards alphabetically", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Name");
      await expect.poll(() => driver.currentSortLabel()).toContain("Name");
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Alpine", "Alpine Twin", "Meadow", "Zephyr"]);
    });

    await test.step("flipping the direction reverses the name order and back", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Descending");
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Zephyr", "Meadow", "Alpine Twin", "Alpine"]);
      await driver.openSortMenu();
      await driver.selectSortOption("Ascending");
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Alpine", "Alpine Twin", "Meadow", "Zephyr"]);
    });

    await test.step("sorting by creation date follows creation order", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Created date");
      await expect.poll(() => driver.currentSortLabel()).toContain("Created date");
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Zephyr", "Alpine", "Meadow", "Alpine Twin"]);
    });

    await test.step("sorting by member count puts the two-member project last", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Number of members");
      await expect.poll(() => driver.currentSortLabel()).toContain("Number of members");
      await expect.poll(async () => (await driver.visibleProjectCardNames()).slice(-1)).toEqual(["Alpine Twin"]);
    });

    await test.step("manual order disables the direction toggle", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Manual");
      await driver.openSortMenu();
      await expect.poll(() => driver.isSortDirectionDisabled()).toBe(true);
      await driver.closeMenu();
    });

    await test.step("the chosen sort survives filtering the list", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Name");
      await driver.openFilterMenu();
      await driver.selectFilterOption("Private");
      await driver.closeMenu();
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Meadow"]);
      await expect.poll(() => driver.currentSortLabel()).toContain("Name");
    });
  }
);

test(
  specTitle(FILTER, "the filter panel narrows projects and flags active filters"),
  { tag: specTags(FILTER) },
  async ({ driver }) => {
    const { owner, ws } = await ownerWithProjects("filter", [
      { name: "Public One", identifier: "PUB1", network: NETWORK.PUBLIC },
      { name: "Secret One", identifier: "SEC1", network: NETWORK.PRIVATE },
    ]);

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Public One");

    await test.step("an internal search finds a filter value and applying it narrows the grid", async () => {
      await driver.openFilterMenu();
      await driver.typeFilterSearch("riv");
      await expect.poll(() => driver.filterMenuHasOption("Private")).toBe(true);
      await driver.selectFilterOption("Private");
      await driver.closeMenu();
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Secret One"]);
    });

    await test.step("bug: NEWFRONT-172 a capitalized query hides its match (one-sided case fold)", async () => {
      // The oracle lowercases the query but not the option labels, so "Priv"
      // matches nothing while "riv" does. Intended: case-insensitive match.
      await driver.openFilterMenu();
      await driver.typeFilterSearch("Priv");
      await expect.poll(() => driver.filterMenuHasOption("Private")).toBe(false);
      await expect.poll(() => driver.filterMenuHasOption("No matches found")).toBe(true);
      await driver.closeMenu();
    });

    await test.step("the filter trigger shows its active badge", async () => {
      await expect.poll(() => driver.isFilterBadgeVisible()).toBe(true);
    });

    await test.step("lead plus date selections combine", async () => {
      // Lead Probe is led by the owner; the other two have no lead. A date
      // filter alone keeps all three, adding the lead narrows to the led one.
      const publicId = await createProjectViaApi(owner, ws.slug, { name: "Lead Probe", identifier: "LPRB" });
      await setProjectLeadViaApi(owner, ws.slug, publicId, owner.userId);
      await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
      await driver.awaitProjectCard("Lead Probe");
      await expect
        .poll(() => driver.visibleProjectCardNames())
        .toEqual(expect.arrayContaining(["Public One", "Secret One", "Lead Probe"]));
      await driver.openFilterMenu();
      await driver.selectFilterOption("Last 7 days");
      await driver.closeMenu();
      await expect
        .poll(() => driver.visibleProjectCardNames())
        .toEqual(expect.arrayContaining(["Public One", "Secret One", "Lead Probe"]));
      await driver.openFilterMenu();
      await driver.selectFilterOption("You");
      await driver.closeMenu();
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Lead Probe"]);
    });
  }
);

test(
  specTitle(STRIP, "the applied-filter strip shows pills, a match count and clears"),
  { tag: specTags(STRIP) },
  async ({ driver }) => {
    const { owner, ws } = await ownerWithProjects("strip", [
      { name: "Strip Public", identifier: "STP", network: NETWORK.PUBLIC },
      { name: "Strip Secret", identifier: "STS", network: NETWORK.PRIVATE },
    ]);

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Strip Public");

    await test.step("applying two filters shows pills and a match count", async () => {
      await driver.openFilterMenu();
      await driver.selectFilterOption("Private");
      await driver.selectFilterOption("My projects");
      await driver.closeMenu();
      await expect
        .poll(() => driver.appliedFilterChipTexts())
        .toEqual(expect.arrayContaining([expect.stringContaining("Private")]));
      await expect
        .poll(() => driver.appliedFilterChipTexts())
        .toEqual(expect.arrayContaining([expect.stringContaining("My projects")]));
      await expect.poll(() => driver.filterMatchCountText()).toBe("1/2");
    });

    await test.step("removing one chip updates the count", async () => {
      await driver.removeAppliedFilterChip("Private");
      await expect.poll(() => driver.filterMatchCountText()).toBe("2/2");
      await driver.removeAppliedFilterChip("My projects");
      await expect.poll(() => driver.appliedFilterChipTexts()).toEqual([]);
    });

    await test.step("clear all restores the full list", async () => {
      await driver.openFilterMenu();
      await driver.selectFilterOption("Private");
      await driver.closeMenu();
      await expect.poll(() => driver.filterMatchCountText()).toBe("1/2");
      await driver.clickClearAllFilters();
      await expect
        .poll(() => driver.visibleProjectCardNames())
        .toEqual(expect.arrayContaining(["Strip Public", "Strip Secret"]));
    });

    await test.step("clear all on the archive view preserves the archive toggle", async () => {
      const secret = await projectByName(owner, ws.slug, "Strip Secret");
      await archiveProjectViaApi(owner, ws.slug, secret!.id);
      await driver.openArchivedProjects(ws.slug);
      await driver.awaitProjectCard("Strip Secret");
      await driver.openFilterMenu();
      await driver.selectFilterOption("Private");
      await driver.closeMenu();
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Strip Secret"]);
      await driver.clickClearAllFilters();
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Strip Secret"]);
      expect(await driver.currentUrlPath()).toContain("/archives");
    });
  }
);

test(
  specTitle(SEARCH, "list search expands, filters live and dismisses by escape"),
  { tag: specTags(SEARCH) },
  async ({ driver }) => {
    const { owner, ws } = await ownerWithProjects("search", [
      { name: "Findable Apple", identifier: "FAP" },
      { name: "Hidden Banana", identifier: "HBA" },
    ]);

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Findable Apple");

    await test.step("typing narrows the grid live", async () => {
      await driver.openListSearch();
      await expect.poll(() => driver.isListSearchExpanded()).toBe(true);
      await driver.typeListSearch("Apple");
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Findable Apple"]);
    });

    await test.step("first escape clears the text, second collapses the field", async () => {
      await driver.pressEscapeInListSearch();
      await expect.poll(() => driver.listSearchValue()).toBe("");
      await driver.pressEscapeInListSearch();
      await expect.poll(() => driver.isListSearchExpanded()).toBe(false);
    });

    await test.step("an outside click collapses only when the field is empty", async () => {
      await driver.openListSearch();
      await expect.poll(() => driver.isListSearchExpanded()).toBe(true);
      await driver.clickOutsideListSearch();
      await expect.poll(() => driver.isListSearchExpanded()).toBe(false);
      await driver.openListSearch();
      await driver.typeListSearch("Apple");
      await expect.poll(() => driver.visibleProjectCardNames()).toEqual(["Findable Apple"]);
      await driver.clickOutsideListSearch();
      await expect.poll(() => driver.isListSearchExpanded()).toBe(true);
      await expect.poll(() => driver.listSearchValue()).toBe("Apple");
    });

    await test.step("the clear button resets the text and collapses the field", async () => {
      await driver.clickListSearchClear();
      await expect.poll(() => driver.listSearchValue()).toBe("");
      await expect.poll(() => driver.isListSearchExpanded()).toBe(false);
      await expect
        .poll(() => driver.visibleProjectCardNames())
        .toEqual(expect.arrayContaining(["Findable Apple", "Hidden Banana"]));
    });
  }
);

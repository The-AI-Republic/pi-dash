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
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  setLastWorkspaceForProjects,
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
      { name: "Meadow", identifier: "MEA" },
    ]);

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Alpine");

    await test.step("sorting by name orders the cards alphabetically", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Name");
      await expect.poll(() => driver.currentSortLabel()).toContain("Name");
      await expect
        .poll(async () => (await driver.visibleProjectCardNames()).slice(0, 3))
        .toEqual(["Alpine", "Meadow", "Zephyr"]);
    });

    await test.step("manual order disables the direction toggle", async () => {
      await driver.openSortMenu();
      await driver.selectSortOption("Manual");
      await driver.openSortMenu();
      await expect.poll(() => driver.isSortDirectionDisabled()).toBe(true);
      await driver.closeMenu();
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

    await test.step("applying a filter shows a removable pill and a match count", async () => {
      await driver.openFilterMenu();
      await driver.selectFilterOption("Private");
      await driver.closeMenu();
      await expect
        .poll(() => driver.appliedFilterChipTexts())
        .toEqual(expect.arrayContaining([expect.stringContaining("Private")]));
      await expect.poll(() => driver.filterMatchCountText()).toBe("1/2");
    });

    await test.step("clear all restores the full list", async () => {
      await driver.clickClearAllFilters();
      await expect
        .poll(() => driver.visibleProjectCardNames())
        .toEqual(expect.arrayContaining(["Strip Public", "Strip Secret"]));
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
  }
);

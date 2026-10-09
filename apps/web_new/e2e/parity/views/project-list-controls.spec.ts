// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): the project views list controls — the
// expandable search field, the sort menu, the filters menu and the
// applied-filter chips. Rows: VIEW-004, VIEW-005, VIEW-006, VIEW-007.
// Green on apps/web first.
import { test, expect } from "../fixtures";
import { createProjectViewFull, patchProjectView, serverSavedViews } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenListAs } from "./support";

const POLL = { timeout: 60_000 };

test(
  specTitle(["VIEW-004"], "expandable search filters by name with Escape and outside-click collapse"),
  { tag: specTags(["VIEW-004"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v4");
    const { owner, workspaceSlug, projectId } = harness;
    const first = `V4 Alpha ${harness.tag}`;
    const second = `V4 Beta ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: first });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: second });

    await test.step("collapsed search shows only its trigger", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([first, second]));
      expect(await driver.viewsSearchTriggerVisible()).toBe(true);
      expect(await driver.viewsSearchExpanded()).toBe(false);
    });

    await test.step("opening focuses the field; typing filters by substring", async () => {
      await driver.viewsSearchOpen();
      expect(await driver.viewsSearchExpanded()).toBe(true);
      expect(await driver.viewsSearchFocused()).toBe(true);
      await driver.viewsSearchType("Alpha");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([first]);
    });

    await test.step("first Escape clears the query, second collapses the field", async () => {
      await driver.viewsSearchEscape();
      await expect.poll(() => driver.viewsSearchValue(), POLL).toBe("");
      expect(await driver.viewsSearchExpanded()).toBe(true);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([first, second]));
      await driver.viewsSearchEscape();
      // The collapse animates the field width; poll past the transition.
      await expect.poll(() => driver.viewsSearchExpanded(), POLL).toBe(false);
      await expect.poll(() => driver.viewsSearchTriggerVisible(), POLL).toBe(true);
    });

    await test.step("outside click collapses the empty field but keeps a query open", async () => {
      await driver.viewsSearchOpen();
      await driver.viewsSearchClickOutside();
      await expect.poll(() => driver.viewsSearchExpanded(), POLL).toBe(false);
      expect(driver.page.url()).toContain("/views");
      await driver.viewsSearchOpen();
      await driver.viewsSearchType("Beta");
      await driver.viewsSearchClickOutside();
      expect(await driver.viewsSearchExpanded()).toBe(true);
      expect(await driver.viewsSearchValue()).toBe("Beta");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([second]);
    });
  }
);

test(
  specTitle(["VIEW-005"], "sort by name and timestamps in both directions"),
  { tag: specTags(["VIEW-005"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v5");
    const { owner, workspaceSlug, projectId } = harness;
    // Created out of alphabetical order with separated stamps so every key
    // and direction has a deterministic expectation.
    const zebra = `V5 Zebra ${harness.tag}`;
    const mango = `V5 Mango ${harness.tag}`;
    const apple = `V5 Apple ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: zebra });
    await new Promise((resolve) => setTimeout(resolve, 1100));
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: mango });
    await new Promise((resolve) => setTimeout(resolve, 1100));
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: apple });

    await test.step("default order is updated-at descending", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([apple, mango, zebra]);
      expect(await driver.viewsSortTriggerText()).toBe("Updated at");
    });

    await test.step("the menu offers keys and directions with checkmarks", async () => {
      await driver.viewsSortOpen();
      expect(await driver.viewsSortMenuTexts()).toEqual([
        "Name",
        "Created at",
        "Updated at",
        "Ascending",
        "Descending",
      ]);
      expect(await driver.viewsSortMenuSelected("Updated at")).toBe(true);
      expect(await driver.viewsSortMenuSelected("Descending")).toBe(true);
      expect(await driver.viewsSortMenuSelected("Name")).toBe(false);
      await driver.viewsSortPick("Name");
    });

    await test.step("name sort reorders immediately in both directions", async () => {
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([zebra, mango, apple]);
      expect(await driver.viewsSortTriggerText()).toBe("Name");
      await driver.viewsSortOpen();
      await driver.viewsSortPick("Ascending");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([apple, mango, zebra]);
    });

    await test.step("timestamp sorts follow creation and update order", async () => {
      await driver.viewsSortOpen();
      await driver.viewsSortPick("Created at");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([zebra, mango, apple]);
      await driver.viewsSortOpen();
      await driver.viewsSortPick("Descending");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([apple, mango, zebra]);
      await driver.viewsSortOpen();
      expect(await driver.viewsSortMenuSelected("Descending")).toBe(true);
      expect(await driver.viewsSortMenuSelected("Created at")).toBe(true);
      await driver.viewsSortPick("Updated at");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([apple, mango, zebra]);
    });

    await test.step("an update moves its view first under updated-at descending", async () => {
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      const mangoId = server.find((view) => view.name === mango)?.id ?? "";
      expect(mangoId).not.toBe("");
      await patchProjectView(workspaceSlug, projectId, mangoId, owner.cookie, { name: `${mango} v2` });
      await driver.reloadPage();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([`${mango} v2`, apple, zebra]);
    });
  }
);

test(
  specTitle(["VIEW-005"], "sort selection is session-local and resets on reload"),
  { tag: specTags(["VIEW-005"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v5b");
    const { owner, workspaceSlug, projectId } = harness;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: `V5b B ${harness.tag}` });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: `V5b A ${harness.tag}` });

    await test.step("pick name sort", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toHaveLength(2);
      await driver.viewsSortOpen();
      await driver.viewsSortPick("Name");
      await expect.poll(() => driver.viewsSortTriggerText(), POLL).toBe("Name");
    });

    await test.step("reload resets the sort to its default", async () => {
      await driver.reloadPage();
      await expect.poll(() => driver.viewsListNames(), POLL).toHaveLength(2);
      expect(await driver.viewsSortTriggerText()).toBe("Updated at");
    });
  }
);

test(
  specTitle(["VIEW-006"], "filter by favorites, creation date and creator; access is cloud-only"),
  { tag: specTags(["VIEW-006"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v6");
    const { owner, workspaceSlug, projectId } = harness;
    const starred = `V6 Starred ${harness.tag}`;
    const plain = `V6 Plain ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: starred });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: plain });

    await test.step("the menu offers favorites, date and creator but no access section", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
      await driver.viewsFiltersOpen();
      const panel = await driver.viewsFiltersPanelText();
      expect(panel).toContain("Favorites");
      expect(panel).toContain("Created date");
      expect(panel).toContain("Created by");
      expect(await driver.viewsFiltersDateOptions()).toEqual(["1 week ago", "2 weeks ago", "1 month ago", "Custom"]);
      expect(await driver.viewsFiltersAccessPresent()).toBe(false);
      await driver.viewsFiltersClose();
    });

    await test.step("the favorites toggle keeps only starred views", async () => {
      await driver.viewsToggleStar(starred);
      await expect.poll(() => driver.viewsRowStarSelected(starred), POLL).toBe(true);
      await driver.viewsFiltersOpen();
      await driver.viewsFiltersToggleFavorites();
      await driver.viewsFiltersClose();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([starred]);
      await driver.viewsChipsClearAll();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
    });

    await test.step("a relative date filter excludes freshly created views", async () => {
      await driver.viewsFiltersOpen();
      await driver.viewsFiltersPickDate("1 month ago");
      await expect.poll(() => driver.viewsNoMatchTitle(), POLL).toBe("No matching results.");
      expect(await driver.viewsListNames()).toEqual([]);
      await driver.viewsFiltersClose();
      await driver.viewsChipsClearAll();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
    });

    await test.step("the creator filter keeps the picked member's views", async () => {
      await driver.viewsFiltersOpen();
      const options = await driver.viewsFiltersCreatorOptions();
      expect(options.join(" ")).toContain("You");
      await driver.viewsFiltersPickCreator("You");
      await driver.viewsFiltersClose();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server).toHaveLength(2);
    });
  }
);

test(
  specTitle(["VIEW-006"], "filters menu search narrows options; selections combine with name search"),
  { tag: specTags(["VIEW-006"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v6b");
    const { owner, workspaceSlug, projectId } = harness;
    const first = `V6b Alpha ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: first });

    await test.step("menu search narrows the date options", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([first]);
      await driver.viewsFiltersOpen();
      await driver.viewsFiltersSearchType("zzz-no-such-option");
      await expect.poll(() => driver.viewsFiltersPanelText(), POLL).toContain("No matches found");
      await driver.viewsFiltersClose();
    });

    await test.step("a picked filter combines with the name search", async () => {
      await driver.viewsToggleStar(first);
      await expect.poll(() => driver.viewsRowStarSelected(first), POLL).toBe(true);
      await driver.viewsFiltersOpen();
      await driver.viewsFiltersToggleFavorites();
      await driver.viewsFiltersClose();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([first]);
      await driver.viewsSearchOpen();
      await driver.viewsSearchType("zzz-no-such-view");
      await expect.poll(() => driver.viewsNoMatchTitle(), POLL).toBe("No matching results.");
      expect(await driver.viewsListNames()).toEqual([]);
    });
  }
);

test(
  specTitle(["VIEW-007"], "applied chips remove per value and hide the strip when empty"),
  { tag: specTags(["VIEW-007"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v7");
    const { owner, workspaceSlug, projectId } = harness;
    const starred = `V7 Starred ${harness.tag}`;
    const plain = `V7 Plain ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: starred });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: plain });

    await test.step("no strip renders while no filter applies", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
      expect(await driver.viewsChipsVisible()).toBe(false);
    });

    await test.step("the favorites chip X clears its dimension", async () => {
      await driver.viewsToggleStar(starred);
      await expect.poll(() => driver.viewsRowStarSelected(starred), POLL).toBe(true);
      await driver.viewsFiltersOpen();
      await driver.viewsFiltersToggleFavorites();
      await driver.viewsFiltersClose();
      await expect.poll(() => driver.viewsChipsVisible(), POLL).toBe(true);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([starred]);
      await driver.viewsChipRemoveDimension("favorites");
      expect(await driver.viewsChipsVisible()).toBe(false);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
    });

    await test.step("the date value X clears its dimension", async () => {
      await driver.viewsFiltersOpen();
      await driver.viewsFiltersPickDate("1 month ago");
      await driver.viewsFiltersClose();
      await expect.poll(() => driver.viewsChipsVisible(), POLL).toBe(true);
      const chips = await driver.viewsChipTexts();
      expect(chips.join(" | ").toLowerCase()).toContain("created at");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([]);
      await driver.viewsChipRemoveValue("created at", "1 month ago");
      expect(await driver.viewsChipsVisible()).toBe(false);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server).toHaveLength(2);
    });
  }
);

test(
  specTitle(["VIEW-007"], "bug: NEWFRONT-216 removing one dimension chip wipes sibling filters"),
  { tag: specTags(["VIEW-007"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v7b");
    const { owner, workspaceSlug, projectId } = harness;
    const starred = `V7b Starred ${harness.tag}`;
    const plain = `V7b Plain ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: starred });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: plain });

    await test.step("two dimensions combine and render two chips", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
      await driver.viewsToggleStar(starred);
      await expect.poll(() => driver.viewsRowStarSelected(starred), POLL).toBe(true);
      await driver.viewsFiltersOpen();
      await driver.viewsFiltersToggleFavorites();
      await driver.viewsFiltersPickDate("1 month ago");
      await driver.viewsFiltersClose();
      await expect.poll(() => driver.viewsChipsVisible(), POLL).toBe(true);
      const chips = await driver.viewsChipTexts();
      expect(chips.join(" | ").toLowerCase()).toContain("favorites");
      expect(chips.join(" | ").toLowerCase()).toContain("created at");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([]);
    });

    // Intended: dropping the date dimension keeps favorites (starred-only
    // list, favorites chip kept). The app wipes the sibling filter instead.
    await test.step("bug: dropping the date dimension also clears favorites", async () => {
      await driver.viewsChipRemoveDimension("created at");
      expect(await driver.viewsChipsVisible()).toBe(false);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([starred, plain]));
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server).toHaveLength(2);
    });
  }
);

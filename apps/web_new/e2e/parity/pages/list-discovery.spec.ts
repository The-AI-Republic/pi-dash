// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-312): pages list discovery — the project
// feature switch gate, visibility tabs, expanding title search, the sort
// control, structured filters with chips, and list row anatomy with
// click/right-click open. Rows: PAGE-001–PAGE-006.
import { test, expect } from "../fixtures";
import {
  currentUser,
  listFavorites,
  parityProjectIdentifier,
  requireMentionMember,
  serverAddProjectMembers,
  serverArchivePage,
  serverCleanupProject,
  serverCreatePage,
  serverCreateProjectWithFlags,
  serverFavoritePage,
  serverListPages,
  serverPatchProject,
  serverProjectPageView,
  serverRenamePage,
  serverSetPageAccess,
  signInSessionWithRetry,
  type PagesListServerRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

/** Project MEMBER role (backend Project.MEMBER); the seed owner is admin. */
const PROJECT_MEMBER_ROLE = 15;

/** Unique area tag so scratch projects and page names never collide. */
function tag(area: string): string {
  return `NF312 ${area} ${Date.now()}`;
}

/** Idempotent setup writes retry: the API 400/401s transiently under stack load. */
async function setupRetry<T>(label: string, fn: () => Promise<T>, attempts = 4): Promise<T> {
  let error: unknown = null;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      return await fn();
    } catch (caught) {
      error = caught;
      await new Promise((resolve) => setTimeout(resolve, 2_000 * attempt));
    }
  }
  throw new Error(
    `[parity] setup step never landed (${label}): ${error instanceof Error ? error.message : String(error)}`
  );
}

/** Scratch project for one scenario (deleted with the project in `finally`). */
async function scratchProject(seed: ParitySeedFacts, area: string, session: string): Promise<string> {
  // Area feeds the project name, which rejects special characters
  // (hyphens included): keep it to letters and digits.
  // Creation 400s transiently under stack load (identifier mint collisions
  // across parallel runs); a fresh mint retries with backoff.
  return setupRetry(
    `create project ${area}`,
    () =>
      serverCreateProjectWithFlags(
        seed.workspaceSlug,
        `${tag(area)} project`,
        parityProjectIdentifier("N312"),
        {},
        session
      ),
    6
  );
}

/** Best-effort cleanup that cannot hang the scenario past `timeoutMs`. */
async function cleanupProject(
  seed: ParitySeedFacts,
  projectId: string,
  session: string,
  timeoutMs = 30_000
): Promise<void> {
  await Promise.race([
    serverCleanupProject(seed.workspaceSlug, projectId, session).catch(() => {}),
    new Promise((resolve) => setTimeout(resolve, timeoutMs)),
  ]);
}

/** Display title the list shows for a server row (blank reads back untitled). */
function displayTitle(row: PagesListServerRow): string {
  return row.name.trim() === "" ? "Untitled" : row.name;
}

/** Server rows ordered the way the list sorts them (name lowercased, ISO dates). */
function orderRows(rows: PagesListServerRow[], key: "name" | "createdAt" | "updatedAt", dir: "asc" | "desc"): string[] {
  const sorted = [...rows].sort((a, b) => {
    const left = key === "name" ? a.name.toLowerCase() : key === "createdAt" ? a.createdAt : a.updatedAt;
    const right = key === "name" ? b.name.toLowerCase() : key === "createdAt" ? b.createdAt : b.updatedAt;
    return left < right ? -1 : left > right ? 1 : 0;
  });
  if (dir === "desc") sorted.reverse();
  return sorted.map(displayTitle);
}

test(
  specTitle(["PAGE-001"], "feature switch gates the list and the admin shortcut opens features"),
  { tag: specTags(["PAGE-001"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectOn = await scratchProject(seed, "gateon", session);
    const projectOff = await scratchProject(seed, "gateoff", session);
    try {
      const name = `${tag("gate")} page`;
      await serverCreatePage(seed.workspaceSlug, projectOn, session, name);
      await serverCreatePage(seed.workspaceSlug, projectOff, session, `${tag("gate")} hidden`);
      await setupRetry("patch page_view off", () =>
        serverPatchProject(seed.workspaceSlug, projectOff, session, { page_view: false })
      );

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);

      // Feature on: the list shows with its tab strip and rows.
      expect(await serverProjectPageView(seed.workspaceSlug, projectOn, session)).toBe(true);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectOn);
      expect(await driver.pagesListGateVisible()).toBe(false);
      expect(await driver.pagesListTabsVisible()).toBe(true);
      expect(await driver.pagesListRowTitles()).toEqual([name]);

      // Feature off: guidance replaces the list; the shortcut is enabled
      // for admins and lands on project features.
      expect(await serverProjectPageView(seed.workspaceSlug, projectOff, session)).toBe(false);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectOff);
      expect(await driver.pagesListGateVisible()).toBe(true);
      expect(await driver.pagesListTabsVisible()).toBe(false);
      expect(await driver.pagesListRowTitles()).toEqual([]);
      expect(await driver.pagesListGateManageEnabled()).toBe(true);
      await driver.pagesListGateOpenManage();

      // The switch gates the UI only: the API still serves the pages.
      const rows = await serverListPages(seed.workspaceSlug, projectOff, session);
      expect(rows.length).toBe(1);
    } finally {
      await cleanupProject(seed, projectOn, session);
      await cleanupProject(seed, projectOff, session);
    }
  }
);

test(
  specTitle(["PAGE-001"], "feature-off guidance disables its shortcut for members"),
  { tag: specTags(["PAGE-001"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const projectId = await scratchProject(seed, "gatemember", session);
    try {
      await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("gate")} hidden`);
      await setupRetry("patch page_view off", () =>
        serverPatchProject(seed.workspaceSlug, projectId, session, { page_view: false })
      );
      await setupRetry("add member", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_MEMBER_ROLE }],
          session
        )
      );

      await driver.openEntry();
      await driver.signInWithPassword(mention.email, mention.password);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);
      expect(await driver.pagesListGateVisible()).toBe(true);
      expect(await driver.pagesListGateManageEnabled()).toBe(false);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-002"], "visibility tabs partition the list through a shareable query value"),
  { tag: specTags(["PAGE-002"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "tabs", session);
    try {
      const stamp = tag("tab");
      const openA = `${stamp} open A`;
      const openB = `${stamp} open B`;
      await serverCreatePage(seed.workspaceSlug, projectId, session, openA);
      await serverCreatePage(seed.workspaceSlug, projectId, session, openB);
      const restrictedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} restricted`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, restrictedId, 1, session)
      );
      const shelvedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} shelved`);
      await setupRetry("archive page", () => serverArchivePage(seed.workspaceSlug, projectId, shelvedId, session));
      const serverRows = await serverListPages(seed.workspaceSlug, projectId, session, true);
      const openRows = serverRows.filter((row) => row.access === 0 && row.archivedAt === null);
      const restrictedRows = serverRows.filter((row) => row.access === 1 && row.archivedAt === null);
      const shelvedRows = serverRows.filter((row) => row.archivedAt !== null);
      expect(openRows.length).toBe(2);
      expect(restrictedRows.length).toBe(1);
      expect(shelvedRows.length).toBe(1);
      const openTitles = orderRows(openRows, "updatedAt", "desc");

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);

      // Missing query value lands on the open tab.
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);
      expect(await driver.pagesListTabs()).toEqual(["Public", "Private", "Archived"]);
      expect(await driver.pagesListActiveTab()).toBe("public");
      expect(await driver.pagesListMarkedTab()).toBe("public");
      expect(await driver.pagesListRowTitles()).toEqual(openTitles);
      for (const tab of ["public", "private", "archived"] as const) {
        expect(await driver.pagesListTabHref(tab)).toContain(`type=${tab}`);
      }

      // Switching swaps the row set and the mark follows.
      await driver.pagesListClickTab("private");
      expect(await driver.pagesListActiveTab()).toBe("private");
      expect(await driver.pagesListMarkedTab()).toBe("private");
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(restrictedRows, "updatedAt", "desc"));
      await driver.pagesListClickTab("archived");
      expect(await driver.pagesListActiveTab()).toBe("archived");
      expect(await driver.pagesListMarkedTab()).toBe("archived");
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(shelvedRows, "updatedAt", "desc"));
      await driver.pagesListClickTab("public");
      expect(await driver.pagesListRowTitles()).toEqual(openTitles);

      // Re-selecting the active tab is a no-op.
      const before = await driver.pagesListRowTitles();
      await driver.pagesListClickTab("public");
      expect(await driver.pagesListActiveTab()).toBe("public");
      expect(await driver.pagesListRowTitles()).toEqual(before);

      // An unknown query value lands on the open tab.
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId, "?type=bogus");
      expect(await driver.pagesListActiveTab()).toBe("public");
      expect(await driver.pagesListMarkedTab()).toBe("public");
      expect(await driver.pagesListRowTitles()).toEqual(openTitles);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-002"], "tab strip appears once pages exist"),
  { tag: specTags(["PAGE-002"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "tabsempty", session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);
      expect(await driver.pagesListTabsVisible()).toBe(false);
      expect(await driver.pagesListRowTitles()).toEqual([]);

      const name = `${tag("tabs")} first`;
      await serverCreatePage(seed.workspaceSlug, projectId, session, name);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);
      expect(await driver.pagesListTabsVisible()).toBe(true);
      expect(await driver.pagesListRowTitles()).toEqual([name]);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-003"], "expanding search narrows rows live with escape and click-away collapse"),
  { tag: specTags(["PAGE-003"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "search", session);
    try {
      const stamp = tag("find");
      const alpha = `${stamp} Alpha`;
      const alpine = `${stamp} Alpine`;
      const beta = `${stamp} Beta`;
      await serverCreatePage(seed.workspaceSlug, projectId, session, alpha);
      await serverCreatePage(seed.workspaceSlug, projectId, session, alpine);
      await serverCreatePage(seed.workspaceSlug, projectId, session, beta);
      const serverRows = await serverListPages(seed.workspaceSlug, projectId, session);
      const matching = (query: string): string[] =>
        orderRows(
          serverRows.filter((row) => displayTitle(row).toLowerCase().includes(query.toLowerCase())),
          "updatedAt",
          "desc"
        );

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);
      expect(await driver.pagesListSearchExpanded()).toBe(false);

      // Collapsed control expands; typing narrows rows live (no submit).
      await driver.pagesListSearchOpen();
      expect(await driver.pagesListSearchExpanded()).toBe(true);
      expect(await driver.pagesListSearchValue()).toBe("");
      await driver.pagesListSearchType("alp");
      expect(await driver.pagesListSearchValue()).toBe("alp");
      expect(await driver.pagesListRowTitles()).toEqual(matching("alp"));
      expect(await driver.pagesListNoResultsVisible()).toBe(false);

      // Zero matches show the no-results state.
      await driver.pagesListSearchType("zzz-no-such-page");
      expect(await driver.pagesListRowTitles()).toEqual([]);
      expect(await driver.pagesListNoResultsVisible()).toBe(true);

      // Escape clears text first (rows restore, stays expanded), then
      // collapses when empty.
      await driver.pagesListSearchEscape();
      expect(await driver.pagesListSearchValue()).toBe("");
      expect(await driver.pagesListSearchExpanded()).toBe(true);
      expect(await driver.pagesListRowTitles()).toEqual(matching(""));
      expect(await driver.pagesListNoResultsVisible()).toBe(false);
      await driver.pagesListSearchEscape();
      expect(await driver.pagesListSearchExpanded()).toBe(false);

      // Clicking away collapses only when empty.
      await driver.pagesListSearchOpen();
      await driver.pagesListSearchType("beta");
      await driver.pagesListClickAway();
      expect(await driver.pagesListSearchExpanded()).toBe(true);
      expect(await driver.pagesListSearchValue()).toBe("beta");
      expect(await driver.pagesListRowTitles()).toEqual(matching("beta"));
      await driver.pagesListSearchClear();
      expect(await driver.pagesListSearchExpanded()).toBe(false);
      expect(await driver.pagesListSearchValue()).toBe(null);
      expect(await driver.pagesListRowTitles()).toEqual(matching(""));
      await driver.pagesListSearchOpen();
      await driver.pagesListClickAway();
      expect(await driver.pagesListSearchExpanded()).toBe(false);

      // Search is client-side: the server list never changed.
      const after = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(after.map((row) => row.id).sort()).toEqual(serverRows.map((row) => row.id).sort());
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-004"], "sort control reorders rows immediately per key and direction"),
  { tag: specTags(["PAGE-004"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "sort", session);
    try {
      const stamp = tag("order");
      const idA = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} Charlie`);
      await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} Alpha`);
      await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} Bravo`);
      // Rename bumps Charlie's modification date to newest; expectations
      // derive from the server rows, so the new name needs no round-trip.
      await setupRetry("rename", () =>
        serverRenamePage(seed.workspaceSlug, projectId, idA, `${stamp} Charlie II`, session)
      );
      const serverRows = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(serverRows.length).toBe(3);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);

      // Default: modification date, newest first.
      expect(await driver.pagesListSortState()).toEqual({ label: "Date modified", descending: true });
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "updatedAt", "desc"));

      // Each key plus direction visibly re-sorts immediately.
      await driver.pagesListSortChooseKey("Name");
      expect(await driver.pagesListSortState()).toEqual({ label: "Name", descending: true });
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "name", "desc"));
      await driver.pagesListSortChooseDirection("Ascending");
      expect(await driver.pagesListSortState()).toEqual({ label: "Name", descending: false });
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "name", "asc"));
      await driver.pagesListSortChooseKey("Date created");
      expect(await driver.pagesListSortState()).toEqual({ label: "Date created", descending: false });
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "createdAt", "asc"));
      await driver.pagesListSortChooseDirection("Descending");
      expect(await driver.pagesListSortState()).toEqual({ label: "Date created", descending: true });
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "createdAt", "desc"));
      await driver.pagesListSortChooseKey("Date modified");
      expect(await driver.pagesListSortState()).toEqual({ label: "Date modified", descending: true });
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "updatedAt", "desc"));
      await driver.pagesListSortChooseDirection("Ascending");
      expect(await driver.pagesListSortState()).toEqual({ label: "Date modified", descending: false });
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "updatedAt", "asc"));

      // Re-choosing the current direction changes nothing.
      await driver.pagesListSortChooseDirection("Ascending");
      expect(await driver.pagesListSortState()).toEqual({ label: "Date modified", descending: false });
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "updatedAt", "asc"));

      // Sort is client-side: the server list never changed.
      const after = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(after.map((row) => row.id).sort()).toEqual(serverRows.map((row) => row.id).sort());
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-005"], "structured filters narrow rows with removable chips and clear-all"),
  { tag: specTags(["PAGE-005"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const mentionSession = await signInSessionWithRetry(mention.email, mention.password);
    const projectId = await scratchProject(seed, "filters", session);
    try {
      await setupRetry("add member", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_MEMBER_ROLE }],
          session
        )
      );
      const stamp = tag("filter");
      const mine = `${stamp} mine`;
      const mineId = await serverCreatePage(seed.workspaceSlug, projectId, session, mine);
      await setupRetry("favorite page", () => serverFavoritePage(seed.workspaceSlug, projectId, mineId, mine, session));
      const plain = `${stamp} plain`;
      await serverCreatePage(seed.workspaceSlug, projectId, session, plain);
      const theirs = `${stamp} theirs`;
      await serverCreatePage(seed.workspaceSlug, projectId, mentionSession, theirs);
      const serverRows = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(serverRows.length).toBe(3);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);
      expect(await driver.pagesListChips()).toEqual([]);

      // Favorites-only narrows to the starred page; removing the chip restores rows.
      await driver.pagesListFiltersOpen();
      await driver.pagesListFilterToggleFavorites();
      expect(await driver.pagesListRowTitles()).toEqual([mine]);
      // Chip text renders capitalized (CSS transform applies to innerText).
      expect(await driver.pagesListChips()).toContain("Favorites");
      await driver.pagesListChipRemove("favorites");
      expect(await driver.pagesListChips()).toEqual([]);
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "updatedAt", "desc"));

      // Creation-date conditions filter; fresh pages match nothing old.
      await driver.pagesListFiltersOpen();
      await driver.pagesListFilterChooseDate("1 week ago");
      expect(await driver.pagesListRowTitles()).toEqual([]);
      expect(await driver.pagesListNoResultsVisible()).toBe(true);
      const dateChips = await driver.pagesListChips();
      expect(dateChips.join(" ")).toContain("Created At");
      expect(dateChips.join(" ")).toContain("1 week ago");
      await driver.pagesListClearAll();
      expect(await driver.pagesListChips()).toEqual([]);
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "updatedAt", "desc"));

      // The creator picker lists workspace members and narrows by search.
      await driver.pagesListFiltersOpen();
      const options = await driver.pagesListFilterCreatorOptions();
      expect(options).toContain("You");
      expect(options).toContain(mention.displayName);
      await driver.pagesListFilterSearch(mention.displayName);
      expect(await driver.pagesListFilterCreatorOptions()).toEqual([mention.displayName]);
      await driver.pagesListFilterSearch("");
      await driver.pagesListFilterPickCreator(mention.displayName);
      expect(await driver.pagesListRowTitles()).toEqual([theirs]);
      expect((await driver.pagesListChips()).join(" ")).toContain("Created By");
      await driver.pagesListClearAll();
      expect(await driver.pagesListRowTitles()).toEqual(orderRows(serverRows, "updatedAt", "desc"));

      // Filtering is client-side: favorites and ownership never changed.
      const favorites = await listFavorites(seed.workspaceSlug, session);
      expect(favorites.some((row) => row.entity_type === "page" && row.entity_identifier === mineId)).toBe(true);
      const after = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(after.find((row) => row.name === theirs)?.ownedBy).toBe(mention.id);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-006"], "rows show anatomy, open on click, and mirror the menu on right-click"),
  { tag: specTags(["PAGE-006"]) },
  async ({ driver, seed, page }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "rows", session);
    try {
      const stamp = tag("row");
      const named = `${stamp} named`;
      const namedId = await serverCreatePage(seed.workspaceSlug, projectId, session, named);
      await setupRetry("favorite page", () =>
        serverFavoritePage(seed.workspaceSlug, projectId, namedId, named, session)
      );
      await serverCreatePage(seed.workspaceSlug, projectId, session, " ");
      const privateId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} private`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, privateId, 1, session)
      );
      const serverRows = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(serverRows.length).toBe(3);
      const ownerName = (await currentUser(session)).display_name ?? "";

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);

      // Each row shows its mark, title (with untitled fallback), owner,
      // visibility, creation date, star, and overflow menu.
      const namedDetail = await driver.pagesListRowDetail(named);
      expect(namedDetail.markOffered).toBe(true);
      expect(namedDetail.owner).toBe(ownerName);
      expect(namedDetail.visibility).toBe("Public");
      expect(namedDetail.createdOn?.startsWith("Created on")).toBe(true);
      expect(namedDetail.starOffered).toBe(true);
      expect(namedDetail.starred).toBe(true);
      expect(namedDetail.menuOffered).toBe(true);
      const untitledTitles = await driver.pagesListRowTitles();
      expect(untitledTitles).toContain("Untitled");
      const untitledDetail = await driver.pagesListRowDetail("Untitled");
      expect(untitledDetail.starred).toBe(false);

      // Row click lands on the detail view.
      await driver.pagesListClickRow(named);
      expect(page.url()).toContain(`/pages/${namedId}`);

      // Right-click mirrors the overflow menu.
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId);
      const overflowItems = await driver.pagesListRowMenuItems(named);
      expect(overflowItems.length).toBeGreaterThan(0);
      await driver.pagesListDismissMenus();
      await driver.pagesListRowRightClick(named);
      expect(await driver.pagesListContextMenuItems()).toEqual(overflowItems);
      await driver.pagesListDismissMenus();

      // Viewing never mutated the server list.
      const after = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(after.map((row) => row.id).sort()).toEqual(serverRows.map((row) => row.id).sort());
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-006"], "bug: NEWFRONT-362 private glyph shows no tooltip; owner, mark, star, menu and date hold"),
  { tag: specTags(["PAGE-006"]) },
  async ({ driver, seed }, testInfo) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "rowsbug", session);
    try {
      const title = `${tag("rowbug")} private`;
      const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, title);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, pageId, 1, session)
      );
      const ownerName = (await currentUser(session)).display_name ?? "";

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesListOpenProjectPages(seed.workspaceSlug, projectId, "?type=private");

      const detail = await driver.pagesListRowDetail(title);
      expect(detail.markOffered).toBe(true);
      expect(detail.owner).toBe(ownerName);
      // Observed bug (NEWFRONT-362): the lock glyph never opens its
      // "Private" tooltip — the icon does not forward the trigger ref.
      // Intended: hovering reveals "Private", mirroring public rows.
      // The new app implements the fix (Parity wiki "Old bugs" allows it),
      // so the oracle leg keeps pinning the bug while the new leg pins the fix.
      const fixed = testInfo.project.name === "new";
      expect(detail.visibility).toBe(fixed ? "Private" : null);
      expect(detail.createdOn?.startsWith("Created on")).toBe(true);
      expect(detail.starOffered).toBe(true);
      expect(detail.menuOffered).toBe(true);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

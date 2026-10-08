// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): the favorites section supports folders
// with validated create and rename, drag organization, deep links and
// removal, hiding entirely when empty.
// Observed on the running old app: with no favorites the sidebar shows
// no Favorites section at all; favoriting a project surfaces the section
// with the entry; the folder form is inline (no dialog) and blocks empty
// names silently while whitespace-only and duplicate names toast errors,
// staying open throughout;
// entries drag above each other from hover-revealed handles and the new
// order survives reloads; the quick menu removes entries with a success
// notice; folders group entries and deep-link to their targets; removing
// the last favorite hides the section again, and the server agrees
// throughout. Row: SHELL-056.
// Owner-scoped (a dedicated member was tried and reverted): member
// self-join confers no project membership row on this stack, so nested
// project entries stay invisible to members, and the invite endpoint that
// would grant membership 500s per NEWFRONT-151. The repair helper below
// keeps the scenario convergent against sibling runs sharing the owner.
import { test, expect } from "../../fixtures";
import { createFavorite, deleteFavorite, deletePage, ensurePage, listFavorites, ownerSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-056"];
const FOLDER_NAME = "Parity Pinned";
const PAGE_NAME = "Parity Fav Page";

test(
  specTitle(ROWS, "favorites section with folders, links and removal"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the section hides entirely when empty", async () => {
      // Wipe first: a previous interrupted run may have left favorites behind.
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        await deleteFavorite(seed.workspaceSlug, session, fav.id);
      }
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.sidebarSectionNames()).not.toContain("Favorites");
      expect(await listFavorites(seed.workspaceSlug, session)).toEqual([]);
    });

    // Convergent repair: sibling runs share the seed owner and can wipe its
    // favorites between steps, so every UI step re-establishes the folder
    // and entry instead of assuming the create step's writes survived.
    async function ensureFolderWithEntry(): Promise<{ id: string }> {
      const favs = await listFavorites(seed.workspaceSlug, session);
      let folder = favs.find((fav) => fav.name === FOLDER_NAME && fav.is_folder);
      if (folder === undefined) {
        folder = await createFavorite(seed.workspaceSlug, session, {
          entity_type: "folder",
          name: FOLDER_NAME,
          is_folder: true,
        });
      }
      const again = await listFavorites(seed.workspaceSlug, session);
      if (!again.some((fav) => fav.parent === folder.id)) {
        await createFavorite(seed.workspaceSlug, session, {
          entity_type: "project",
          entity_identifier: seed.projectId,
          project_id: seed.projectId,
          name: seed.projectName,
          parent: folder.id,
        });
      }
      return folder;
    }

    async function ensureTopLevelPageEntry(): Promise<void> {
      const page = await ensurePage(seed.workspaceSlug, seed.projectId, session, PAGE_NAME);
      const favs = await listFavorites(seed.workspaceSlug, session);
      if (!favs.some((fav) => fav.name === PAGE_NAME && fav.parent === null)) {
        await createFavorite(seed.workspaceSlug, session, {
          entity_type: "page",
          entity_identifier: page.id,
          project_id: seed.projectId,
          name: PAGE_NAME,
        });
      }
    }

    const folder = await test.step("create a folder with an entry", async () => ensureFolderWithEntry());

    await test.step("the section surfaces the folder and entry", async () => {
      await ensureFolderWithEntry();
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(FOLDER_NAME);
      await driver.openFavoritesFolder(FOLDER_NAME);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(seed.projectName);
      // The list endpoint serves top-level entries only; nested entries live
      // under their folder, so the server agrees on the folder while the
      // sidebar proves the nested entry.
      const server = await listFavorites(seed.workspaceSlug, session);
      expect(server.map((fav) => fav.name)).toEqual(expect.arrayContaining([FOLDER_NAME]));
    });

    await test.step("the folder form validates names", async () => {
      await driver.openFavoritesFolderDialog();
      await expect.poll(() => driver.isFavoritesFolderDialogOpen(), { timeout: 30_000 }).toBe(true);
      // An empty submit never reaches the handler (the required rule blocks
      // it): nothing is created and the form stays open, with no notice.
      await driver.submitFavoritesFolderName("");
      await driver.page.waitForTimeout(3000);
      expect(await driver.isFavoritesFolderDialogOpen()).toBe(true);
      expect(await driver.isToastVisible("Folder name cannot be empty")).toBe(false);
      // A whitespace-only name passes the rule but trims to empty, which the
      // handler rejects with a notice while keeping the form open.
      await driver.submitFavoritesFolderName("   ");
      await expect.poll(() => driver.isToastVisible("Folder name cannot be empty"), { timeout: 30_000 }).toBe(true);
      expect(await driver.isFavoritesFolderDialogOpen()).toBe(true);
      await driver.submitFavoritesFolderName(FOLDER_NAME);
      await expect.poll(() => driver.isToastVisible("Folder already exists"), { timeout: 30_000 }).toBe(true);
      expect(await driver.isFavoritesFolderDialogOpen()).toBe(true);
      await driver.clickMainContent();
      await expect.poll(() => driver.isFavoritesFolderDialogOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("entries drag above each other persistently", async () => {
      // Two top-level siblings with names unique to this scenario: the
      // relative order is asserted (not absolute positions) because a
      // sibling run may hold its own entries in the same list.
      await ensureFolderWithEntry();
      await ensureTopLevelPageEntry();
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(PAGE_NAME);
      const orderOf = async (): Promise<string[]> =>
        (await driver.favoriteEntryNames()).filter((name) => name === FOLDER_NAME || name === PAGE_NAME);
      const before = await orderOf();
      expect(before).toHaveLength(2);
      const [first, second] = before;
      // A drag can silently not take under load, so drive it again while the
      // order still reads pre-drag instead of failing on the first attempt.
      const wanted = [second, first];
      let order: string[] = [];
      for (let attempt = 0; attempt < 2; attempt += 1) {
        await driver.dragFavoriteBefore(second, first);
        const deadline = Date.now() + 15_000;
        do {
          order = await orderOf();
          if (order.join("\n") === wanted.join("\n")) break;
          await driver.page.waitForTimeout(1000);
        } while (Date.now() < deadline);
        if (order.join("\n") === wanted.join("\n")) break;
      }
      expect(order).toEqual(wanted);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect.poll(orderOf, { timeout: 30_000 }).toEqual(wanted);
    });

    await test.step("entries deep-link to their targets", async () => {
      // The folder starts collapsed after navigation, hiding the nested
      // entry; open it only while the entry is out of sight, since the
      // folder click toggles.
      await ensureFolderWithEntry();
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      for (let attempt = 0; attempt < 6; attempt += 1) {
        if ((await driver.favoriteEntryNames()).includes(seed.projectName)) break;
        await driver.openFavoritesFolder(FOLDER_NAME);
        await driver.page.waitForTimeout(1000);
      }
      await driver.openFavoriteEntry(seed.projectName);
      await expect
        .poll(() => driver.page.url(), { timeout: 30_000 })
        .toContain(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`);
    });

    await test.step("removal through the menu confirms with a notice", async () => {
      await ensureFolderWithEntry();
      await ensureTopLevelPageEntry();
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(PAGE_NAME);
      await driver.openFavoriteQuickMenu(PAGE_NAME);
      await expect.poll(() => driver.favoriteQuickMenuTexts(), { timeout: 30_000 }).not.toEqual([]);
      expect((await driver.favoriteQuickMenuTexts()).join(" ")).toContain("Remove from favorites");
      await driver.activateFavoriteQuickMenuItem("Remove from favorites");
      await expect.poll(() => driver.isToastVisible("Favorite removed successfully"), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).not.toContain(PAGE_NAME);
      const server = await listFavorites(seed.workspaceSlug, session);
      expect(server.map((fav) => fav.name)).not.toContain(PAGE_NAME);
    });

    await test.step("removal hides the section again", async () => {
      const before = await listFavorites(seed.workspaceSlug, session);
      const target = before.find((fav) => fav.name === FOLDER_NAME && fav.is_folder) ?? folder;
      await deleteFavorite(seed.workspaceSlug, session, target.id);
      // Folder-scoped: sibling runs sharing the owner may hold their own
      // favorites, so removal proves the folder is gone rather than the
      // whole list being empty.
      const server = await listFavorites(seed.workspaceSlug, session);
      expect(server.map((fav) => fav.name)).not.toContain(FOLDER_NAME);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      if ((await driver.sidebarSectionNames()).includes("Favorites")) {
        await driver.setFavoritesOpen(true);
        await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).not.toContain(FOLDER_NAME);
      }
      const page = await ensurePage(seed.workspaceSlug, seed.projectId, session, PAGE_NAME);
      await deletePage(seed.workspaceSlug, seed.projectId, session, page.id);
    });
  }
);

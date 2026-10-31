// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-525): favorite entries drop onto a folder to
// nest inside it, and nested entries drag out onto the top list to leave
// the folder, with both moves surviving reloads. Split from the
// NEWFRONT-286 review: SHELL-056 proves sibling reorder only, while the
// running old app changes parents on folder-target drops (PATCH parent)
// and clears them on drag-out (PATCH parent null).
// Observed on the running old app: dropping a top-level entry onto a
// closed folder's center nests it (the folder's grouped read gains the
// entry); dragging a nested entry above a top-level row un-nests it back
// into the top list (the grouped read loses it, the top list gains it).
// Row: SHELL-109.
// Owner-scoped like SHELL-056: the seed owner is shared with sibling
// runs, so every step resets its own uniquely-named rows instead of
// assuming earlier writes survived. Server reads are split: the flat
// list serves top-level rows only, so nesting is read back through the
// folder's grouped read.
import { test, expect } from "../../fixtures";
import {
  createFavorite,
  deleteFavorite,
  deletePage,
  ensurePage,
  listFavoriteGroup,
  listFavorites,
  ownerSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-109"];
const FOLDER_NAME = "Parity Drop Folder";
const PAGE_NAME = "Parity Drop Page";
const NESTED_NAME = "Parity Drop Nested";
const TARGET_NAME = "Parity Drop Target";

test(
  specTitle(ROWS, "favorite entries nest into folders and drag back out"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    // Convergent repair: sibling runs share the seed owner and can wipe
    // its favorites between steps, so every step resets its own rows.
    // All four names are unique to this spec, so the delete-then-create
    // is invisible to sibling runs. A grouped read that 404s means a
    // sibling wiped the folder mid-step; it reads back as empty and the
    // next ensure recreates it.
    async function ensureFolder(): Promise<{ id: string }> {
      const favs = await listFavorites(seed.workspaceSlug, session);
      const folder = favs.find((fav) => fav.name === FOLDER_NAME && fav.is_folder);
      if (folder !== undefined) return folder;
      return createFavorite(seed.workspaceSlug, session, {
        entity_type: "folder",
        name: FOLDER_NAME,
        is_folder: true,
      });
    }

    async function nestedNames(folderId: string): Promise<string[]> {
      const children = await listFavoriteGroup(seed.workspaceSlug, session, folderId).catch(() => []);
      return children.map((fav) => fav.name);
    }

    async function isTopLevel(name: string): Promise<boolean> {
      const favs = await listFavorites(seed.workspaceSlug, session);
      return favs.some((fav) => fav.name === name && fav.parent === null);
    }

    async function resetPageEntry(name: string, parent: string | null, folderId: string): Promise<void> {
      const page = await ensurePage(seed.workspaceSlug, seed.projectId, session, name);
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        if (fav.name === name) await deleteFavorite(seed.workspaceSlug, session, fav.id);
      }
      for (const fav of await listFavoriteGroup(seed.workspaceSlug, session, folderId).catch(() => [])) {
        if (fav.name === name) await deleteFavorite(seed.workspaceSlug, session, fav.id);
      }
      await createFavorite(seed.workspaceSlug, session, {
        entity_type: "page",
        entity_identifier: page.id,
        project_id: seed.projectId,
        name,
        parent,
      });
    }

    await test.step("set up a folder, a nested entry and a loose entry", async () => {
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        if (fav.name === PAGE_NAME || fav.name === NESTED_NAME || fav.name === TARGET_NAME) {
          await deleteFavorite(seed.workspaceSlug, session, fav.id);
        }
      }
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        if (fav.name === FOLDER_NAME) await deleteFavorite(seed.workspaceSlug, session, fav.id);
      }
      const folder = await ensureFolder();
      await resetPageEntry(NESTED_NAME, folder.id, folder.id);
      await resetPageEntry(PAGE_NAME, null, folder.id);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(FOLDER_NAME);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(PAGE_NAME);
    });

    await test.step("entries drop into folders persistently", async () => {
      const folder = await ensureFolder();
      await resetPageEntry(NESTED_NAME, folder.id, folder.id);
      await resetPageEntry(PAGE_NAME, null, folder.id);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(PAGE_NAME);
      // A drag can silently not take under load, so drive it again while
      // the grouped read still lacks the entry. The loop breaks as soon
      // as the entry nests, so a hidden (already nested) row is never
      // dragged.
      for (let attempt = 0; attempt < 3; attempt += 1) {
        if ((await nestedNames(folder.id)).includes(PAGE_NAME)) break;
        await driver.dragFavoriteIntoFolder(PAGE_NAME, FOLDER_NAME);
        const deadline = Date.now() + 15_000;
        while (Date.now() < deadline) {
          if ((await nestedNames(folder.id)).includes(PAGE_NAME)) break;
          await driver.page.waitForTimeout(1000);
        }
      }
      expect(await nestedNames(folder.id)).toContain(PAGE_NAME);
      // The folder starts closed after navigation: the nested entry is
      // out of sight until the folder opens, which proves the nesting
      // in the UI while the grouped read above proves the parent.
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(FOLDER_NAME);
      for (let attempt = 0; attempt < 6; attempt += 1) {
        if (!(await driver.favoriteEntryNames()).includes(PAGE_NAME)) break;
        await driver.openFavoritesFolder(FOLDER_NAME);
        await driver.page.waitForTimeout(1000);
      }
      expect(await driver.favoriteEntryNames()).not.toContain(PAGE_NAME);
      for (let attempt = 0; attempt < 6; attempt += 1) {
        if ((await driver.favoriteEntryNames()).includes(PAGE_NAME)) break;
        await driver.openFavoritesFolder(FOLDER_NAME);
        await driver.page.waitForTimeout(1000);
      }
      expect(await driver.favoriteEntryNames()).toContain(PAGE_NAME);
      expect(await nestedNames(folder.id)).toContain(PAGE_NAME);
    });

    await test.step("nested entries drag out persistently", async () => {
      const folder = await ensureFolder();
      // The page stays nested as the folder-closed marker for the final
      // UI proof; the nested entry is the one dragged out.
      await resetPageEntry(NESTED_NAME, folder.id, folder.id);
      await resetPageEntry(PAGE_NAME, folder.id, folder.id);
      await resetPageEntry(TARGET_NAME, null, folder.id);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(TARGET_NAME);
      for (let attempt = 0; attempt < 6; attempt += 1) {
        if ((await driver.favoriteEntryNames()).includes(NESTED_NAME)) break;
        await driver.openFavoritesFolder(FOLDER_NAME);
        await driver.page.waitForTimeout(1000);
      }
      // A drag can silently not take under load, so drive it again while
      // the top list still lacks the entry.
      for (let attempt = 0; attempt < 3; attempt += 1) {
        if (await isTopLevel(NESTED_NAME)) break;
        await driver.dragFavoriteOutOfFolder(NESTED_NAME, TARGET_NAME);
        const deadline = Date.now() + 15_000;
        while (Date.now() < deadline) {
          if (await isTopLevel(NESTED_NAME)) break;
          await driver.page.waitForTimeout(1000);
        }
      }
      expect(await isTopLevel(NESTED_NAME)).toBe(true);
      expect(await nestedNames(folder.id)).not.toContain(NESTED_NAME);
      // After reload the un-nested entry shows while the folder stays
      // closed around the page, which proves it left the folder in the
      // UI while the server reads above prove the cleared parent.
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      for (let attempt = 0; attempt < 6; attempt += 1) {
        if (!(await driver.favoriteEntryNames()).includes(PAGE_NAME)) break;
        await driver.openFavoritesFolder(FOLDER_NAME);
        await driver.page.waitForTimeout(1000);
      }
      expect(await driver.favoriteEntryNames()).not.toContain(PAGE_NAME);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(NESTED_NAME);
      expect(await isTopLevel(NESTED_NAME)).toBe(true);
    });

    await test.step("clean up the scenario favorites and pages", async () => {
      const folder = (await listFavorites(seed.workspaceSlug, session)).find(
        (fav) => fav.name === FOLDER_NAME && fav.is_folder
      );
      if (folder !== undefined) {
        for (const fav of await listFavoriteGroup(seed.workspaceSlug, session, folder.id).catch(() => [])) {
          if (fav.name === PAGE_NAME || fav.name === NESTED_NAME || fav.name === TARGET_NAME) {
            await deleteFavorite(seed.workspaceSlug, session, fav.id);
          }
        }
      }
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        if (
          fav.name === FOLDER_NAME ||
          fav.name === PAGE_NAME ||
          fav.name === NESTED_NAME ||
          fav.name === TARGET_NAME
        ) {
          await deleteFavorite(seed.workspaceSlug, session, fav.id);
        }
      }
      for (const name of [PAGE_NAME, NESTED_NAME, TARGET_NAME]) {
        const page = await ensurePage(seed.workspaceSlug, seed.projectId, session, name);
        await deletePage(seed.workspaceSlug, seed.projectId, session, page.id);
      }
      const server = await listFavorites(seed.workspaceSlug, session);
      expect(server.map((fav) => fav.name)).not.toContain(FOLDER_NAME);
    });
  }
);

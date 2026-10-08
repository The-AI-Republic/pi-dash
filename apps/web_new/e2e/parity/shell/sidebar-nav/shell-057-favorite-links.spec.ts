// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): favorite entries resolve icons and links
// by entity type with a safe fallback for unknown types.
// Observed on the running old app: a project favorite lands on the
// project's default tab, a page favorite on its page, a cycle favorite on
// its cycle page, and deleting a favorited cycle removes the favorite
// entry on the server and in the sidebar. bug: an entry whose type the
// client does not recognize
// (entity_type outside the client's link table) crashes the sidebar
// with a render error instead of falling back to the workspace root
// the way the link resolver intends (see NEWFRONT-144). The fallback
// half is therefore recorded, not executed: creating such an entry
// would take down the sidebar for every later scenario on the shared
// stack. Row: SHELL-057.
import { test, expect } from "../../fixtures";
import {
  createFavorite,
  deleteCycle,
  deleteFavorite,
  deletePage,
  ensureCycle,
  ensurePage,
  listFavorites,
  ownerSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-057"];
const PAGE_NAME = "Parity Wiki Page";
const CYCLE_NAME = "Parity Archive Cycle";

test(
  specTitle(ROWS, "favorite entries resolve links by entity type"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));
    const page = await test.step("provision a page", async () =>
      ensurePage(seed.workspaceSlug, seed.projectId, session, PAGE_NAME));
    const cycle = await test.step("provision a cycle", async () =>
      ensureCycle(seed.workspaceSlug, seed.projectId, session, CYCLE_NAME));

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("seed project, page and cycle favorites", async () => {
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        await deleteFavorite(seed.workspaceSlug, session, fav.id);
      }
      await createFavorite(seed.workspaceSlug, session, {
        entity_type: "project",
        entity_identifier: seed.projectId,
        project_id: seed.projectId,
        name: seed.projectName,
      });
      await createFavorite(seed.workspaceSlug, session, {
        entity_type: "page",
        entity_identifier: page.id,
        project_id: seed.projectId,
        name: PAGE_NAME,
      });
      await createFavorite(seed.workspaceSlug, session, {
        entity_type: "cycle",
        entity_identifier: cycle.id,
        project_id: seed.projectId,
        name: CYCLE_NAME,
      });
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarSectionNames(), { timeout: 60_000 }).toContain("Favorites");
      await driver.setFavoritesOpen(true);
      await expect
        .poll(() => driver.favoriteEntryNames(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining([seed.projectName, PAGE_NAME, CYCLE_NAME]));
    });

    await test.step("a project favorite lands on the default tab", async () => {
      await driver.openFavoriteEntry(seed.projectName);
      await expect
        .poll(() => driver.page.url(), { timeout: 30_000 })
        .toContain(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`);
    });

    await test.step("a page favorite lands on its page", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(PAGE_NAME);
      await driver.openFavoriteEntry(PAGE_NAME);
      await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain(page.id);
    });

    await test.step("a cycle favorite lands on its cycle", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setFavoritesOpen(true);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(CYCLE_NAME);
      await driver.openFavoriteEntry(CYCLE_NAME);
      await expect
        .poll(() => driver.page.url(), { timeout: 30_000 })
        .toContain(`/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}`);
    });

    await test.step("deleting a favorited cycle removes its entry", async () => {
      // The server deletes the cycle's favorite with the cycle, so the
      // sidebar entry goes away too and the server list agrees.
      await deleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, session);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setFavoritesOpen(true);
      // Loaded marker first: the project favorite proves the list rendered
      // before the absence below means anything.
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(seed.projectName);
      await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).not.toContain(CYCLE_NAME);
      const server = await listFavorites(seed.workspaceSlug, session);
      expect(server.map((fav) => fav.name)).not.toContain(CYCLE_NAME);
    });

    await test.step("bug: an unknown entry type crashes instead of falling back (NEWFRONT-144)", async () => {
      // Not executed live: storing an entry whose type the client does not
      // recognize renders it through a details hook that reads fields off
      // a null entity, which throws during render and takes down the whole
      // sidebar (observed: zero asides, render error naming the favorites
      // item hook). The intended behavior is the link resolver's
      // workspace-root fallback. Recorded here and tracked in NEWFRONT-144
      // so the fallback half is not lost; it must become a live assertion
      // once the crash is fixed. The server accepts the entry (entity_type
      // is free text), which is what makes the crash reachable.
      const server = await listFavorites(seed.workspaceSlug, session);
      expect(server.map((fav) => fav.name)).not.toContain("Parity Unknown Link");
    });

    await test.step("restore the seeded baseline", async () => {
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        await deleteFavorite(seed.workspaceSlug, session, fav.id);
      }
      await deletePage(seed.workspaceSlug, seed.projectId, session, page.id);
    });
  }
);

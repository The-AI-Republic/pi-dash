// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): favorite entries resolve icons and links
// by entity type with a safe fallback for unknown types.
// Observed on the running old app: a project favorite lands on the
// project's default tab, a page favorite on its page, a cycle favorite on
// its cycle page, and deleting a favorited cycle removes the favorite
// entry on the server and in the sidebar.
// Target alignment (NEWFRONT-295 gate): the unknown-type fallback now
// runs live, subsuming NEWFRONT-144 — the old client crashed the whole
// sidebar there (bug, recorded on the SHELL-057 row), while web_new must
// render the entry with the workspace-root link. Row: SHELL-057.
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
const UNKNOWN_NAME = "Parity Unknown Link";

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

    await test.step("unknown entry types fall back to the workspace root (NEWFRONT-144)", async () => {
      // The favorites API stores any entity_type string and serializes an
      // unrecognized one as entity_data: null. The old client read details
      // off that null during render and took the whole sidebar down; the
      // recorded bug: step below pins that on the oracle. web_new must
      // render the row with the stored name and the workspace-root link.
      const unknown = await createFavorite(seed.workspaceSlug, session, {
        entity_type: "dashboard",
        name: UNKNOWN_NAME,
      });
      try {
        // The API stores the unknown type as-is and serializes no entity
        // payload for it, which is what makes the fallback reachable.
        expect(unknown.entity_type).toBe("dashboard");
        expect(unknown.entity_data).toBeNull();
        if (driver.target !== "web_new") {
          // The old app must not be loaded with this row: bug NEWFRONT-144,
          // its details hook reads off the null entity during render and the
          // error boundary takes the whole sidebar down. Intended (asserted
          // for web_new below, and recorded on the SHELL-057 row): render
          // the entry with the workspace-root link instead of crashing.
          return;
        }
        await driver.openWorkspaceHome(seed.workspaceSlug);
        await driver.setFavoritesOpen(true);
        await expect.poll(() => driver.favoriteEntryNames(), { timeout: 30_000 }).toContain(UNKNOWN_NAME);
        expect(await driver.favoriteEntryHref(UNKNOWN_NAME)).toBe(`/${seed.workspaceSlug}`);
        // The sidebar survives: its built-in rows still render beside it.
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 30_000 }).toContain("Home");
      } finally {
        await deleteFavorite(seed.workspaceSlug, session, unknown.id);
      }
    });

    await test.step("restore the seeded baseline", async () => {
      for (const fav of await listFavorites(seed.workspaceSlug, session)) {
        await deleteFavorite(seed.workspaceSlug, session, fav.id);
      }
      await deletePage(seed.workspaceSlug, seed.projectId, session, page.id);
    });
  }
);

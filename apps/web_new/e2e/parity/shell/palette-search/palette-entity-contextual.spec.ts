// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette entity-contextual entries (NEWFRONT-127). Row: SHELL-091. Opening
// the palette on a cycle, module or page detail surfaces a contextual group
// whose entries follow per-entity capabilities: cycles offer a gated
// favorite toggle plus an always-visible copy-URL; modules add gated member
// and status management; pages offer lock, visibility, archive and favorite
// entries each gated by its own capability flag. One test per entity kind
// (each detail route compiles its own chunk on demand, so sharing one test
// budget would flake on cold servers); entities are created through the API
// and deleted afterwards, and the cycle favorite toggle is proven against
// the server.
import { test, expect } from "../../fixtures";
import {
  serverCreateCycle,
  serverCreateModule,
  serverCreatePage,
  serverCycleIsFavorite,
  serverDeleteCycle,
  serverDeleteModule,
  serverDeletePage,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

// HEAD's serverCreateCycle takes explicit start/end dates (YYYY-MM-DD); cover
// "now" so the cycle reads as current on the detail page.
function cycleDates(): [string, string] {
  const start = new Date();
  const end = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000);
  return [start.toISOString().slice(0, 10), end.toISOString().slice(0, 10)];
}

test.describe("palette entity-contextual entries", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
  });

  test(
    specTitle(["SHELL-091"], "the cycle detail palette offers a favorite toggle plus copy-URL"),
    { tag: specTags(["SHELL-091"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const cycleName = `Parity cycle ${Date.now()}`;
      const cycleId = await serverCreateCycle(seed.workspaceSlug, seed.projectId, cycleName, ...cycleDates(), session);
      try {
        await test.step("the cycle palette offers a favorite toggle plus copy-URL", async () => {
          await driver.goToPath(`/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycleId}`);
          await expect.poll(() => driver.hasVisibleText(cycleName), { timeout: 120_000 }).toBe(true);
          await driver.pressPaletteOpenChord();
          await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
          expect(await driver.paletteHasCommand("Add to favorites")).toBe(true);
          expect(await driver.paletteHasCommand("Copy URL")).toBe(true);
        });

        await test.step("the cycle favorite toggle persists to the server", async () => {
          await driver.activatePaletteCommand("Add to favorites");
          await expect
            .poll(() => serverCycleIsFavorite(seed.workspaceSlug, seed.projectId, cycleId, session), {
              timeout: 30_000,
            })
            .toBe(true);
        });
      } finally {
        await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, session);
      }
    }
  );

  test(
    specTitle(["SHELL-091"], "the module detail palette offers member, status, favorite and copy-URL entries"),
    { tag: specTags(["SHELL-091"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const moduleName = `Parity module ${Date.now()}`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, seed.projectId, moduleName, session);
      try {
        await driver.goToPath(`/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${moduleId}`);
        await expect.poll(() => driver.hasVisibleText(moduleName), { timeout: 120_000 }).toBe(true);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        expect(await driver.paletteHasCommand("Add/remove members")).toBe(true);
        expect(await driver.paletteHasCommand("Change status")).toBe(true);
        expect(await driver.paletteHasCommand("Add to favorites")).toBe(true);
        expect(await driver.paletteHasCommand("Copy URL")).toBe(true);
      } finally {
        await serverDeleteModule(seed.workspaceSlug, seed.projectId, moduleId, session);
      }
    }
  );

  test(
    specTitle(["SHELL-091"], "the page detail palette offers lock, visibility, archive, favorite and copy-URL entries"),
    { tag: specTags(["SHELL-091"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const pageName = `Parity page ${Date.now()}`;
      const pageId = await serverCreatePage(seed.workspaceSlug, seed.projectId, session, pageName);
      try {
        await driver.goToPath(`/${seed.workspaceSlug}/projects/${seed.projectId}/pages/${pageId}`);
        await expect.poll(() => driver.hasVisibleText(pageName), { timeout: 120_000 }).toBe(true);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        const titles = await driver.paletteCommandTitles();
        const joined = titles.join("\n");
        expect(joined.includes("Lock") || joined.includes("Unlock")).toBe(true);
        expect(joined.includes("Make public") || joined.includes("Make private")).toBe(true);
        expect(joined.includes("Archive") || joined.includes("Restore")).toBe(true);
        expect(joined.includes("Add to favorites") || joined.includes("Remove from favorites")).toBe(true);
        expect(await driver.paletteHasCommand("Copy URL")).toBe(true);
      } finally {
        await serverDeletePage(seed.workspaceSlug, seed.projectId, pageId, session);
      }
    }
  );
});

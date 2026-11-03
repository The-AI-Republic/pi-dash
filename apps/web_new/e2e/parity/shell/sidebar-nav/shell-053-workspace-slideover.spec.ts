// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-125): extra destinations list in More with
// persistent pin and order preferences.
// Observed on the running old app: the destinations that do not fit the
// main sidebar live in the secondary (More) disclosure; pin and order
// writes through the sidebar-preferences endpoint persist on the server,
// and the visible destination set keeps its membership across reloads.
// This build has no pin toggle (the extended sidebar that hosts it is not
// mounted) and pinning adds no main-sidebar row (every pin candidate is
// relocated to More), so the scenario pins through the API and proves the
// row stays out of the main sidebar. Row: SHELL-053.
import { test, expect } from "../../fixtures";
import {
  deleteWorkspace,
  ensureWorkspace,
  getSidebarPreferences,
  patchSidebarPreferences,
  ownerSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-053"];

test(
  specTitle(ROWS, "extra destinations pin and reorder persistently"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the secondary disclosure lists the extra destinations", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setMoreSectionOpen(true);
      await expect.poll(() => driver.moreSectionLinks(), { timeout: 60_000 }).not.toEqual([]);
      const texts = (await driver.moreSectionLinks()).map((link) => link.text);
      expect(texts).toEqual(expect.arrayContaining(["Analytics", "Archives"]));
      for (const link of await driver.moreSectionLinks()) {
        expect(link.href ?? "").toContain(`/${seed.workspaceSlug}/`);
      }
    });

    await test.step("pin and order writes persist on the server", async () => {
      // A dedicated workspace keeps the check to a single member's rows: the
      // preferences write endpoint scopes by key and workspace, not by user.
      const pins = await ensureWorkspace(session, "Parity Pins", "parity-pins");
      try {
        const before = await getSidebarPreferences(pins.slug, session);
        await patchSidebarPreferences(pins.slug, session, [
          { key: "analytics", is_pinned: true, sort_order: 1000 },
          { key: "archives", is_pinned: false, sort_order: 2000 },
        ]);
        const server = await getSidebarPreferences(pins.slug, session);
        expect(server["analytics"]?.is_pinned).toBe(true);
        expect(server["archives"]?.is_pinned).toBe(false);
        expect(server["analytics"]?.sort_order).toBe(1000);
        expect(server["archives"]?.sort_order).toBe(2000);

        // The pinned destination renders in More (its relocated home) while
        // the main sidebar gains no row for it: with More closed the main
        // sidebar holds exactly the built-in rows.
        await driver.openWorkspacePath(`/${pins.slug}/`);
        await driver.setMoreSectionOpen(true);
        await expect
          .poll(async () => (await driver.moreSectionLinks()).map((link) => link.text), { timeout: 60_000 })
          .toEqual(expect.arrayContaining(["Analytics", "Archives"]));
        await driver.setMoreSectionOpen(false);
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
        const main = await driver.sidebarLinkTexts();
        for (const absent of ["Analytics", "Archives"]) {
          expect(main).not.toContain(absent);
        }

        await patchSidebarPreferences(pins.slug, session, [
          {
            key: "analytics",
            is_pinned: before["analytics"]?.is_pinned ?? false,
            sort_order: before["analytics"]?.sort_order ?? 0,
          },
          {
            key: "archives",
            is_pinned: before["archives"]?.is_pinned ?? false,
            sort_order: before["archives"]?.sort_order ?? 0,
          },
        ]);
        const restored = await getSidebarPreferences(pins.slug, session);
        expect(restored["analytics"]?.is_pinned).toBe(before["analytics"]?.is_pinned ?? false);
      } finally {
        await deleteWorkspace(session, pins.slug);
      }
    });

    await test.step("the destination set keeps its membership across reloads", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setMoreSectionOpen(true);
      await expect.poll(() => driver.moreSectionLinks(), { timeout: 60_000 }).not.toEqual([]);
      const texts = (await driver.moreSectionLinks()).map((link) => link.text);
      expect(texts).toEqual(expect.arrayContaining(["Analytics", "Archives"]));
    });
  }
);

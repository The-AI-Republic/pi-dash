// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Command palette — server-search hits for cycles, modules and pages land
// on the entity page (NEWPIDASH-44). Row: SHELL-084 (server search
// family: the hits below come from the debounced workspace search).
//
// Views ride the same resolver branch (project views land on the project
// view, project-less views on the workspace view) but stay out of this
// scenario: the server only ever returns project views for issue_view
// hits, and view creation through the UI would couple this spec to the
// views area. The view branch is covered by the resolver unit tests.
// Project-less pages land on the workspace home in the new app: the old
// app's /wiki/{id} twin does not exist there yet, so no parity landing
// can be asserted for them (also unit-tested).
import { test, expect } from "../../fixtures";
import {
  createCycle,
  createModule,
  deleteCycle,
  deleteModule,
  deletePage,
  ensurePage,
  signInSession,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

/** Per-attempt unique marker so retries never collide on names. */
function stamp(): string {
  return `${Date.now().toString(36)}${Math.floor(Math.random() * 1296)
    .toString(36)
    .padStart(2, "0")}`;
}

/** Landing comparisons ignore the oracle's trailing slash. */
function bare(path: string): string {
  return path.replace(/\/$/, "");
}

test.describe("command palette entity search landings", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-084"], "picking a cycle hit lands on the cycle page"),
    { tag: specTags(["SHELL-084"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const name = `Cycle44 ${stamp()}`;
      const cycle = await createCycle(seed.workspaceSlug, seed.projectId, session, name);
      try {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.typeInCommandPalette(name);
        await driver.page.waitForTimeout(900);
        await expect.poll(() => driver.paletteSearchResultsHeading()).not.toBeNull();

        await driver.activatePaletteCommand(name);

        const expected = `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}`;
        await expect.poll(() => bare(new URL(driver.page.url()).pathname)).toBe(expected);
      } finally {
        await deleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, session);
      }
    }
  );

  test(
    specTitle(["SHELL-084"], "picking a module hit lands on the module page"),
    { tag: specTags(["SHELL-084"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const name = `Module44 ${stamp()}`;
      const module = await createModule(seed.workspaceSlug, seed.projectId, session, name);
      try {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.typeInCommandPalette(name);
        await driver.page.waitForTimeout(900);
        await expect.poll(() => driver.paletteSearchResultsHeading()).not.toBeNull();

        await driver.activatePaletteCommand(name);

        const expected = `/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${module.id}`;
        await expect.poll(() => bare(new URL(driver.page.url()).pathname)).toBe(expected);
      } finally {
        await deleteModule(seed.workspaceSlug, seed.projectId, module.id, session);
      }
    }
  );

  test(
    specTitle(["SHELL-084"], "picking a page hit lands on the page"),
    { tag: specTags(["SHELL-084"]) },
    async ({ driver, seed }) => {
      test.setTimeout(300_000);
      const session = await signInSession(seed.email, seed.password);
      const name = `Page44 ${stamp()}`;
      const page = await ensurePage(seed.workspaceSlug, seed.projectId, session, name);
      try {
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen()).toBe(true);
        await driver.typeInCommandPalette(name);
        await driver.page.waitForTimeout(900);
        await expect.poll(() => driver.paletteSearchResultsHeading()).not.toBeNull();

        await driver.activatePaletteCommand(name);

        const expected = `/${seed.workspaceSlug}/projects/${seed.projectId}/pages/${page.id}`;
        await expect.poll(() => bare(new URL(driver.page.url()).pathname)).toBe(expected);
      } finally {
        await deletePage(seed.workspaceSlug, seed.projectId, session, page.id);
      }
    }
  );
});

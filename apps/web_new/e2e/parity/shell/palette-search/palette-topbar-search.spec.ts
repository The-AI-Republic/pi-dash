// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Top-bar search box (NEWFRONT-127). Row: SHELL-081. The top navigation
// always mounts a plain-text search field; focusing it opens an inline
// (non-dialog) cmdk results panel with the same commands plus server hits as
// the modal palette. Escape clears the term and closes; an outside click
// closes; closing resets the term.
import { test, expect } from "../../fixtures";
import { specTags, specTitle } from "../../helpers/tags";

const TOPBAR_PLACEHOLDER = "Search commands...";

test.describe("top-bar search box", () => {
  test.beforeEach(async ({ driver, seed }) => {
    // Hook-level budget: body setTimeout calls do not extend running hooks.
    test.setTimeout(300_000);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.openProjectIssuesSettled(seed.workspaceSlug, seed.projectId);
  });

  test(
    specTitle(["SHELL-081"], "the top-bar box opens inline results, filters, and resets on close"),
    { tag: specTags(["SHELL-081"]) },
    async ({ driver }) => {
      test.setTimeout(300_000);
      await test.step("the top-bar search box is always visible and starts closed", async () => {
        expect(await driver.topBarSearchPlaceholder()).toBe(TOPBAR_PLACEHOLDER);
        expect(await driver.isTopBarResultsOpen()).toBe(false);
      });

      await test.step("focus opens the inline results panel, not the modal palette", async () => {
        await driver.focusTopBarSearch();
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(true);
        expect(await driver.isCommandPaletteOpen()).toBe(false);
        // Polled: web_new's rows load lazily behind the panel shell.
        await expect.poll(() => driver.topBarResultsCommandTitles()).not.toEqual([]);
        const titles = await driver.topBarResultsCommandTitles();
        expect(titles.length).toBeGreaterThan(0);
      });

      await test.step("typing filters to matching entries", async () => {
        await driver.typeInTopBarSearch("Change interface theme");
        await expect.poll(() => driver.topBarResultsCommandTitles()).toContain("Change interface theme");
      });

      await test.step("Escape clears the term and closes the panel", async () => {
        await driver.pressInTopBarSearch("Escape");
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(false);
        expect(await driver.topBarSearchValue()).toBe("");
      });

      await test.step("an outside click closes and resets the term", async () => {
        await driver.focusTopBarSearch();
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(true);
        await driver.typeInTopBarSearch("abc");
        await driver.closeTopBarViaOutsideClick();
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(false);
        expect(await driver.topBarSearchValue()).toBe("");
      });

      await test.step("focus widens the box and close shrinks it back (NEWPIDASH-7)", async () => {
        // Bounding-box comparison, not exact pixels: both apps grow the
        // box while the panel is open and restore it on close.
        const narrow = await driver.topBarSearchWidth();
        expect(narrow).not.toBeNull();
        await driver.focusTopBarSearch();
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(true);
        // Polled: the width animates from narrow to wide.
        await expect.poll(async () => driver.topBarSearchWidth()).toBeGreaterThan(narrow!);
        const wide = (await driver.topBarSearchWidth())!;
        expect(wide).toBeGreaterThan(narrow!);
        await driver.pressInTopBarSearch("Escape");
        await expect.poll(() => driver.isTopBarResultsOpen()).toBe(false);
        await expect.poll(async () => driver.topBarSearchWidth()).toBeLessThanOrEqual(narrow! + 1);
      });

      await test.step("a 640px window never scrolls sideways (NEWPIDASH-7)", async () => {
        // Below the box's rest width plus its neighbours, web_new yields
        // the closed footprint and caps the open overlay to the viewport,
        // so the page never gains a horizontal scrollbar and the box stays
        // inside the window, closed and open. The old bar is fixed-width
        // by design (every cell shrink-0 around a fixed 364px box), so on
        // the oracle only the open/close function is asserted.
        const NARROW_PX = 640;
        await driver.setViewportSize(NARROW_PX, 800);
        const fitsWindow = async (): Promise<boolean> =>
          driver.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
        if (driver.target === "web_new") {
          const closed = await driver.topBarSearchRect();
          expect(closed).not.toBeNull();
          expect(closed!.x).toBeGreaterThanOrEqual(0);
          expect(closed!.x + closed!.width).toBeLessThanOrEqual(NARROW_PX);
          await expect.poll(fitsWindow).toBe(true);
          await driver.focusTopBarSearch();
          await expect.poll(() => driver.isTopBarResultsOpen()).toBe(true);
          // Polled: the width animates from narrow to wide.
          await expect.poll(async () => (await driver.topBarSearchRect())?.width).toBeGreaterThan(closed!.width);
          // Past the transition: the final rect is the widest one.
          await driver.page.waitForTimeout(500);
          const open = await driver.topBarSearchRect();
          expect(open!.width).toBeGreaterThan(closed!.width);
          expect(open!.x).toBeGreaterThanOrEqual(0);
          expect(open!.x + open!.width).toBeLessThanOrEqual(NARROW_PX);
          await expect.poll(fitsWindow).toBe(true);
          await driver.pressInTopBarSearch("Escape");
          await expect.poll(() => driver.isTopBarResultsOpen()).toBe(false);
          await expect.poll(fitsWindow).toBe(true);
        } else {
          await driver.focusTopBarSearch();
          await expect.poll(() => driver.isTopBarResultsOpen()).toBe(true);
          await driver.pressInTopBarSearch("Escape");
          await expect.poll(() => driver.isTopBarResultsOpen()).toBe(false);
        }
        await driver.setViewportSize(1280, 720);
      });
    }
  );
});

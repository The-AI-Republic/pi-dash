// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings content width (NEWPIDASH-8, row WSET-001): the general page
// centres its content in a column at most 900px wide, keeps its side
// gutters on narrow windows, and leaves the wide sections (members,
// billing, exports) full width. Both apps share these numbers, so the
// scenario runs on each target unchanged.
import { expect, test } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

/** Wide viewports from the issue: the column caps at 900px and centres. */
const WIDE_VIEWPORTS = [1440, 1920];

/** Sections that skip the centred column on both apps. */
const FULL_WIDTH_SECTIONS = ["members", "billing", "exports"];

test(
  specTitle(["WSET-001"], "settings content centres in a max-900px column with full-width exceptions"),
  { tag: specTags(["WSET-001"]) },
  async ({ driver, seed }) => {
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.wsSettingsOpen(seed.workspaceSlug, "");

    for (const width of WIDE_VIEWPORTS) {
      await test.step(`the general column caps at 900px and centres at ${width}px`, async () => {
        await driver.setViewportSize(width, 900);
        await expect.poll(() => driver.wsSettingsContentBox(), { timeout: 15_000 }).not.toBeNull();
        const facts = (await driver.wsSettingsContentBox())!;
        expect(facts.width).toBeLessThanOrEqual(900.5);
        const left = facts.x - facts.parentX;
        const right = facts.parentX + facts.parentWidth - facts.x - facts.width;
        expect(Math.abs(left - right)).toBeLessThanOrEqual(1);
      });
    }

    // The old header is a separate full-width bar, not a lined-up column,
    // so this step only runs on the new app.
    if (driver.target === "web_new") {
      await test.step("the header content lines up with the body column", async () => {
        await driver.setViewportSize(1440, 900);
        await expect.poll(() => driver.wsSettingsContentBox(), { timeout: 15_000 }).not.toBeNull();
        const body = (await driver.wsSettingsContentBox())!;
        const header = await driver.page.getByTestId("settings-page-header-content").boundingBox();
        expect(header).not.toBeNull();
        expect(Math.abs(header!.x - body.x)).toBeLessThanOrEqual(1);
        expect(Math.abs(header!.width - body.width)).toBeLessThanOrEqual(1);
      });
    }

    await test.step("narrow windows keep the side gutters with no column overflow", async () => {
      await driver.setViewportSize(1024, 900);
      await expect.poll(() => driver.wsSettingsContentBox(), { timeout: 15_000 }).not.toBeNull();
      const facts = (await driver.wsSettingsContentBox())!;
      expect(facts.width).toBeLessThanOrEqual(facts.parentWidth);
      expect(await driver.wsSettingsContentGutter()).toBeGreaterThan(0);
    });

    for (const section of FULL_WIDTH_SECTIONS) {
      await test.step(`the ${section} section stays full width`, async () => {
        await driver.setViewportSize(1920, 900);
        await driver.wsSettingsOpen(seed.workspaceSlug, section);
        await expect.poll(() => driver.wsSettingsContentBox(), { timeout: 15_000 }).toBeNull();
      });
    }
  }
);

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): the views feature toggle in project
// settings (VIEW-041) and command-palette creation/jumping (VIEW-042).
// Green on apps/web first.
//
// Settings route:
// `app/(all)/[workspaceSlug]/(settings)/settings/projects/[projectId]/features/views/page.tsx`
// (admin gate + enable-views toggle; header file is a thin breadcrumb
// shell). Palette gating lives in the palette issue-command registry.
import { test, expect } from "../fixtures";
import { ROLE, createProjectViewFull, serverProjectViewFlags, setProjectViewFlags } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenListAs, viewsSeat, viewsSettingsOpenFeaturesViews } from "./support";

const POLL = { timeout: 60_000 };

test.describe("views settings toggle and command palette", () => {
  test(
    specTitle(["VIEW-041"], "settings gate: member sees not-authorized, admin flips the toggle"),
    { tag: specTags(["VIEW-041"]) },
    async ({ driver }) => {
      const harness = await viewsHarness("parity-vset");
      const { workspaceSlug, projectId, owner } = harness;
      const member = await viewsSeat(harness, ROLE.MEMBER, "parity-vset-member");
      const seen = `Vset Listed ${harness.tag}`;
      await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: seen });

      await test.step("a member sees the not-authorized notice instead of the toggle", async () => {
        await viewsSettingsOpenFeaturesViews(driver, harness, member);
        expect(await driver.viewsSettingsNotAuthorized()).toBe(true);
      });

      await test.step("the toggle starts on for the project admin", async () => {
        await viewsSettingsOpenFeaturesViews(driver, harness);
        expect(await driver.viewsSettingsNotAuthorized()).toBe(false);
        expect(await driver.viewsSettingsViewsToggleValue()).toBe(true);
      });

      await test.step("flipping it off disables views on the server and gates the list", async () => {
        await driver.viewsSettingsViewsToggleFlip();
        await expect
          .poll(async () => serverProjectViewFlags(workspaceSlug, projectId, owner.cookie), POLL)
          .toMatchObject({ issue_views_view: false });
        await viewsOpenListAs(driver, harness);
        await expect.poll(() => driver.viewsGateTitle(), POLL).toBe("Views are not enabled for the project.");
      });

      await test.step("flipping it back on restores the list with its views", async () => {
        await viewsSettingsOpenFeaturesViews(driver, harness);
        await driver.viewsSettingsViewsToggleFlip();
        await expect
          .poll(async () => serverProjectViewFlags(workspaceSlug, projectId, owner.cookie), POLL)
          .toMatchObject({ issue_views_view: true });
        await viewsOpenListAs(driver, harness);
        await expect.poll(() => driver.viewsListNames(), POLL).toContain(seen);
      });
    }
  );

  test(
    specTitle(["VIEW-042"], "palette create command opens the same create dialog, gated by role and flag"),
    { tag: specTags(["VIEW-042"]) },
    async ({ driver }) => {
      const harness = await viewsHarness("parity-vpal");
      const { workspaceSlug, projectId, owner } = harness;
      const guest = await viewsSeat(harness, ROLE.GUEST, "parity-vpal-guest");

      await test.step("the nv key sequence opens the create dialog", async () => {
        // A global keydown handler (power-k GlobalShortcutsProvider +
        // ShortcutHandler), not the open palette, owns key sequences. It
        // runs first: every keydown extends its 1s-window sequence buffer,
        // so pressing nv right after the Escape below would miss.
        await viewsOpenListAs(driver, harness);
        await expect.poll(() => driver.viewsEmptyTitle(), POLL).toBe("Save custom views for your project");
        await driver.pressKey("n");
        await driver.pressKey("v");
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
        expect(await driver.viewsDialogHeading()).toBe("Create View");
        await driver.viewsDialogCancel();
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(false);
      });

      await test.step("a member opens the create dialog through the palette", async () => {
        await expect.poll(() => driver.viewsEmptyTitle(), POLL).toBe("Save custom views for your project");
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), POLL).toBe(true);
        await driver.typeInCommandPalette("New view");
        await expect.poll(() => driver.paletteHasCommand("New view"), POLL).toBe(true);
        await driver.activatePaletteCommand("New view");
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
        expect(await driver.viewsDialogHeading()).toBe("Create View");
        await driver.viewsDialogEscape();
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(false);
      });

      await test.step("a guest has no create command in the palette", async () => {
        await viewsOpenListAs(driver, harness, guest);
        await expect.poll(() => driver.viewsEmptyTitle(), POLL).toBe("Save custom views for your project");
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), POLL).toBe(true);
        // Load-proof first: the jump command proves project context is in
        // (titles carry the key-sequence badge, hence the substring match).
        await expect
          .poll(async () => (await driver.paletteCommandTitles()).some((t) => t.includes("Open a project view")), POLL)
          .toBe(true);
        await driver.typeInCommandPalette("New view");
        await expect.poll(() => driver.paletteCommandTitles(), POLL).toEqual([]);
        expect(await driver.paletteHasCommand("New view")).toBe(false);
        await driver.pressInCommandPalette("Escape");
      });

      await test.step("the command disappears while the feature flag is off", async () => {
        await setProjectViewFlags(workspaceSlug, projectId, owner.cookie, { issue_views_view: false });
        await viewsOpenListAs(driver, harness);
        await expect.poll(() => driver.viewsGateTitle(), POLL).toBe("Views are not enabled for the project.");
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), POLL).toBe(true);
        // Load-proof first: a global command proves the palette rendered.
        await expect
          .poll(async () => (await driver.paletteCommandTitles()).some((t) => t.includes("Go to home")), POLL)
          .toBe(true);
        await driver.typeInCommandPalette("New view");
        await expect.poll(() => driver.paletteCommandTitles(), POLL).toEqual([]);
        expect(await driver.paletteHasCommand("New view")).toBe(false);
        await driver.pressInCommandPalette("Escape");
        await setProjectViewFlags(workspaceSlug, projectId, owner.cookie, { issue_views_view: true });
      });
    }
  );

  test(
    specTitle(["VIEW-042"], "palette jump menu lists the project views and navigates on selection"),
    { tag: specTags(["VIEW-042"]) },
    async ({ driver }) => {
      const harness = await viewsHarness("parity-vjmp");
      const { workspaceSlug, projectId, owner, projectName } = harness;
      const guest = await viewsSeat(harness, ROLE.GUEST, "parity-vjmp-guest");
      const target = `Vjmp Target ${harness.tag}`;
      await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: target });

      await test.step("the jump menu lists views and lands on the selected detail", async () => {
        await viewsOpenListAs(driver, harness);
        await expect.poll(() => driver.viewsListNames(), POLL).toContain(target);
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), POLL).toBe(true);
        await driver.typeInCommandPalette("Open a project view");
        await expect.poll(() => driver.paletteHasCommand("Open a project view"), POLL).toBe(true);
        await driver.activatePaletteCommand("Open a project view");
        await expect.poll(() => driver.paletteCommandTitles(), POLL).toContain(target);
        await driver.activatePaletteCommand(target);
        await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toBe(`${projectName} - ${target}`);
      });

      await test.step("jumping stays available to every role", async () => {
        await viewsOpenListAs(driver, harness, guest);
        // The guest sees an empty list (the owner's view is not shared
        // with them) but the jump command still lists for every role.
        await expect.poll(() => driver.viewsEmptyTitle(), POLL).toBe("Save custom views for your project");
        await driver.pressPaletteOpenChord();
        await expect.poll(() => driver.isCommandPaletteOpen(), POLL).toBe(true);
        await driver.typeInCommandPalette("Open a project view");
        await expect.poll(() => driver.paletteHasCommand("Open a project view"), POLL).toBe(true);
        await driver.pressInCommandPalette("Escape");
      });
    }
  );
});

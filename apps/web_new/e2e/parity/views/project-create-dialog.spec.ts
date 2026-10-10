// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): the project create-view dialog — its
// header entry point, fields, validation, pickers, submit success and
// failure, and the OSS access-selector absence. Rows: VIEW-003,
// VIEW-013, VIEW-014, VIEW-019 (OSS part). Green on apps/web first.
import { test, expect } from "../fixtures";
import { VIEW_ACCESS, serverProjectViewDetail, serverSavedViews } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenListAs } from "./support";

const POLL = { timeout: 60_000 };

test(
  specTitle(["VIEW-003"], "header Add opens the create dialog over the list"),
  { tag: specTags(["VIEW-003"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd3");
    const { workspaceSlug, projectId, owner } = harness;

    await test.step("the dialog opens with a blank form", async () => {
      await viewsOpenListAs(driver, harness);
      await driver.viewsOpenCreateFromHeader();
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
      expect(await driver.viewsDialogHeading()).toBe("Create View");
      expect(await driver.viewsDialogTitleValue()).toBe("");
      expect(await driver.viewsDialogDescriptionValue()).toBe("");
      expect(await driver.viewsDialogLayoutValue()).toBe("List");
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server).toHaveLength(0);
    });
  }
);

test(
  specTitle(["VIEW-013", "VIEW-019"], "create dialog fields, validation, pickers; no access selector on OSS"),
  { tag: specTags(["VIEW-013", "VIEW-019"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd13");
    const { workspaceSlug, projectId, owner } = harness;

    await test.step("pickers and the filter builder render; access is absent", async () => {
      await viewsOpenListAs(driver, harness);
      await driver.viewsOpenCreateFromHeader();
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
      expect(await driver.viewsDialogAccessPresent()).toBe(false);
      expect(await driver.viewsDialogFiltersExpanded()).toBe(true);
      await driver.viewsDialogIconOpen();
      expect(await driver.viewsDialogIconTabs()).toEqual(["Emoji", "Icon"]);
      await driver.viewsDialogPickFirstIcon();
      await driver.viewsDialogDisplayOpen();
      const display = (await driver.viewsDialogDisplayTexts()).join(" | ");
      expect(display).toContain("Display Properties");
      expect(display).toContain("Group by");
    });

    await test.step("title is required and capped at 255 characters", async () => {
      await driver.viewsDialogSubmitAttempt();
      await expect.poll(() => driver.viewsDialogTitleError(), POLL).toBe("Title is required");
      expect(await driver.viewsDialogOpen()).toBe(true);
      await driver.viewsDialogFillTitle("x".repeat(256));
      await driver.viewsDialogSubmitAttempt();
      await expect.poll(() => driver.viewsDialogTitleError(), POLL).toContain("255");
      expect(await driver.viewsDialogOpen()).toBe(true);
    });

    await test.step("Escape and Cancel close without saving", async () => {
      await driver.viewsDialogEscape();
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(false);
      await driver.viewsOpenCreateFromHeader();
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
      await driver.viewsDialogFillTitle("Never saved");
      await driver.viewsDialogFillDescription("discarded");
      await driver.viewsDialogCancel();
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(false);
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server).toHaveLength(0);
    });
  }
);

test(
  specTitle(["VIEW-014", "VIEW-019"], "create success navigates with defaults; failure resets (bug)"),
  { tag: specTags(["VIEW-014", "VIEW-019"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd14");
    const { workspaceSlug, projectId, owner } = harness;
    const name = `Vd14 Board ${harness.tag}`;

    // Intended: the dialog stays open with the user's input intact so the
    // user can retry. The app resets the form to blank defaults instead.
    await test.step("bug: NEWFRONT-221 a failing submit toasts and resets the form", async () => {
      await viewsOpenListAs(driver, harness);
      await driver.viewsOpenCreateFromHeader();
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
      await driver.viewsDialogFillTitle(name);
      await driver.viewsDialogFillDescription("wiped on failure");
      await driver.viewsFailNextWrite(500);
      await driver.viewsDialogSubmitAttempt();
      await expect.poll(() => driver.lastToast(), POLL).toContain("Failed to create view");
      expect(await driver.viewsDialogOpen()).toBe(true);
      await expect.poll(() => driver.viewsDialogTitleValue(), POLL).toBe("");
      expect(await driver.viewsDialogDescriptionValue()).toBe("");
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server).toHaveLength(0);
    });

    await test.step("success navigates to the detail page with server defaults", async () => {
      await driver.viewsDialogFillTitle(name);
      await driver.viewsDialogFillDescription("board with icon");
      await driver.viewsDialogPickLayout("Board");
      await driver.viewsDialogIconOpen();
      await driver.viewsDialogPickFirstIcon();
      await driver.viewsDialogSubmit();
      await expect.poll(() => driver.page.url(), POLL).toContain("/views/");
      const viewId = driver.page.url().split("/views/")[1]?.split("/")[0] ?? "";
      expect(viewId).toMatch(/^[0-9a-f-]{36}$/);
      await expect.poll(() => driver.lastToast(), POLL).toContain("View created successfully");
      const detail = await serverProjectViewDetail(workspaceSlug, projectId, viewId, owner.cookie);
      expect(detail.name).toBe(name);
      expect(detail.description).toBe("board with icon");
      expect(detail.access).toBe(VIEW_ACCESS.PUBLIC);
      expect((detail.display_filters as { layout?: string })?.layout).toBe("kanban");
      expect((detail.display_filters as { group_by?: string })?.group_by).toBe("state");
      expect((detail.logo_props as { in_use?: string })?.in_use).toBe("icon");
      expect((detail.logo_props as { icon?: { name?: string } })?.icon?.name).toBe("Activity");
    });
  }
);

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): custom workspace view rows, the
// create/edit dialog, delete, and the permission-gated quick menus.
// Rows: VIEW-032, VIEW-034, VIEW-035, VIEW-036, VIEW-037. Green on
// apps/web first.
import { test, expect } from "../fixtures";
import { ROLE, createWorkspaceViewFull, serverWorkspaceViewDetail, serverWorkspaceViews } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsSeat, wsViewsOpenDetailAs, wsViewsOpenListAs } from "./support";

const POLL = { timeout: 60_000 };

test(
  specTitle(["VIEW-032"], "custom rows show name, description and deep link with an edit/delete menu"),
  { tag: specTags(["VIEW-032"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w32");
    const { owner, workspaceSlug } = harness;
    const long = `W32 ${"n".repeat(80)} ${harness.tag}`;
    const custom = `W32 Alpha ${harness.tag}`;
    await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom, description: "Row desc" });
    const longId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: long });

    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual(expect.arrayContaining([custom]));
    expect(await driver.wsViewsRowDescription(custom)).toBe("Row desc");
    expect(await driver.wsViewsRowDescription(long.slice(0, 40))).toBe("");

    // Long names truncate at length in the row.
    const names = await driver.wsViewsListNames();
    const shown = names.find((n) => n.startsWith("W32 nnn")) ?? "";
    expect(shown.length).toBeLessThan(long.length);

    await driver.wsViewsRowMenuOpen(custom);
    await expect.poll(() => driver.wsViewsRowMenuEntries(), POLL).toEqual(["Edit View", "Delete View"]);

    await driver.wsViewsRowMenuPick("Edit View");
    await expect.poll(() => driver.viewsDialogHeading(), POLL).toBe("Update View");
    await driver.viewsDialogCancel();

    await driver.wsViewsRowOpen(custom);
    await expect.poll(() => driver.currentUrlPath(), POLL).toContain("/workspace-views/");
    expect(await serverWorkspaceViewDetail(workspaceSlug, longId, owner.cookie)).toMatchObject({ name: long });
  }
);

test(
  specTitle(["VIEW-032"], "row menus render ungated; the server enforces edit and delete"),
  { tag: specTags(["VIEW-032"]) },
  async ({ driver }) => {
    // Gap vs "menu entries permission-gated": the row renders Edit and
    // Delete for every member; only the server refuses (403) non-owner
    // edits and non-owner non-admin deletes.
    const harness = await viewsHarness("parity-w32g");
    const { owner, workspaceSlug } = harness;
    const member = await viewsSeat(harness, ROLE.MEMBER, "parity-w32gm");
    const custom = `W32g Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

    await wsViewsOpenListAs(driver, harness, member);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([custom]);
    await driver.wsViewsRowMenuOpen(custom);
    await expect.poll(() => driver.wsViewsRowMenuEntries(), POLL).toEqual(["Edit View", "Delete View"]);
    await driver.wsViewsRowMenuPick("Delete View");
    await expect.poll(() => driver.viewsDeleteTitle(), POLL).toContain("delete this view");
    await driver.viewsDeleteConfirm();
    await expect.poll(() => driver.lastToast(), POLL).toContain("Failed to delete");
    expect((await serverWorkspaceViews(workspaceSlug, owner.cookie)).map((v) => v.id)).toContain(customId);
  }
);

test(
  specTitle(["VIEW-034"], "create validates, then succeeds with a notice and navigation"),
  { tag: specTags(["VIEW-034"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w34");
    const { owner, workspaceSlug } = harness;

    await wsViewsOpenDetailAs(driver, harness, "all-issues");
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    await driver.wsViewsDetailAddClick();
    await expect.poll(() => driver.viewsDialogHeading(), POLL).toBe("Create View");
    // OSS: no access selector (cloud-only, as VIEW-019); the Display
    // dropdown is fixed to the spreadsheet set; filters start expanded.
    expect(await driver.viewsDialogAccessPresent()).toBe(false);
    const displayOptions = await driver.wsViewsDialogDisplayOptions();
    expect(displayOptions).toContain("Display Properties");
    // The form hardcodes the spreadsheet layout set (no layout picker).
    expect(displayOptions).not.toContain("Sub-group by");
    expect(await driver.viewsDialogFiltersExpanded()).toBe(true);

    await driver.viewsDialogSubmitAttempt();
    expect(await driver.viewsDialogTitleError()).toBe("Title is required");
    expect(await driver.viewsDialogOpen()).toBe(true);

    await driver.viewsDialogFillTitle("x".repeat(256));
    await driver.viewsDialogSubmitAttempt();
    expect(await driver.viewsDialogTitleError()).toContain("255");

    const name = `W34 Made ${harness.tag}`;
    await driver.viewsDialogFillTitle(name);
    await driver.viewsDialogFillDescription("Made desc");
    await driver.viewsDialogSubmit();
    await expect.poll(() => driver.lastToast(), POLL).toContain("created successfully");
    await expect.poll(() => driver.currentUrlPath(), POLL).toMatch(/\/workspace-views\/[0-9a-f-]{36}/);
    const created = (await serverWorkspaceViews(workspaceSlug, owner.cookie)).find((v) => v.name === name);
    expect(created?.description).toBe("Made desc");
  }
);

test(
  specTitle(["VIEW-034"], "failed create shows an error notice and keeps the dialog"),
  { tag: specTags(["VIEW-034"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w34f");
    const { owner, workspaceSlug } = harness;

    await wsViewsOpenDetailAs(driver, harness, "all-issues");
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    await driver.wsViewsDetailAddClick();
    await driver.viewsDialogFillTitle(`W34f Made ${harness.tag}`);
    await driver.wsViewsFailNextWrite(500);
    await driver.viewsDialogSubmitAttempt();
    await expect.poll(() => driver.lastToast(), POLL).toContain("could not be created");
    expect(await driver.viewsDialogHeading()).toBe("Create View");
    expect(await serverWorkspaceViews(workspaceSlug, owner.cookie)).toEqual([]);
  }
);

test(
  specTitle(["VIEW-035"], "edit prefills the definition and persists on save"),
  { tag: specTags(["VIEW-035"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w35");
    const { owner, workspaceSlug } = harness;
    const custom = `W35 Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, {
      name: custom,
      description: "Before desc",
    });

    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([custom]);
    await driver.wsViewsRowMenuOpen(custom);
    await driver.wsViewsRowMenuPick("Edit View");
    await expect.poll(() => driver.viewsDialogHeading(), POLL).toBe("Update View");
    expect(await driver.viewsDialogTitleValue()).toBe(custom);
    expect(await driver.viewsDialogDescriptionValue()).toBe("Before desc");

    const renamed = `${custom} Renamed`;
    await driver.viewsDialogFillTitle(renamed);
    await driver.viewsDialogSubmit();
    await expect.poll(() => driver.lastToast(), POLL).toContain("updated successfully");
    await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(false);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([renamed]);
    expect((await serverWorkspaceViewDetail(workspaceSlug, customId, owner.cookie)).name).toBe(renamed);
  }
);

test(
  specTitle(["VIEW-035"], "bug: failed edit stays silent with the dialog open"),
  { tag: specTags(["VIEW-035"]) },
  async ({ driver }) => {
    // bug: NEWFRONT-232 — the store swallows PATCH failures (optimistic
    // rollback, no rethrow), so the modal's error toast never fires: the
    // failed save leaves the dialog open on the previous values with no
    // notice at all. The intended behavior is the VIEW-035 error notice.
    const harness = await viewsHarness("parity-w35f");
    const { owner, workspaceSlug } = harness;
    const custom = `W35f Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([custom]);
    await driver.wsViewsRowMenuOpen(custom);
    await driver.wsViewsRowMenuPick("Edit View");
    await driver.viewsDialogFillTitle(`${custom} Renamed`);
    await driver.wsViewsFailNextWrite(500);
    await driver.viewsDialogSubmitAttempt();
    // Sequential reads; the local round trip settles long before the last.
    // (The list behind the modal is aria-hidden, so rollback is proven
    // server-side, not through the row.)
    await expect.poll(() => driver.viewsDialogHeading(), POLL).toBe("Update View");
    expect((await serverWorkspaceViewDetail(workspaceSlug, customId, owner.cookie)).name).toBe(custom);
    expect(await driver.lastToast()).toBeNull();
  }
);

test(
  specTitle(["VIEW-036"], "delete confirms, then removes silently; cancel keeps the view"),
  { tag: specTags(["VIEW-036"]) },
  async ({ driver }) => {
    // Gap vs "a success notice": deletion closes silently with no toast.
    const harness = await viewsHarness("parity-w36");
    const { owner, workspaceSlug } = harness;
    const custom = `W36 Alpha ${harness.tag}`;
    await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([custom]);
    expect(await driver.lastToast()).toBeNull();

    await driver.wsViewsRowMenuOpen(custom);
    await driver.wsViewsRowMenuPick("Delete View");
    await expect.poll(() => driver.viewsDeleteTitle(), POLL).toContain("delete this view");
    expect(await driver.viewsDeleteBody()).toContain("permanently deleted");
    await driver.viewsDeleteCancel();
    expect(await driver.wsViewsListNames()).toEqual([custom]);

    await driver.wsViewsRowMenuOpen(custom);
    await driver.wsViewsRowMenuPick("Delete View");
    await driver.viewsDeleteConfirm();
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([]);
    expect(await driver.lastToast()).toBeNull();
    expect(await serverWorkspaceViews(workspaceSlug, owner.cookie)).toEqual([]);
  }
);

test(
  specTitle(["VIEW-036"], "failed delete shows an error notice and keeps the view"),
  { tag: specTags(["VIEW-036"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w36f");
    const { owner, workspaceSlug } = harness;
    const custom = `W36f Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([custom]);
    await driver.wsViewsRowMenuOpen(custom);
    await driver.wsViewsRowMenuPick("Delete View");
    await driver.wsViewsFailNextWrite(500);
    await driver.viewsDeleteConfirm();
    await expect.poll(() => driver.lastToast(), POLL).toContain("Failed to delete");
    expect((await serverWorkspaceViews(workspaceSlug, owner.cookie)).map((v) => v.id)).toContain(customId);
  }
);

test(
  specTitle(["VIEW-037"], "detail quick menus follow ownership; copy and new-tab work for all"),
  { tag: specTags(["VIEW-037"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w37");
    const { owner, workspaceSlug } = harness;
    const member = await viewsSeat(harness, ROLE.MEMBER, "parity-w37m");
    const admin = await viewsSeat(harness, ROLE.ADMIN, "parity-w37a");
    const custom = `W37 Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

    await wsViewsOpenDetailAs(driver, harness, customId);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    await driver.wsViewsDetailMenuOpen();
    await expect
      .poll(() => driver.wsViewsDetailMenuEntries(), POLL)
      .toEqual(["Edit", "Open in new tab", "Copy link", "Delete"]);
    const href = await driver.wsViewsDetailOpenNewTabHref();
    expect(href).toContain(customId);

    await driver.wsViewsDetailMenuOpen();
    const copied = await driver.wsViewsDetailCopyLink();
    expect(copied).toContain(customId);
    expect(await driver.lastToast()).toContain("Link Copied");

    await wsViewsOpenDetailAs(driver, harness, customId, member);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    await driver.wsViewsDetailMenuOpen();
    await expect.poll(() => driver.wsViewsDetailMenuEntries(), POLL).toEqual(["Open in new tab", "Copy link"]);

    await wsViewsOpenDetailAs(driver, harness, customId, admin);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    await driver.wsViewsDetailMenuOpen();
    await expect
      .poll(() => driver.wsViewsDetailMenuEntries(), POLL)
      .toEqual(["Open in new tab", "Copy link", "Delete"]);
  }
);

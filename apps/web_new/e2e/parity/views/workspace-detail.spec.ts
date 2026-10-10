// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): workspace view details — default
// menus, the (absent) tab strip, custom detail chrome and bodies,
// unknown-id handling, and the OSS layout default. Rows: VIEW-031
// (detail half), VIEW-033, VIEW-038, VIEW-039 (detail half), VIEW-040,
// VIEW-045 (no-access half). Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  createIssue,
  createWorkspaceViewFull,
  serverPlantViewFlags,
  serverWorkspaceViewDetail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsSeat, wsSpreadsheetDisplay, wsViewsOpenDetailAs, wsViewsOpenListAs } from "./support";

const POLL = { timeout: 60_000 };

test(
  specTitle(["VIEW-031"], "default detail menu offers open-in-new-tab and copy link"),
  { tag: specTags(["VIEW-031"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w31d");
    await wsViewsOpenDetailAs(driver, harness, "all-issues");
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    expect(await driver.wsViewsDetailErrorTitle()).toBe("");

    await driver.wsViewsDetailMenuOpen();
    await expect.poll(() => driver.wsViewsDetailMenuEntries(), POLL).toEqual(["Open in new tab", "Copy link"]);
    const href = await driver.wsViewsDetailOpenNewTabHref();
    expect(href).toContain("/workspace-views/all-issues");

    await driver.wsViewsDetailMenuOpen();
    const copied = await driver.wsViewsDetailCopyLink();
    expect(copied).toContain("/workspace-views/all-issues");
    expect(await driver.lastToast()).toContain("Link Copied");
  }
);

test(
  specTitle(["VIEW-033"], "no tab strip renders on workspace list or detail pages"),
  { tag: specTags(["VIEW-033"]) },
  async ({ driver }) => {
    // Gap: the GlobalViewsHeader tab strip (default/custom tabs with
    // auto-scroll, per-tab menus and a trailing member+ add button) is
    // unmounted dead code — no strip renders anywhere. Creation lives in
    // the detail header's Add view button instead (see VIEW-038).
    const harness = await viewsHarness("parity-w33");
    const { owner, workspaceSlug } = harness;
    const custom = `W33 Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsListTabTitle(), POLL).toContain("All Views");
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([custom]);
    expect(await driver.wsViewsStripVisible()).toBe(false);

    await wsViewsOpenDetailAs(driver, harness, customId);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    expect(await driver.wsViewsStripVisible()).toBe(false);
  }
);

test(
  specTitle(["VIEW-038"], "custom detail shows switcher, controls and the issue body"),
  { tag: specTags(["VIEW-038"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w38");
    const { owner, workspaceSlug, projectId } = harness;
    const issue = `W38 Issue ${harness.tag}`;
    await createIssue(workspaceSlug, projectId, owner.cookie, issue);
    const custom = `W38 Alpha ${harness.tag}`;
    const other = `W38 Beta ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, {
      name: custom,
      ...wsSpreadsheetDisplay(),
    });
    await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: other, ...wsSpreadsheetDisplay() });

    await wsViewsOpenDetailAs(driver, harness, customId);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    expect(await driver.wsViewsDetailErrorTitle()).toBe("");
    await expect.poll(() => driver.wsViewsDetailCrumbs(), POLL).toEqual(["Views", custom]);
    expect(await driver.wsViewsDetailDisplayVisible()).toBe(true);
    expect(await driver.wsViewsDetailFiltersToggleVisible()).toBe(true);
    expect(await driver.wsViewsDetailAddVisible()).toBe(true);
    await expect.poll(() => driver.viewsDetailShowsIssue(issue), POLL).toBe(true);

    await driver.wsViewsDetailSwitcherOpen(custom);
    const options = await driver.wsViewsDetailSwitcherOptions();
    expect(options).toEqual(
      expect.arrayContaining(["All work items", "Assigned", "Created", "Subscribed", custom, other])
    );
    expect(await driver.wsViewsDetailSwitcherSearchVisible()).toBe(true);
    await driver.wsViewsDetailSwitcherSearch("Beta");
    await expect.poll(() => driver.wsViewsDetailSwitcherOptions(), POLL).toEqual([other]);
    await driver.wsViewsDetailSwitcherPick(other);
    await expect.poll(() => driver.wsViewsDetailCrumbs(), POLL).toEqual(["Views", other]);

    const detail = await serverWorkspaceViewDetail(workspaceSlug, customId, owner.cookie);
    expect(detail.name).toBe(custom);
    expect(detail.display_filters).toMatchObject({ layout: "spreadsheet" });
  }
);

test(
  specTitle(["VIEW-038"], "locked customs hide display but keep toggle and add; guests see add"),
  { tag: specTags(["VIEW-038"]) },
  async ({ driver }) => {
    // Gap (mirrors the project locked-row gap): the filter toggle stays
    // on locked views while Display hides. Gap vs "add ADMIN/MEMBER":
    // the live detail Add renders for every role incl. guests (the
    // member-gated + belonged to the dead tab strip).
    const harness = await viewsHarness("parity-w38l");
    const { owner, workspaceSlug } = harness;
    const guest = await viewsSeat(harness, ROLE.GUEST, "parity-w38lg");
    const custom = `W38l Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, {
      name: custom,
      ...wsSpreadsheetDisplay(),
    });
    await serverPlantViewFlags(customId, { is_locked: true });

    await wsViewsOpenDetailAs(driver, harness, customId);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    expect(await driver.wsViewsDetailDisplayVisible()).toBe(false);
    expect(await driver.wsViewsDetailFiltersToggleVisible()).toBe(true);
    expect(await driver.wsViewsDetailAddVisible()).toBe(true);
    expect((await serverWorkspaceViewDetail(workspaceSlug, customId, owner.cookie)).is_locked).toBe(true);

    await wsViewsOpenDetailAs(driver, harness, "all-issues", guest);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    expect(await driver.wsViewsDetailErrorTitle()).toBe("");
    expect(await driver.wsViewsDetailAddVisible()).toBe(true);
  }
);

test(
  specTitle(["VIEW-039", "VIEW-045"], "unknown and forbidden workspace views offer the way back"),
  { tag: specTags(["VIEW-039", "VIEW-045"]) },
  async ({ driver }) => {
    // Gap vs "same missing-view handling as VIEW-022": unknown workspace
    // ids keep the detail header and offer "Go to All work items" (to the
    // default view, not the list).
    const harness = await viewsHarness("parity-w39d");
    const { owner, workspaceSlug } = harness;
    const guest = await viewsSeat(harness, ROLE.GUEST, "parity-w39dg");
    const custom = `W39d Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

    await wsViewsOpenDetailAs(driver, harness, "00000000-0000-0000-0000-000000000000");
    await expect.poll(() => driver.wsViewsDetailErrorTitle(), POLL).toBe("View does not exist");
    await driver.wsViewsDetailErrorBack();
    await expect.poll(() => driver.currentUrlPath(), POLL).toContain("/workspace-views/all-issues");
    await expect.poll(() => driver.wsViewsDetailErrorTitle(), POLL).toBe("");

    // Guests are forbidden others' customs (missing-view state); the way
    // back lands them on the default view they can use.
    await wsViewsOpenDetailAs(driver, harness, customId, guest);
    await expect.poll(() => driver.wsViewsDetailErrorTitle(), POLL).toBe("View does not exist");
    await driver.wsViewsDetailErrorBack();
    await expect.poll(() => driver.currentUrlPath(), POLL).toContain("/workspace-views/all-issues");
    await expect.poll(() => driver.wsViewsDetailErrorTitle(), POLL).toBe("");
  }
);

test(
  specTitle(["VIEW-040"], "OSS shows no layout selector; the spreadsheet default renders"),
  { tag: specTags(["VIEW-040"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w40");
    const { owner, workspaceSlug, projectId } = harness;
    const issue = `W40 Issue ${harness.tag}`;
    await createIssue(workspaceSlug, projectId, owner.cookie, issue);
    const custom = `W40 Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, {
      name: custom,
      ...wsSpreadsheetDisplay(),
    });

    await wsViewsOpenDetailAs(driver, harness, customId);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    // The layout selector is a cloud-only seam (empty OSS stub).
    expect(await driver.wsViewsDetailLayoutVisible()).toBe(false);
    await expect.poll(() => driver.viewsDetailShowsIssue(issue), POLL).toBe(true);
  }
);

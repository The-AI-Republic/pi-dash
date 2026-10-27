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
import { isCloudNewEdition } from "../helpers/parity-env";
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
    // Target, no oracle (spec §10/§16): the workspace detail page carries a
    // tab strip with one tab per default and custom view, per-tab menus and
    // a trailing add button. The old app never mounted that header, so it
    // pins the strip's absence; web_new ships it. The list page keeps no
    // strip — it has no active view, and its creation lives nowhere (§9).
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
    if (driver.target === "web_new") {
      // The strip sits inside the detail's lazy filter-chunk boundary, so it
      // appears a beat after the header; poll rather than read once.
      await expect.poll(() => driver.wsViewsStripVisible(), POLL).toBe(true);
      return;
    }
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
    // Intended (VIEW-038, spec §11/§16): a locked custom hides the filter
    // toggle along with Display and the layout selector, and Add view is
    // offered only to workspace admins and members. The old app kept the
    // toggle and showed Add to guests; web_new ships the target.
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
    if (driver.target === "web_new") {
      expect(await driver.wsViewsDetailFiltersToggleVisible()).toBe(false);
      expect(await driver.wsViewsDetailAddVisible()).toBe(true);
    } else {
      expect(await driver.wsViewsDetailFiltersToggleVisible()).toBe(true);
      expect(await driver.wsViewsDetailAddVisible()).toBe(true);
    }
    expect((await serverWorkspaceViewDetail(workspaceSlug, customId, owner.cookie)).is_locked).toBe(true);

    await wsViewsOpenDetailAs(driver, harness, "all-issues", guest);
    await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
    expect(await driver.wsViewsDetailErrorTitle()).toBe("");
    expect(await driver.wsViewsDetailAddVisible()).toBe(driver.target === "web_new" ? false : true);
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
  async ({ driver }, testInfo) => {
    // The cloud build fills the selector slot, so this absence assertion is
    // the OSS half of the row and only means something on an OSS build.
    test.skip(testInfo.project.name === "new" && isCloudNewEdition(), "the cloud build serves the selector");
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

test(
  specTitle(["VIEW-040"], "cloud: the layout selector switches the body and persists it"),
  { tag: specTags(["VIEW-040"]) },
  async ({ driver }, testInfo) => {
    // Cloud leg (parity/specs/views.md §11/§14): target-only, because the old
    // cloud app renders no selector (its ce layout module is an empty stub).
    // It runs when PARITY_NEW_URL points at a cloud build, which the run
    // declares with PARITY_NEW_EDITION=cloud.
    test.skip(testInfo.project.name !== "new", "the layout selector pins the web_new cloud build only");
    test.skip(!isCloudNewEdition(), "cloud leg needs web_new built with the cloud edition");
    const harness = await viewsHarness("parity-w40c");
    const { owner, workspaceSlug, projectId } = harness;
    const issue = `W40c Issue ${harness.tag}`;
    await createIssue(workspaceSlug, projectId, owner.cookie, issue);
    const custom = `W40c Alpha ${harness.tag}`;
    const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, {
      name: custom,
      ...wsSpreadsheetDisplay(),
    });

    await test.step("a custom opens with the selector on its stored layout", async () => {
      await wsViewsOpenDetailAs(driver, harness, customId);
      await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
      expect(await driver.wsViewsDetailLayoutVisible()).toBe(true);
      // Stored spreadsheet -> index 3 in the switcher's canonical order.
      await expect.poll(() => driver.viewsDetailLayoutActive(), POLL).toBe(3);
      await expect.poll(() => driver.viewsDetailShowsIssue(issue), POLL).toBe(true);
    });

    await test.step("switching to the list layout persists in the view's display payload", async () => {
      await driver.viewsDetailLayoutPick(0);
      // The switch is an optimistic local edit followed by the view's own
      // display-filter update (§11), so wait on the server, not the header.
      await expect
        .poll(
          async () => (await serverWorkspaceViewDetail(workspaceSlug, customId, owner.cookie)).display_filters,
          POLL
        )
        .toMatchObject({ layout: "list" });
      await expect.poll(() => driver.viewsDetailShowsIssue(issue), POLL).toBe(true);
    });

    await test.step("the choice survives a reload", async () => {
      await wsViewsOpenDetailAs(driver, harness, customId);
      await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
      await expect.poll(() => driver.viewsDetailLayoutActive(), POLL).toBe(0);
      await expect.poll(() => driver.viewsDetailShowsIssue(issue), POLL).toBe(true);
    });

    await test.step("a default view keeps the switch personal, off the custom's record", async () => {
      await wsViewsOpenDetailAs(driver, harness, "all-issues");
      await expect.poll(() => driver.wsViewsDetailTabTitle(), POLL).toContain("All Views");
      expect(await driver.wsViewsDetailLayoutVisible()).toBe(true);
      const before = await serverWorkspaceViewDetail(workspaceSlug, customId, owner.cookie);
      // Switch the default to whichever offered layout it is not on, so the
      // pick is always a real change.
      const current = await driver.viewsDetailLayoutActive();
      const target = current === 0 ? 3 : 0;
      await driver.viewsDetailLayoutPick(target);
      await expect.poll(() => driver.viewsDetailLayoutActive(), POLL).toBe(target);
      // The custom keeps its own stored layout: a default's pick writes the
      // member's personal record, never another record's display payload.
      const after = await serverWorkspaceViewDetail(workspaceSlug, customId, owner.cookie);
      expect(after.display_filters).toEqual(before.display_filters);
    });

    await test.step("a locked custom offers no selector", async () => {
      await serverPlantViewFlags(customId, { is_locked: true });
      await wsViewsOpenDetailAs(driver, harness, customId);
      // The absence assertion needs the header mounted first, or "no
      // selector" would pass merely because nothing had rendered yet.
      await expect.poll(() => driver.wsViewsDetailCrumbs(), POLL).toEqual(["Views", custom]);
      expect(await driver.wsViewsDetailLayoutVisible()).toBe(false);
      await expect.poll(() => driver.viewsDetailShowsIssue(issue), POLL).toBe(true);
    });
  }
);

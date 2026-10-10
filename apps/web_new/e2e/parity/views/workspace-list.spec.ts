// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): workspace views list — combined
// defaults and customs with search, the static defaults, and loading
// and empty behavior. Rows: VIEW-029, VIEW-030, VIEW-031 (row half),
// VIEW-039 (list half). Green on apps/web first.
import { test, expect } from "../fixtures";
import { ROLE, createWorkspaceViewFull, serverWorkspaceViews } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsSeat, wsViewsOpenListAs } from "./support";

const POLL = { timeout: 60_000 };

const DEFAULTS = ["All work items", "Assigned", "Created", "Subscribed"];

test(
  specTitle(["VIEW-029"], "workspace views list shows defaults and customs with a filtering search"),
  { tag: specTags(["VIEW-029"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w29");
    const { owner, workspaceSlug, workspaceName } = harness;
    const alpha = `W29 Alpha ${harness.tag}`;
    const beta = `W29 Beta ${harness.tag}`;
    await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: alpha });
    await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: beta });

    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsListTabTitle(), POLL).toBe(`${workspaceName} - All Views`);
    await expect.poll(() => driver.wsViewsDefaultNames(), POLL).toEqual(DEFAULTS);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual(expect.arrayContaining([alpha, beta]));

    await driver.wsViewsSearchFill("Beta");
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([beta]);
    expect(await driver.wsViewsDefaultNames()).toEqual([]);

    await driver.wsViewsSearchFill("assigned");
    await expect.poll(() => driver.wsViewsDefaultNames(), POLL).toEqual(["Assigned"]);
    expect(await driver.wsViewsListNames()).toEqual([]);

    const serverNames = (await serverWorkspaceViews(workspaceSlug, owner.cookie)).map((v) => v.name);
    expect(serverNames).toEqual(expect.arrayContaining([alpha, beta]));
  }
);

test(
  specTitle(["VIEW-029"], "guests list defaults and their own customs only"),
  { tag: specTags(["VIEW-029"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w29g");
    const { owner, workspaceSlug } = harness;
    const guest = await viewsSeat(harness, ROLE.GUEST, "parity-w29gg");
    await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: `W29g Owner ${harness.tag}` });
    const own = `W29g Own ${harness.tag}`;
    await createWorkspaceViewFull(workspaceSlug, guest.cookie, { name: own });

    await wsViewsOpenListAs(driver, harness, guest);
    await expect.poll(() => driver.wsViewsDefaultNames(), POLL).toEqual(DEFAULTS);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([own]);
    expect((await serverWorkspaceViews(workspaceSlug, guest.cookie)).map((v) => v.name)).toEqual([own]);
  }
);

test(
  specTitle(["VIEW-030"], "the four static defaults always render with detail links and no actions"),
  { tag: specTags(["VIEW-030"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w30");
    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsDefaultNames(), POLL).toEqual(DEFAULTS);
    // Defaults carry no overflow menu (nothing to edit or delete) and no
    // star control exists anywhere on the workspace list (nothing to
    // favorite).
    for (const name of DEFAULTS) {
      expect(await driver.wsViewsDefaultRowHasMenu(name)).toBe(false);
    }
    expect(await driver.wsViewsListStarVisible()).toBe(false);
    expect(await driver.wsViewsListNames()).toEqual([]);
  }
);

test(
  specTitle(["VIEW-031"], "default rows carry no overflow menu"),
  { tag: specTags(["VIEW-031"]) },
  async ({ driver }) => {
    // Gap vs the row's "overflow menu on a default row" wording: default
    // rows render a bare link with no menu trigger; the open/copy pair
    // lives only in the detail header (covered with VIEW-031 there).
    const harness = await viewsHarness("parity-w31");
    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsDefaultNames(), POLL).toEqual(DEFAULTS);
    expect(await driver.wsViewsDefaultRowHasMenu("All work items")).toBe(false);
  }
);

test(
  specTitle(["VIEW-039"], "bug: customs render empty during load with no skeleton"),
  { tag: specTags(["VIEW-039"]) },
  async ({ driver }) => {
    // bug: NEWFRONT-230 — the store computed returns [] (not null) while
    // customs load, so the ViewListLoader branch never runs: holding the
    // list response for 5s shows an empty customs section, then rows pop
    // in. The intended behavior is the VIEW-039 skeleton.
    const harness = await viewsHarness("parity-w39");
    const { owner, workspaceSlug } = harness;
    const custom = `W39 Alpha ${harness.tag}`;
    await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

    await driver.wsViewsDelayNextList(5000);
    await wsViewsOpenListAs(driver, harness);
    expect(await driver.wsViewsSkeletonFlashed(8000)).toBe(false);
    await expect.poll(() => driver.wsViewsListNames(), POLL).toEqual([custom]);
    expect(await driver.wsViewsSkeletonVisible()).toBe(false);
  }
);

test(
  specTitle(["VIEW-039"], "a workspace without customs lists only the four defaults"),
  { tag: specTags(["VIEW-039"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-w39e");
    await wsViewsOpenListAs(driver, harness);
    await expect.poll(() => driver.wsViewsDefaultNames(), POLL).toEqual(DEFAULTS);
    expect(await driver.wsViewsListNames()).toEqual([]);
    expect(await driver.wsViewsSkeletonVisible()).toBe(false);
  }
);

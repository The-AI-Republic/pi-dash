// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): project view detail chrome — render,
// breadcrumb and tab title, the missing/forbidden empty state, the view
// switcher and the private lock. Rows: VIEW-021, VIEW-022, VIEW-023,
// VIEW-024. Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  createIssue,
  createProjectViewFull,
  serverPlantViewFlags,
  serverProjectViewDetail,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenDetailAs, viewsSeat } from "./support";

const POLL = { timeout: 60_000 };

test(
  specTitle(["VIEW-021"], "detail renders the saved layout with its issues, breadcrumb and tab title"),
  { tag: specTags(["VIEW-021"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd21");
    const { owner, workspaceSlug, projectId, projectName } = harness;
    const first = `Vd21 First ${harness.tag}`;
    const second = `Vd21 Second ${harness.tag}`;
    await createIssue(workspaceSlug, projectId, owner.cookie, first);
    await createIssue(workspaceSlug, projectId, owner.cookie, second);
    const name = `Vd21 Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await viewsOpenDetailAs(driver, harness, viewId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toBe(`${projectName} - ${name}`);
    await expect.poll(() => driver.viewsDetailBreadcrumb(), POLL).toEqual([projectName, "Views", name]);
    expect(await driver.viewsDetailErrorTitle()).toBe("");
    await expect.poll(() => driver.visibleIssueNames(), POLL).toEqual(expect.arrayContaining([first, second]));
    expect(await driver.viewsDetailLayoutActive()).toBe(0);

    const detail = await serverProjectViewDetail(workspaceSlug, projectId, viewId, owner.cookie);
    expect(detail.name).toBe(name);
    expect(detail.display_filters).toMatchObject({ layout: "list" });
  }
);

test(
  specTitle(["VIEW-022"], "forbidden view shows the missing-view empty state with a way back"),
  { tag: specTags(["VIEW-022"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd22");
    const { owner, workspaceSlug, projectId } = harness;
    const guest = await viewsSeat(harness, ROLE.GUEST, "parity-vd22g");
    const name = `Vd22 Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    // Guests may not open another member's view: the fetch fails and the
    // detail route renders the missing-view empty state instead.
    await viewsOpenDetailAs(driver, harness, viewId, guest);
    await expect.poll(() => driver.viewsDetailErrorTitle(), POLL).toBe("View does not exist");

    await driver.viewsDetailErrorBack();
    await expect.poll(() => driver.currentUrlPath(), POLL).toMatch(/\/views\/?$/);
    // The guest lands on a rendered list; it is empty because none of the
    // owner's views are visible to them.
    await expect.poll(() => driver.viewsEmptyTitle(), POLL).not.toBe("");
    expect(await driver.viewsListNames()).toEqual([]);
  }
);

test(
  specTitle(["VIEW-022"], "bug: an unknown view id renders a nameless phantom, not the error state"),
  { tag: specTags(["VIEW-022"]) },
  async ({ driver }) => {
    // bug: NEWFRONT-227 — the retrieve endpoint answers 200 with a null
    // body for unknown ids instead of 404, so the detail renders a
    // nameless phantom (no error state, no current-view crumb). The
    // intended behavior is the VIEW-022 error state with a way back.
    const harness = await viewsHarness("parity-vd22b");
    const { owner, workspaceSlug, projectId, projectName } = harness;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: `Vd22b Alpha ${harness.tag}` });

    await viewsOpenDetailAs(driver, harness, "00000000-0000-0000-0000-000000000000");
    await expect.poll(() => driver.viewsDetailEmptyTitle(), POLL).toBe("View work items will appear here");
    expect(await driver.viewsDetailErrorTitle()).toBe("");
    await expect.poll(() => driver.viewsDetailBreadcrumb(), POLL).toEqual([projectName, "Views"]);
  }
);

test(
  specTitle(["VIEW-023"], "view switcher lists every saved view with search and navigates on pick"),
  { tag: specTags(["VIEW-023"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd23");
    const { owner, workspaceSlug, projectId, projectName } = harness;
    const alpha = `Vd23 Alpha ${harness.tag}`;
    const beta = `Vd23 Beta ${harness.tag}`;
    const alphaId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: alpha });
    const betaId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: beta });

    await viewsOpenDetailAs(driver, harness, alphaId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(alpha);

    await driver.viewsDetailSwitcherOpen(alpha);
    const options = await driver.viewsDetailSwitcherOptions();
    expect(options).toHaveLength(2);
    expect(options).toEqual(expect.arrayContaining([alpha, beta]));
    expect(await driver.viewsDetailSwitcherSearchVisible()).toBe(true);
    await driver.viewsDetailSwitcherSearch("Beta");
    await expect.poll(() => driver.viewsDetailSwitcherOptions(), POLL).toEqual([beta]);

    await driver.viewsDetailSwitcherPick(beta);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toBe(`${projectName} - ${beta}`);
    await expect.poll(() => driver.viewsDetailBreadcrumb(), POLL).toEqual([projectName, "Views", beta]);
    expect(await driver.currentUrlPath()).toContain(betaId);
  }
);

test(
  specTitle(["VIEW-024"], "private views show a lock glyph; shared views show none"),
  { tag: specTags(["VIEW-024"]) },
  async ({ driver }) => {
    // The lock's hover tooltip ("Private") never opens under Playwright:
    // base-ui hover tooltips fire nowhere on this page (the layout
    // buttons' tooltips stay shut too), so only its wiring is
    // source-verified. The glyph itself is the asserted behavior.
    const harness = await viewsHarness("parity-vd24");
    const { owner, workspaceSlug, projectId } = harness;
    const locked = `Vd24 Locked ${harness.tag}`;
    const shared = `Vd24 Shared ${harness.tag}`;
    const lockedId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: locked });
    const sharedId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: shared });
    await serverPlantViewFlags(lockedId, { access: 0 });

    await viewsOpenDetailAs(driver, harness, lockedId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(locked);
    expect(await driver.viewsDetailLockVisible()).toBe(true);
    expect((await serverProjectViewDetail(workspaceSlug, projectId, lockedId, owner.cookie)).access).toBe(0);

    await viewsOpenDetailAs(driver, harness, sharedId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(shared);
    expect(await driver.viewsDetailLockVisible()).toBe(false);
    expect((await serverProjectViewDetail(workspaceSlug, projectId, sharedId, owner.cookie)).access).toBe(1);
  }
);

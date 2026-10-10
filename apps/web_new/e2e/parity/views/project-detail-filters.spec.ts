// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): project view detail layouts, display
// options, saved-query editing, adding work items, issue matching and
// save-as. Rows: VIEW-025, VIEW-026, VIEW-027, VIEW-028, VIEW-048.
// Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  createIssue,
  createProjectViewFull,
  patchProject,
  patchProjectView,
  serverIssueNames,
  serverPlantViewFlags,
  serverProjectViewDetail,
  serverSavedViews,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenDetailAs, viewsSeat } from "./support";

const POLL = { timeout: 60_000 };

async function layoutOf(workspaceSlug: string, projectId: string, viewId: string, cookie: string): Promise<unknown> {
  const detail = await serverProjectViewDetail(workspaceSlug, projectId, viewId, cookie);
  return (detail.display_filters as Record<string, unknown> | undefined)?.layout;
}

test(
  specTitle(["VIEW-025"], "layout switch re-renders issues and persists through Update view"),
  { tag: specTags(["VIEW-025"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd25");
    const { owner, workspaceSlug, projectId } = harness;
    const first = `Vd25 First ${harness.tag}`;
    const second = `Vd25 Second ${harness.tag}`;
    await createIssue(workspaceSlug, projectId, owner.cookie, first);
    await createIssue(workspaceSlug, projectId, owner.cookie, second);
    const name = `Vd25 Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await viewsOpenDetailAs(driver, harness, viewId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    await expect.poll(() => driver.visibleIssueNames(), POLL).toEqual(expect.arrayContaining([first, second]));
    expect(await driver.viewsDetailLayoutActive()).toBe(0);

    // Switching to board re-renders the same issues in that layout. The
    // choice is local until Update view saves it with the filter payload.
    await driver.viewsDetailLayoutPick(1);
    await expect.poll(() => driver.viewsDetailLayoutActive(), POLL).toBe(1);
    await expect.poll(() => driver.viewsDetailShowsIssue(first), POLL).toBe(true);
    expect(await driver.viewsDetailShowsIssue(second)).toBe(true);
    expect(await layoutOf(workspaceSlug, projectId, viewId, owner.cookie)).toBe("list");

    await driver.viewsDetailFilterAdd("Priority", "Urgent");
    await driver.viewsDetailUpdateView();
    await expect.poll(() => layoutOf(workspaceSlug, projectId, viewId, owner.cookie), POLL).toBe("kanban");
    expect(
      JSON.stringify((await serverProjectViewDetail(workspaceSlug, projectId, viewId, owner.cookie)).rich_filters)
    ).toContain("urgent");
    expect(await driver.lastToast()).toContain("updated successfully");
  }
);

test(
  specTitle(["VIEW-025", "VIEW-026"], "locked views hide layout and display but keep a working filter toggle"),
  { tag: specTags(["VIEW-025", "VIEW-026"]) },
  async ({ driver }) => {
    // Gap vs the rows' "no switcher / neither control" wording: the
    // filter toggle stays visible on locked views while layout and
    // Display hide; saving stays shut server-side (locked PATCHes fail
    // with 400) and no Update/Save button offers without a dirty query.
    const harness = await viewsHarness("parity-vd25l");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Vd25l Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });
    await serverPlantViewFlags(viewId, { is_locked: true });

    await viewsOpenDetailAs(driver, harness, viewId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    await expect.poll(() => driver.viewsDetailEmptyTitle(), POLL).toBe("View work items will appear here");
    expect(await driver.viewsDetailLayoutVisible()).toBe(false);
    expect(await driver.viewsDetailDisplayVisible()).toBe(false);
    expect(await driver.viewsDetailFiltersToggleVisible()).toBe(true);
    expect(await driver.viewsDetailAddVisible()).toBe(true);
    expect((await serverProjectViewDetail(workspaceSlug, projectId, viewId, owner.cookie)).is_locked).toBe(true);
    await expect(patchProjectView(workspaceSlug, projectId, viewId, owner.cookie, { name })).rejects.toThrow(
      /HTTP 400/
    );
  }
);

test(
  specTitle(["VIEW-026"], "display options match the layout; cycle and module follow project flags"),
  { tag: specTags(["VIEW-026"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd26");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Vd26 Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await viewsOpenDetailAs(driver, harness, viewId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    await expect.poll(() => driver.viewsDetailEmptyTitle(), POLL).toBe("View work items will appear here");

    // Fresh projects ship with cycle and module views off, so their
    // entries are absent (not merely disabled) from the Display panel.
    const listOptions = await driver.viewsDetailDisplayOptions();
    for (const entry of ["Display Properties", "Group by", "Order by", "Show sub-work items"]) {
      expect(listOptions).toContain(entry);
    }
    expect(listOptions).not.toContain("Sub-group by");
    expect(listOptions).not.toContain("Cycle");
    expect(listOptions).not.toContain("Module");

    await driver.viewsDetailLayoutPick(1);
    await expect.poll(() => driver.viewsDetailLayoutActive(), POLL).toBe(1);
    expect(await driver.viewsDetailDisplayOptions()).toContain("Sub-group by");

    await patchProject(workspaceSlug, projectId, owner.cookie, { cycle_view: true, module_view: true });
    await driver.reloadPage();
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    await expect.poll(() => driver.viewsDetailEmptyTitle(), POLL).toBe("View work items will appear here");
    const flagged = await driver.viewsDetailDisplayOptions();
    expect(flagged).toContain("Cycle");
    expect(flagged).toContain("Module");
  }
);

test(
  specTitle(["VIEW-026"], "filter toggle edits the saved work-item query through Update view"),
  { tag: specTags(["VIEW-026"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd26f");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Vd26f Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await viewsOpenDetailAs(driver, harness, viewId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    await expect.poll(() => driver.viewsDetailEmptyTitle(), POLL).toBe("View work items will appear here");
    expect(await driver.viewsDetailFiltersToggleVisible()).toBe(true);

    await driver.viewsDetailFilterAdd("Priority", "Urgent");
    await driver.viewsDetailUpdateView();
    await expect
      .poll(async () => {
        const detail = await serverProjectViewDetail(workspaceSlug, projectId, viewId, owner.cookie);
        return JSON.stringify(detail.rich_filters);
      }, POLL)
      .toContain("urgent");
    expect(await driver.lastToast()).toContain("updated successfully");
  }
);

test(
  specTitle(["VIEW-027"], "add work item is role-gated; created items appear when matching"),
  { tag: specTags(["VIEW-027"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd27");
    const { owner, workspaceSlug, projectId } = harness;
    await patchProject(workspaceSlug, projectId, owner.cookie, { guest_view_all_features: true });
    const guest = await viewsSeat(harness, ROLE.GUEST, "parity-vd27g");
    const member = await viewsSeat(harness, ROLE.MEMBER, "parity-vd27m");
    const name = `Vd27 Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await viewsOpenDetailAs(driver, harness, viewId, guest);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    expect(await driver.viewsDetailErrorTitle()).toBe("");
    expect(await driver.viewsDetailAddVisible()).toBe(false);

    await viewsOpenDetailAs(driver, harness, viewId, member);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    expect(await driver.viewsDetailAddVisible()).toBe(true);

    const created = `Vd27 Item ${harness.tag}`;
    await driver.viewsDetailAddClick();
    expect(await driver.createModalOpen()).toBe(true);
    await driver.fillCreateTitle(created);
    await driver.submitCreateModal();
    await expect.poll(() => driver.visibleIssueNames(), POLL).toEqual(expect.arrayContaining([created]));
    expect(await serverIssueNames(workspaceSlug, projectId, owner.cookie)).toContain(created);
  }
);

test(
  specTitle(["VIEW-028"], "detail lists exactly the issues matching the saved query"),
  { tag: specTags(["VIEW-028"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd28");
    const { owner, workspaceSlug, projectId } = harness;
    const match = `Vd28 Urgent ${harness.tag}`;
    const other = `Vd28 Low ${harness.tag}`;
    await createIssue(workspaceSlug, projectId, owner.cookie, match, { priority: "urgent" });
    await createIssue(workspaceSlug, projectId, owner.cookie, other, { priority: "low" });
    const rich = { and: [{ priority__in: "urgent" }] };
    const name = `Vd28 Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name, rich_filters: rich });

    await viewsOpenDetailAs(driver, harness, viewId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    await expect.poll(() => driver.visibleIssueNames(), POLL).toEqual(expect.arrayContaining([match]));
    expect(await driver.visibleIssueNames()).not.toContain(other);

    // The server holds both issues and the saved query; the detail narrows.
    expect((await serverProjectViewDetail(workspaceSlug, projectId, viewId, owner.cookie)).rich_filters).toEqual(rich);
    expect(await serverIssueNames(workspaceSlug, projectId, owner.cookie)).toEqual(
      expect.arrayContaining([match, other])
    );

    // The same matching set follows into the board layout.
    await driver.viewsDetailLayoutPick(1);
    await expect.poll(() => driver.viewsDetailLayoutActive(), POLL).toBe(1);
    await expect.poll(() => driver.viewsDetailShowsIssue(match), POLL).toBe(true);
    expect(await driver.viewsDetailShowsIssue(other)).toBe(false);
  }
);

test(
  specTitle(["VIEW-048"], "save as duplicates the view with its current filters"),
  { tag: specTags(["VIEW-048"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd48");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Vd48 Alpha ${harness.tag}`;
    const viewId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await viewsOpenDetailAs(driver, harness, viewId);
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
    await expect.poll(() => driver.viewsDetailEmptyTitle(), POLL).toBe("View work items will appear here");
    expect(await driver.viewsDetailSaveAsVisible()).toBe(false);

    await driver.viewsDetailFilterAdd("Priority", "Urgent");
    expect(await driver.viewsDetailSaveAsVisible()).toBe(true);
    await driver.viewsDetailSaveAsClick();
    expect(await driver.viewsDialogTitleValue()).toBe(`${name} 2`);
    await driver.viewsDialogSubmit();
    await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(`${name} 2`);

    const names = (await serverSavedViews(workspaceSlug, projectId, owner.cookie)).map((v) => v.name);
    expect(names).toEqual(expect.arrayContaining([name, `${name} 2`]));
  }
);

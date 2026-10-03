// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-117): the spreadsheet layout — sticky table
// structure (ISS-015), display-driven columns (ISS-016), cell editing
// (ISS-017), header sorting (ISS-018), quick-add plus pagination (ISS-019),
// and keyboard navigation (ISS-020).
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  requireMentionMember,
  seedProjectUserProperties,
  serverCreateIssue,
  serverCreateState,
  serverDeleteIssue,
  serverDeleteState,
  serverIssueDetails,
  serverIssues,
  serverListStates,
  serverPatchIssue,
  serverPatchProject,
  serverPatchProjectUserProperties,
  serverProjectDetails,
  serverProjectUserProperties,
  sessionBrowserCookies,
  signInSession,
  uniqueSuffix,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.use({ viewport: { width: 1600, height: 900 } });

async function resetPrefs(workspaceSlug: string, projectId: string, session: string): Promise<void> {
  const seed = seedProjectUserProperties();
  await serverPatchProjectUserProperties(workspaceSlug, projectId, session, {
    display_filters: seed.display_filters,
    display_properties: seed.display_properties,
  });
}

async function occupantIds(
  workspaceSlug: string,
  projectId: string,
  session: string,
  stateId: string
): Promise<string[]> {
  const found: string[] = [];
  const issues = await serverIssues(workspaceSlug, projectId, session);
  for (const issue of issues) {
    const details = await serverIssueDetails(workspaceSlug, projectId, issue.id, session).catch(() => null);
    if (details?.stateId === stateId) found.push(`${issue.name} (${issue.id})`);
  }
  return found;
}

/** Move every live occupant of a temp state back to Todo, wait for the
 *  moves to land, then delete the state. The issue PATCH lands
 *  asynchronously server-side, so a delete can 400 while a move is still
 *  in flight — restore, poll, and delete retry together, and a persistent
 *  failure names the remaining occupants instead of a bare 400. */
async function deleteTempState(
  workspaceSlug: string,
  projectId: string,
  session: string,
  stateId: string
): Promise<void> {
  const states = await serverListStates(workspaceSlug, projectId, session);
  const todoId = states.find((row) => row.name === "Todo")?.id ?? "";
  for (let attempt = 0; attempt < 3; attempt++) {
    if (todoId) {
      const issues = await serverIssues(workspaceSlug, projectId, session);
      for (const issue of issues) {
        const details = await serverIssueDetails(workspaceSlug, projectId, issue.id, session).catch(() => null);
        if (details?.stateId === stateId) {
          await serverPatchIssue(workspaceSlug, projectId, issue.id, { state_id: todoId }, session);
        }
      }
      await expect
        .poll(async () => occupantIds(workspaceSlug, projectId, session, stateId), { timeout: 30_000 })
        .toEqual([]);
    }
    try {
      await serverDeleteState(workspaceSlug, projectId, stateId, session);
      return;
    } catch {
      // Still occupied or settling — the next attempt restores again.
    }
  }
  const stuck = await occupantIds(workspaceSlug, projectId, session, stateId).catch(() => ["<unreadable>"]);
  throw new Error(`[parity] temp state ${stateId} stayed occupied by: ${stuck.join(", ") || "<none>"}.`);
}

/** Remove temp states leaked by earlier failed runs (best-effort; a red
 *  run's strays must not break the next run's reads). */
async function deleteStrayTempStates(
  workspaceSlug: string,
  projectId: string,
  session: string,
  keepStateId = ""
): Promise<void> {
  const states = await serverListStates(workspaceSlug, projectId, session).catch(() => []);
  for (const row of states) {
    // The keep id is the current test's own temp state: without the
    // exclusion the janitor eats it at open time, the body then fails to
    // find its option, and the finally's re-delete 400s (deleting a
    // soft-deleted state is a 400, not a 404) and masks the body error.
    if (row.id === keepStateId) continue;
    if (row.name.startsWith("Parity Sheet ") || row.name.startsWith("Parity Sort ")) {
      await deleteTempState(workspaceSlug, projectId, session, row.id).catch(() => {});
    }
  }
}

async function openSheet(
  driver: Pick<ParityDriver, "openAuthenticated">,
  seed: Pick<ParitySeedFacts, "workspaceSlug" | "projectId">,
  session: string,
  keepStateId = ""
): Promise<void> {
  await deleteStrayTempStates(seed.workspaceSlug, seed.projectId, session, keepStateId);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
    display_filters: { layout: "spreadsheet", group_by: null, order_by: "sort_order" },
  });
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
    sessionBrowserCookies(session)
  );
}

test(
  specTitle(["ISS-015"], "sheet structure: headers, sticky columns, sub-issues"),
  { tag: specTags(["ISS-015"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await openSheet(driver, seed, session);
    // The sheet renders after the header chrome the navigation waits
    // settle on, so the first read polls instead of asserting once.
    await expect.poll(async () => driver.layoutsSpreadsheetVisible(), { timeout: 300_000 }).toEqual(true);

    await test.step("headers start with the work-item column, then properties", async () => {
      const headers = await driver.layoutsSheetHeaders();
      expect(headers[0]).toEqual("Work items");
      expect(headers).toEqual(
        expect.arrayContaining(["State", "Priority", "Assignees", "Labels", "Start date", "Due date"])
      );
    });

    await test.step("first column and header row stick; scrolling shadows the first column", async () => {
      expect(await driver.layoutsSheetFirstColumnSticky()).toEqual(true);
      expect(await driver.layoutsSheetHeaderSticky()).toEqual(true);
      expect(await driver.layoutsSheetFirstColumnShadowed()).toEqual(false);
      await driver.layoutsSheetScrollRight();
      expect(await driver.layoutsSheetFirstColumnShadowed()).toEqual(true);
    });

    await test.step("rows render every issue", async () => {
      expect(await driver.layoutsSheetRowNames()).toEqual(expect.arrayContaining([...seed.issueNames]));
    });

    const suffix = uniqueSuffix();
    const parentName = `Parity sheet parent ${suffix}`;
    const childName = `Parity sheet child ${suffix}`;
    const parentId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, parentName);
    const childId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, childName);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, childId, { parent_id: parentId }, session);
    try {
      await test.step("sub-issues expand inline", async () => {
        await driver.layoutsReloadIssues();
        expect(await driver.layoutsSheetHasSubIssueToggle(seed.issueNames[0] ?? "")).toEqual(false);
        expect(await driver.layoutsSheetHasSubIssueToggle(parentName)).toEqual(true);
        await driver.layoutsSheetExpandSubIssues(parentName);
        expect(await driver.layoutsSheetSubIssueNames(parentName)).toContain(childName);
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, childId, session);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, parentId, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-015", "ISS-013"], "bug:NEWFRONT-162 nested expansion fetches but does not render grandchildren"),
  { tag: specTags(["ISS-015", "ISS-013"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const suffix = uniqueSuffix();
    const names = [0, 1, 2].map((level) => `Parity nest L${level} ${suffix}`);
    const ids: string[] = [];
    try {
      for (const name of names) ids.push(await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name));
      for (let level = 1; level < ids.length; level++) {
        await serverPatchIssue(
          seed.workspaceSlug,
          seed.projectId,
          ids[level] ?? "",
          { parent_id: ids[level - 1] ?? "" },
          session
        );
      }
      await openSheet(driver, seed, session);
      // Level 1 expands inline.
      await driver.layoutsSheetExpandSubIssues(names[0] ?? "");
      expect(await driver.layoutsSheetSubIssueNames(names[0] ?? "")).toContain(names[1]);
      // Level 2 does not: the nested toggle fetches its children (HTTP
      // 200) but never renders them — no row, no peek, no navigation.
      // The settle below is the honest absence pin: a passing render
      // would land within seconds of the fetch, so nothing after 15s
      // means nothing renders. Intended behavior is cap-3 inline with
      // peek past it; when NEWFRONT-162 is fixed, flip this back to the
      // stepwise expansion plus the depth-3 peek assertion.
      expect(await driver.layoutsSheetHasSubIssueToggle(names[1] ?? "")).toEqual(true);
      await driver.layoutsSheetToggleSubIssues(names[1] ?? "");
      await new Promise((resolve) => setTimeout(resolve, 15_000));
      expect(await driver.layoutsSheetSubIssueNames(names[1] ?? "")).not.toContain(names[2]);
      expect(await driver.layoutsSheetRowNames()).not.toContain(names[2]);
      expect(await driver.layoutsPeekVisible()).toEqual(false);
    } finally {
      for (const id of [...ids].reverse()) await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-016"], "columns follow display properties and project features"),
  { tag: specTags(["ISS-016"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const flags = await serverProjectDetails(seed.workspaceSlug, seed.projectId, session);
    // The seed leaves the views off; the test needs them on, and
    // restores whatever it found.
    await serverPatchProject(seed.workspaceSlug, seed.projectId, session, { cycle_view: true, module_view: true });
    try {
      await openSheet(driver, seed, session);
      expect(await driver.layoutsSheetHeaders()).toEqual(expect.arrayContaining(["State", "Cycle", "Modules"]));

      await test.step("disabling a display property drops its column", async () => {
        await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
          display_filters: { layout: "spreadsheet", group_by: null, order_by: "sort_order" },
          display_properties: { state: false },
        });
        await driver.layoutsReloadIssues();
        expect(await driver.layoutsSheetHeaders()).not.toContain("State");
        const props = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session);
        expect(props.displayProperties["state"]).toEqual(false);
      });

      await test.step("re-enabling restores the column", async () => {
        await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
          display_filters: { layout: "spreadsheet", group_by: null, order_by: "sort_order" },
          display_properties: {},
        });
        await driver.layoutsReloadIssues();
        expect(await driver.layoutsSheetHeaders()).toContain("State");
      });

      await test.step("cycle/module columns need their project features", async () => {
        await serverPatchProject(seed.workspaceSlug, seed.projectId, session, {
          cycle_view: false,
          module_view: false,
        });
        const stored = await serverProjectDetails(seed.workspaceSlug, seed.projectId, session);
        expect(stored.cycleView).toEqual(false);
        expect(stored.moduleView).toEqual(false);
        await driver.layoutsReloadIssues();
        const headers = await driver.layoutsSheetHeaders();
        expect(headers).not.toContain("Cycle");
        expect(headers).not.toContain("Modules");
        expect(headers).toContain("State");
      });
    } finally {
      await serverPatchProject(seed.workspaceSlug, seed.projectId, session, {
        cycle_view: flags.cycleView,
        module_view: flags.moduleView,
      });
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-017"], "edit state, priority, assignee, and due date cells"),
  { tag: specTags(["ISS-017"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const member = requireMentionMember(seed);
    const first = seed.issueNames[0] ?? "";
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    const firstId = rows.find((row) => row.name === first)?.id ?? "";
    const stateName = `Parity Sheet ${uniqueSuffix()}`;
    const stateId = await serverCreateState(seed.workspaceSlug, seed.projectId, session, stateName, "started");
    // A red run leaks field state (its end-of-body restore never runs),
    // so the test resets the seed issue's fields up front instead of
    // assuming a pristine project.
    await serverPatchIssue(
      seed.workspaceSlug,
      seed.projectId,
      firstId,
      { priority: "none", assignee_ids: [], target_date: null },
      session
    );
    try {
      await openSheet(driver, seed, session, stateId);
      expect(await driver.layoutsSheetCellEditable(first, "State")).toEqual(true);
      expect(await driver.layoutsSheetCellText(first, "State")).toEqual("Todo");
      expect(await driver.layoutsSheetCellText(first, "Priority")).toEqual("None");

      // The sheet updates its cells optimistically, so every server
      // read after a UI edit polls until the PATCH lands.
      await driver.layoutsSheetCellSetState(first, stateName);
      expect(await driver.layoutsSheetCellText(first, "State")).toEqual(stateName);
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, firstId, session)).stateId, {
          timeout: 30_000,
        })
        .toEqual(stateId);
      expect(await driver.layoutsSheetFocusedCell()).toEqual({ issueName: first, column: "State" });

      await driver.layoutsSheetCellSetPriority(first, "High");
      expect(await driver.layoutsSheetCellText(first, "Priority")).toEqual("High");
      await expect
        .poll(
          async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, firstId, session)).priority ?? "",
          { timeout: 30_000 }
        )
        .toEqual(expect.stringMatching(/^high$/i));

      await test.step("assignee cells write the member", async () => {
        await serverPatchIssue(seed.workspaceSlug, seed.projectId, firstId, { assignee_ids: [] }, session);
        // The driver addresses members by display name; the seeded member
        // carries one the assignee picker shows. Unlike priority's "None",
        // the empty assignee cell renders its column label (a nested
        // "Assignees" button inside the cell trigger).
        const before = await driver.layoutsSheetCellText(first, "Assignees");
        expect(before).toEqual("Assignees");
        await driver.layoutsSheetCellSetAssignee(first, member.displayName);
        await expect
          .poll(
            async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, firstId, session)).assigneeIds,
            { timeout: 30_000 }
          )
          .toContain(member.id);
      });

      await test.step("due-date cells write the date", async () => {
        const due = new Date(Date.now() + 30 * 24 * 3600 * 1000);
        const iso = due.toISOString().slice(0, 10);
        await driver.layoutsSheetCellSetDueDate(first, iso);
        await expect
          .poll(
            async () => (await serverIssueDetails(seed.workspaceSlug, seed.projectId, firstId, session)).targetDate,
            { timeout: 30_000 }
          )
          .toEqual(iso);
        expect(await driver.layoutsSheetCellText(first, "Due date")).toContain(String(due.getUTCDate()));
      });

      await test.step("teardown restores the seed issue", async () => {
        const states = await serverListStates(seed.workspaceSlug, seed.projectId, session);
        const todoId = states.find((row) => row.name === "Todo")?.id ?? "";
        await serverPatchIssue(
          seed.workspaceSlug,
          seed.projectId,
          firstId,
          // Priority clears with "none", not null (non-null CharField;
          // null 400s the whole PATCH and the state restore never lands).
          // The state move itself lands asynchronously; deleteTempState in
          // the finally waits for it before deleting the temp state.
          { state_id: todoId, priority: "none", assignee_ids: [], target_date: null },
          session
        );
      });
    } finally {
      await deleteTempState(seed.workspaceSlug, seed.projectId, session, stateId);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-017"], "read-only cells, sub-issue navigation, guest cells"),
  { tag: specTags(["ISS-017"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const first = seed.issueNames[0] ?? "";
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    const firstId = rows.find((row) => row.name === first)?.id ?? "";
    const suffix = uniqueSuffix();
    const childId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, `Parity sheet nav ${suffix}`);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, childId, { parent_id: firstId }, session);
    try {
      await openSheet(driver, seed, session);
      expect(await driver.layoutsSheetCellEditable(first, "Created on")).toEqual(false);

      await test.step("activating the sub-issue count navigates to the sub-issues", async () => {
        const before = await driver.layoutsCurrentUrl();
        await driver.layoutsSheetOpenSubIssueCount(first);
        const after = await driver.layoutsCurrentUrl();
        expect(after).not.toEqual(before);
        // The detail route canonicalizes to the browse-by-identifier
        // path (PAR-1 style), not the uuid the driver clicked from.
        const project = await serverProjectDetails(seed.workspaceSlug, seed.projectId, session);
        const sequence = rows.find((row) => row.id === firstId)?.sequenceId ?? -1;
        expect(after).toContain(`${project.identifier}-${sequence}`);
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, childId, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }

    if (!seed.guestEmail || !seed.guestPassword) {
      throw new Error("[parity] seed facts carry no guest; re-run the stack seed step (see stack/README.md).");
    }
    // Guests see only issues they created (the server drops guest
    // assignees silently and guests cannot create), so the owner flips
    // guest_view_all_features for this step and restores it after. Prefs
    // are per-user: the sheet layout is set on the guest's own prefs.
    await serverPatchProject(seed.workspaceSlug, seed.projectId, session, { guest_view_all_features: true });
    const guestSession = await signInSession(seed.guestEmail, seed.guestPassword);
    try {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, guestSession, {
        display_filters: { layout: "spreadsheet", group_by: null, order_by: "sort_order" },
      });
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
        sessionBrowserCookies(guestSession)
      );
      await expect.poll(async () => driver.layoutsSpreadsheetVisible(), { timeout: 300_000 }).toEqual(true);
      expect(await driver.layoutsSheetCellEditable(first, "State")).toEqual(false);
      expect(await driver.layoutsSheetCellEditable(first, "Priority")).toEqual(false);
    } finally {
      await serverPatchProject(seed.workspaceSlug, seed.projectId, session, { guest_view_all_features: false });
      await resetPrefs(seed.workspaceSlug, seed.projectId, guestSession);
    }
  }
);

test(
  specTitle(["ISS-018"], "sort by a column header, then clear the sort"),
  { tag: specTags(["ISS-018"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const stateName = `Parity Sort ${uniqueSuffix()}`;
    const stateId = await serverCreateState(seed.workspaceSlug, seed.projectId, session, stateName, "started");
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    const second = seed.issueNames[1] ?? "";
    const secondId = rows.find((row) => row.name === second)?.id ?? "";
    try {
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, secondId, { state_id: stateId }, session);
      await openSheet(driver, seed, session, stateId);
      expect(await driver.layoutsSheetSortMarker("State")).toEqual("none");
      expect(await driver.layoutsSheetSortMenu("State")).toHaveLength(2);

      await driver.layoutsSheetSort("State", "ascending");
      expect(await driver.layoutsSheetSortMarker("State")).toEqual("ascending");
      expect(
        (await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).displayFilters["order_by"]
      ).toEqual("state__name");
      expect(await driver.layoutsSheetSortMenu("State")).toContain("Clear sorting");
      // Rows re-render on every sort; the order reads poll until the
      // seed rows are all present instead of racing the transition.
      await expect
        .poll(async () => driver.layoutsSheetRowNames(), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([second, seed.issueNames[0] ?? ""]));
      const asc = await driver.layoutsSheetRowNames();
      expect(asc.indexOf(second)).toBeLessThan(asc.indexOf(seed.issueNames[0] ?? ""));

      await driver.layoutsSheetSort("State", "descending");
      expect(await driver.layoutsSheetSortMarker("State")).toEqual("descending");
      expect(
        (await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).displayFilters["order_by"]
      ).toEqual("-state__name");
      await expect
        .poll(async () => driver.layoutsSheetRowNames(), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([second, seed.issueNames[0] ?? ""]));
      const desc = await driver.layoutsSheetRowNames();
      expect(desc.indexOf(second)).toBeGreaterThan(desc.indexOf(seed.issueNames[0] ?? ""));

      await driver.layoutsSheetClearSort("State");
      expect(await driver.layoutsSheetSortMarker("State")).toEqual("none");
      expect(
        (await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).displayFilters["order_by"]
      ).toEqual("-created_at");
      await expect
        .poll(async () => driver.layoutsSheetRowNames(), { timeout: 300_000 })
        .toEqual([...seed.issueNames].reverse());
    } finally {
      await deleteTempState(seed.workspaceSlug, seed.projectId, session, stateId);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-019", "ISS-015"], "sheet quick-add and infinite pagination over a virtualized table"),
  { tag: specTags(["ISS-019", "ISS-015"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    try {
      await openSheet(driver, seed, session);

      const title = `Parity sheetadd ${uniqueSuffix()}`;
      await driver.layoutsSheetQuickAdd(title);
      expect(await driver.layoutsSheetRowNames()).toContain(title);
      const created = (await serverIssues(seed.workspaceSlug, seed.projectId, session)).find(
        (row) => row.name === title
      );
      expect(created?.id ?? "").not.toEqual("");
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, created?.id ?? "", session);

      const prefix = `Parity sheetpage ${uniqueSuffix()}`;
      const createdIds: string[] = [];
      // Sequential: concurrent creates interleave sort_order, so only
      // sequential creation keeps the tail (max sequence) last.
      for (let batch = 0; batch < 102; batch++) {
        createdIds.push(await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, `${prefix} ${batch}`));
      }
      try {
        await driver.layoutsReloadIssues();
        // 105 stored, but virtualization renders only the visible window.
        const rendered = await driver.layoutsSheetRowNames();
        expect(rendered.length).toBeGreaterThan(0);
        expect(rendered.length).toBeLessThan(105);
        const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
        const tailName = all
          .filter((row) => row.name.startsWith(prefix))
          .sort((a, b) => b.sequenceId - a.sequenceId)[0]?.name;
        expect(tailName).toBeDefined();
        for (let scroll = 0; scroll < 8; scroll++) {
          if ((await driver.layoutsSheetRowNames()).includes(tailName ?? "")) break;
          await driver.layoutsSheetScrollEnd();
        }
        expect(await driver.layoutsSheetRowNames()).toContain(tailName);
      } finally {
        for (let batch = 0; batch < createdIds.length; batch += 12) {
          await Promise.all(
            createdIds
              .slice(batch, batch + 12)
              .map((id) => serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session))
          );
        }
      }
    } finally {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-020"], "move sheet focus with the arrow keys"),
  { tag: specTags(["ISS-020"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await openSheet(driver, seed, session);
    const first = seed.issueNames[0] ?? "";
    const second = seed.issueNames[1] ?? "";

    await driver.layoutsSheetFocusCell(first, "State");
    expect(await driver.layoutsSheetFocusedCell()).toEqual({ issueName: first, column: "State" });
    await driver.layoutsSheetPressArrow("right");
    expect(await driver.layoutsSheetFocusedCell()).toEqual({ issueName: first, column: "Priority" });
    await driver.layoutsSheetPressArrow("down");
    expect(await driver.layoutsSheetFocusedCell()).toEqual({ issueName: second, column: "Priority" });
    await driver.layoutsSheetPressArrow("left");
    expect(await driver.layoutsSheetFocusedCell()).toEqual({ issueName: second, column: "State" });
    await driver.layoutsSheetPressArrow("up");
    expect(await driver.layoutsSheetFocusedCell()).toEqual({ issueName: first, column: "State" });

    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

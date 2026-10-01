// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-117): the list layout — grouping into
// collapsible sections (ISS-008), empty-group visibility (ISS-009),
// pagination (ISS-010), inline quick-add (ISS-011), rows (ISS-013), and
// inline property editing (ISS-014). ISS-007 stays covered by
// sign-in-and-list.spec.ts.
import { test, expect } from "../fixtures";
import {
  seedProjectUserProperties,
  serverCreateIssue,
  serverCreateState,
  serverDeleteIssue,
  serverDeleteState,
  serverIssueDetails,
  serverIssueNames,
  serverIssues,
  serverListStates,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverWorkspaceUserId,
  sessionBrowserCookies,
  signInSession,
  uniqueSuffix,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

// The desktop header (segmented layout control) only renders above the
// content-area breakpoint; the default 1280px viewport shows the compact
// dropdown instead, which the mobile spec covers separately.
test.use({ viewport: { width: 1600, height: 900 } });

async function resetPrefs(workspaceSlug: string, projectId: string, session: string): Promise<void> {
  const seed = seedProjectUserProperties();
  await serverPatchProjectUserProperties(workspaceSlug, projectId, session, {
    display_filters: seed.display_filters,
    display_properties: seed.display_properties,
  });
}

test(
  specTitle(["ISS-011"], "quick-add an issue from the flat list"),
  { tag: specTags(["ISS-011"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      sessionBrowserCookies(session)
    );

    const title = `Parity quickadd ${uniqueSuffix()}`;
    await test.step("create through the quick-add form", async () => {
      await driver.layoutsListQuickAdd(title);
      expect(await driver.layoutsListGroupIssueNames("All work items")).toContain(title);
    });

    let createdId = "";
    await test.step("the server stored it in the project default state", async () => {
      const server = await serverIssueNames(seed.workspaceSlug, seed.projectId, session);
      expect(server).toContain(title);
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      createdId = rows.find((row) => row.name === title)?.id ?? "";
      expect(createdId).not.toEqual("");
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, createdId, session);
      const first = rows.find((row) => row.name === seed.issueNames[0]);
      const firstDetails = await serverIssueDetails(seed.workspaceSlug, seed.projectId, first?.id ?? "", session);
      expect(details.stateId).toEqual(firstDetails.stateId);
    });

    await test.step("teardown removes the created issue", async () => {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, createdId, session);
    });
  }
);

test(
  specTitle(["ISS-011"], "quick-add inherits the section grouping value"),
  { tag: specTags(["ISS-011"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const stateName = `Parity Doing ${uniqueSuffix()}`;
    const stateId = await serverCreateState(seed.workspaceSlug, seed.projectId, session, stateName, "started");
    // Empty sections render headers only (no rows, no quick-add), so the
    // section holds a seed issue before the quick-add runs; the created
    // issue inheriting the section's state is what the test proves.
    const seedTitle = `Parity sectionseed ${uniqueSuffix()}`;
    const seedId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, seedTitle);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, seedId, { state_id: stateId }, session);
    try {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: { layout: "list", group_by: "state", order_by: "sort_order", show_empty_groups: true },
      });
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
        sessionBrowserCookies(session)
      );
      await expect
        .poll(async () => driver.layoutsListGroups(), { timeout: 300_000 })
        .toEqual(expect.arrayContaining(["Todo", stateName]));

      const title = `Parity sectionadd ${uniqueSuffix()}`;
      await driver.layoutsListQuickAdd(title, stateName);
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const createdId = rows.find((row) => row.name === title)?.id ?? "";
      expect(createdId).not.toEqual("");
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, createdId, session);
      expect(details.stateId).toEqual(stateId);

      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, createdId, session);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, seedId, session).catch(() => {});
      await serverDeleteState(seed.workspaceSlug, seed.projectId, stateId, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-008"], "collapse and expand a list group"),
  { tag: specTags(["ISS-008"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
      display_filters: { layout: "list", group_by: "state", order_by: "sort_order" },
    });
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      sessionBrowserCookies(session)
    );
    await expect.poll(async () => driver.layoutsListGroups(), { timeout: 300_000 }).toEqual(["Todo"]);

    await test.step("collapse hides the rows", async () => {
      expect(await driver.layoutsListGroupExpanded("Todo")).toEqual(true);
      await driver.layoutsListToggleGroup("Todo");
      expect(await driver.layoutsListGroupExpanded("Todo")).toEqual(false);
      expect(await driver.layoutsListGroupIssueNames("Todo")).toEqual([]);
    });

    await test.step("expand shows them again", async () => {
      await driver.layoutsListToggleGroup("Todo");
      expect(await driver.layoutsListGroupExpanded("Todo")).toEqual(true);
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("Todo"), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });

    await test.step("the collapsed state survives reload and is shared with the board", async () => {
      await driver.layoutsListToggleGroup("Todo");
      await driver.layoutsReloadIssues();
      expect(await driver.layoutsListGroupExpanded("Todo")).toEqual(false);
      await driver.layoutsSwitchTo("kanban");
      const visible = await driver.visibleIssueNames();
      for (const name of seed.issueNames) expect(visible).not.toContain(name);
      await driver.layoutsSwitchTo("list");
      await driver.layoutsListToggleGroup("Todo");
      expect(await driver.layoutsListGroupExpanded("Todo")).toEqual(true);
    });

    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

test(specTitle(["ISS-009"], "hide and show empty groups"), { tag: specTags(["ISS-009"]) }, async ({ driver, seed }) => {
  test.setTimeout(720_000);
  const session = await signInSession(seed.email, seed.password);
  const stateName = `Parity Empty ${uniqueSuffix()}`;
  const stateId = await serverCreateState(seed.workspaceSlug, seed.projectId, session, stateName, "started");
  try {
    await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
      display_filters: { layout: "list", group_by: "state", order_by: "sort_order", show_empty_groups: false },
    });
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      sessionBrowserCookies(session)
    );
    await expect.poll(async () => driver.layoutsListGroups(), { timeout: 300_000 }).toEqual(["Todo"]);

    await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
      display_filters: { layout: "list", group_by: "state", order_by: "sort_order", show_empty_groups: true },
    });
    await driver.layoutsReloadIssues();
    await expect.poll(async () => driver.layoutsListGroups(), { timeout: 300_000 }).toHaveLength(2);
    await expect
      .poll(async () => driver.layoutsListGroups(), { timeout: 300_000 })
      .toEqual(expect.arrayContaining(["Todo", stateName]));
  } finally {
    await serverDeleteState(seed.workspaceSlug, seed.projectId, stateId, session);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
});

test(
  specTitle(["ISS-010"], "paginate grouped and flat lists"),
  { tag: specTags(["ISS-010"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const prefix = `Parity page ${uniqueSuffix()}`;
    const createdIds: string[] = [];
    try {
      await test.step("seed 102 extra issues", async () => {
        // Sequential: concurrent creates interleave sort_order, so only
        // sequential creation keeps the tail (max sequence) last.
        for (let batch = 0; batch < 102; batch++) {
          createdIds.push(await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, `${prefix} ${batch}`));
        }
        expect(await serverIssueNames(seed.workspaceSlug, seed.projectId, session)).toHaveLength(105);
      });
      // The last page's tail proves every page loaded; rows render in
      // server order, so the tail name comes from the server, not a guess.
      const all = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const tailName = all
        .filter((row) => row.name.startsWith(prefix))
        .sort((a, b) => b.sequenceId - a.sequenceId)[0]?.name;
      expect(tailName).toBeDefined();

      await test.step("grouped lists page through an explicit row", async () => {
        await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
          display_filters: { layout: "list", group_by: "state", order_by: "sort_order" },
        });
        await driver.openAuthenticated(
          `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
          sessionBrowserCookies(session)
        );
        await expect.poll(async () => driver.layoutsListGroupHasLoadMore("Todo"), { timeout: 300_000 }).toEqual(true);
        await driver.layoutsListGroupLoadMore("Todo");
        expect(await driver.layoutsListGroupHasLoadMore("Todo")).toEqual(true);
        await driver.layoutsListGroupLoadMore("Todo");
        expect(await driver.layoutsListGroupHasLoadMore("Todo")).toEqual(false);
        await expect
          .poll(async () => driver.layoutsListGroupIssueNames("Todo"), { timeout: 300_000 })
          .toContain(tailName);
      });

      await test.step("the flat list auto-loads on scroll", async () => {
        await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
          display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
        });
        await driver.layoutsReloadIssues();
        // Rows first: the no-row read below must not pass on a slow load.
        await expect
          .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
          .not.toHaveLength(0);
        expect(await driver.layoutsListGroupHasLoadMore("All work items")).toEqual(false);
        for (let scroll = 0; scroll < 4; scroll++) {
          if ((await driver.layoutsListGroupIssueNames("All work items")).includes(tailName ?? "")) break;
          await driver.layoutsListScrollEnd();
        }
        await expect
          .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
          .toContain(tailName);
      });
    } finally {
      await test.step("teardown removes the extra issues and prefs", async () => {
        for (let batch = 0; batch < createdIds.length; batch += 12) {
          await Promise.all(
            createdIds
              .slice(batch, batch + 12)
              .map((id) => serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session))
          );
        }
        await resetPrefs(seed.workspaceSlug, seed.projectId, session);
      });
    }
  }
);

test(
  specTitle(["ISS-013"], "rows link, peek, and expand sub-issues"),
  { tag: specTags(["ISS-013"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      sessionBrowserCookies(session)
    );
    const first = seed.issueNames[0] ?? "";

    await test.step("rows are links that open peek", async () => {
      const href = await driver.layoutsRowHref(first);
      expect(href).toContain("/browse/");
      await driver.layoutsRowOpenPeek(first);
      expect(await driver.layoutsPeekVisible()).toEqual(true);
      expect(await driver.layoutsPeekTitle()).toEqual(first);
      await driver.layoutsPeekClose();
      expect(await driver.layoutsPeekVisible()).toEqual(false);
    });

    const suffix = uniqueSuffix();
    const parentId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, `Parity parent ${suffix}`);
    const childName = `Parity child ${suffix}`;
    const childId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, childName);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, childId, { parent_id: parentId }, session);
    try {
      await test.step("sub-issues expand inline", async () => {
        await driver.layoutsReloadIssues();
        const parentName = `Parity parent ${suffix}`;
        expect(await driver.layoutsRowHasSubIssueToggle(seed.issueNames[1] ?? "")).toEqual(false);
        expect(await driver.layoutsRowHasSubIssueToggle(parentName)).toEqual(true);
        await driver.layoutsRowExpandSubIssues(parentName);
        expect(await driver.layoutsRowSubIssueNames(parentName)).toContain(childName);
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, childId, session);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, parentId, session);
    }
  }
);

test(
  specTitle(["ISS-014"], "edit state and priority inline on a row"),
  { tag: specTags(["ISS-014"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      sessionBrowserCookies(session)
    );
    const first = seed.issueNames[0] ?? "";
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    const firstId = rows.find((row) => row.name === first)?.id ?? "";
    const stateName = `Parity Doing ${uniqueSuffix()}`;
    const stateId = await serverCreateState(seed.workspaceSlug, seed.projectId, session, stateName, "started");
    try {
      expect(await driver.layoutsRowCanEditState(first)).toEqual(true);
      expect(await driver.layoutsRowState(first)).toEqual("Todo");
      expect(await driver.layoutsRowPriority(first)).toEqual("None");

      await driver.layoutsRowSetState(first, stateName);
      expect(await driver.layoutsRowState(first)).toEqual(stateName);
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, firstId, session)).stateId).toEqual(stateId);

      await driver.layoutsRowSetPriority(first, "High");
      expect(await driver.layoutsRowPriority(first)).toEqual("High");
      const priority = (await serverIssueDetails(seed.workspaceSlug, seed.projectId, firstId, session)).priority;
      expect((priority ?? "").toLowerCase()).toEqual("high");

      await driver.layoutsRowSetState(first, "Todo");
      await driver.layoutsRowSetPriority(first, "None");
    } finally {
      const states = await serverListStates(seed.workspaceSlug, seed.projectId, session).catch(() => []);
      const todoId = states.find((row) => row.name === "Todo")?.id;
      if (todoId) {
        await serverPatchIssue(
          seed.workspaceSlug,
          seed.projectId,
          firstId,
          { state_id: todoId, priority: null },
          session
        ).catch(() => {});
      }
      await serverDeleteState(seed.workspaceSlug, seed.projectId, stateId, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-014"], "guests cannot inline-edit rows"),
  { tag: specTags(["ISS-014"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    if (!seed.guestEmail || !seed.guestPassword) {
      throw new Error("[parity] seed facts carry no guest; re-run the stack seed step (see stack/README.md).");
    }
    // Guests only see issues assigned to them, so the owner shares the
    // first seed issue before the guest opens the list.
    const ownerSession = await signInSession(seed.email, seed.password);
    const guestId = await serverWorkspaceUserId(seed.workspaceSlug, seed.guestEmail, ownerSession);
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, ownerSession);
    const first = seed.issueNames[0] ?? "";
    const firstId = rows.find((row) => row.name === first)?.id ?? "";
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, firstId, { assignee_ids: [guestId] }, ownerSession);
    const guestSession = await signInSession(seed.guestEmail, seed.guestPassword);
    try {
      await resetPrefs(seed.workspaceSlug, seed.projectId, ownerSession);
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
        sessionBrowserCookies(guestSession)
      );
      expect(await driver.layoutsRowState(first)).toEqual("Todo");
      expect(await driver.layoutsRowCanEditState(first)).toEqual(false);
    } finally {
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, firstId, { assignee_ids: [] }, ownerSession);
    }
  }
);

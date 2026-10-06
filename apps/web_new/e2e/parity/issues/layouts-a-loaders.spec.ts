// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-117): loaders and freshness — initial
// skeletons plus empty routing (ISS-004), mutation and pagination loaders
// (ISS-005), and fresh/optimistic rows (ISS-006). Loader windows are
// caught deterministically by stalling the API through the driver: every
// stall is per-request, so slow hosts only shift the window.
import { test, expect } from "../fixtures";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  seedProjectUserProperties,
  serverCreateIssue,
  serverCreateProject,
  serverCreateState,
  serverDeleteIssue,
  serverDeleteProject,
  serverDeleteState,
  serverIssues,
  serverPatchProjectUserProperties,
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

function projectIssuesPath(seed: Pick<ParitySeedFacts, "workspaceSlug" | "projectId">): string {
  return `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;
}

test(
  specTitle(["ISS-004"], "initial load shows a skeleton, then the list"),
  { tag: specTags(["ISS-004"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    // Open unstalled first: the shared openAuthenticated boot check reloads
    // a textless page, which would abort the stalled request below. The
    // asserted initial load is the reload under the stall.
    await driver.openAuthenticated(projectIssuesPath(seed), sessionBrowserCookies(session));
    await driver.layoutsStallIssuesGet(15_000);
    try {
      await driver.layoutsReloadIssues();
      await expect.poll(async () => driver.layoutsSkeletonVisible(), { timeout: 300_000 }).toEqual(true);
    } finally {
      await driver.layoutsReleaseStalls();
    }
    await expect
      .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
      .toEqual(expect.arrayContaining([...seed.issueNames]));
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

test(
  specTitle(["ISS-004"], "zero issues route to the empty state; calendar renders its own grid"),
  { tag: specTags(["ISS-004"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProject(
      seed.workspaceSlug,
      session,
      `Parity Zero ${uniqueSuffix()}`,
      `Z${Date.now().toString().slice(-4)}`
    );
    const freshPath = `/${seed.workspaceSlug}/projects/${projectId}/issues`;
    try {
      await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, session, {
        display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
      });
      await driver.openAuthenticated(freshPath, sessionBrowserCookies(session));
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).not.toBeNull();
      expect(await driver.layoutsListVisible()).toEqual(false);

      await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, session, {
        display_filters: { layout: "calendar", group_by: null, order_by: "sort_order" },
      });
      await driver.openAuthenticated(freshPath, sessionBrowserCookies(session));
      await expect.poll(async () => driver.layoutsCalendarVisible(), { timeout: 300_000 }).toEqual(true);
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 60_000 }).toBeNull();
    } finally {
      await serverDeleteProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-005"], "refetching the list shows the floating spinner"),
  { tag: specTags(["ISS-005"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    // A fresh project: the filter state below stays out of the seed
    // project's preferences entirely.
    const suffix = uniqueSuffix();
    const projectId = await serverCreateProject(
      seed.workspaceSlug,
      session,
      `Parity Spin ${suffix}`,
      `S${Date.now().toString().slice(-4)}`
    );
    const keeper = await serverCreateIssue(seed.workspaceSlug, projectId, session, `Parity keeper ${suffix}`);
    const filterName = `Parity EFilt ${suffix}`;
    const filterStateId = await serverCreateState(seed.workspaceSlug, projectId, filterName, "unstarted", session);
    const freshPath = `/${seed.workspaceSlug}/projects/${projectId}/issues`;
    try {
      // The spinner floats while the list loader reads "mutation", which
      // a filter refetch sets — single-issue edits never raise it. Seed
      // one match-nothing condition, then add the second through the
      // filter row while the refetch GET pends.
      await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, session, {
        display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
        rich_filters: { and: [{ priority__in: "urgent" }] },
      });
      await driver.openAuthenticated(freshPath, sessionBrowserCookies(session));
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).not.toBeNull();

      await driver.layoutsStallIssuesGet(20_000);
      try {
        const settled = driver.layoutsFilterAddConditionViaRow("State", filterName);
        // 5s cadence: the read takes a dozen bounding boxes, and
        // polling it hot wedges the loaded renderer (the loader then
        // never clears); the 20s stall window survives 5s sampling.
        await expect
          .poll(async () => driver.layoutsMutationSpinnerVisible(), { timeout: 300_000, intervals: [5_000] })
          .toEqual(true);
        await settled;
      } finally {
        await driver.layoutsReleaseStalls();
      }
      await expect
        .poll(async () => driver.layoutsMutationSpinnerVisible(), { timeout: 60_000, intervals: [5_000] })
        .toEqual(false);
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toEqual("No matching results.");
      // Server state: both conditions persisted (the UI writes rich).
      await expect
        .poll(
          async () =>
            (
              (await serverProjectUserProperties(seed.workspaceSlug, projectId, session)).richFilters as
                | { and?: unknown[] }
                | null
                | undefined
            )?.and?.length ?? 0,
          { timeout: 60_000 }
        )
        .toEqual(2);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, projectId, keeper, session).catch(() => {});
      await serverDeleteState(seed.workspaceSlug, projectId, filterStateId, session).catch(() => {});
      await serverDeleteProject(seed.workspaceSlug, projectId, session).catch(() => {});
    }
  }
);

test(
  specTitle(["ISS-005"], "group pagination shows skeleton rows while loading"),
  { tag: specTags(["ISS-005"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const prefix = `Parity skel ${uniqueSuffix()}`;
    const createdIds: string[] = [];
    for (let batch = 0; batch < 52; batch += 12) {
      const made = await Promise.all(
        Array.from({ length: Math.min(12, 52 - batch) }, (_, k) =>
          serverCreateIssue(seed.workspaceSlug, seed.projectId, session, `${prefix} ${batch + k}`)
        )
      );
      createdIds.push(...made);
    }
    try {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: { layout: "list", group_by: "state", order_by: "sort_order", show_empty_groups: false },
      });
      await driver.openAuthenticated(projectIssuesPath(seed), sessionBrowserCookies(session));
      await expect.poll(async () => driver.layoutsListGroupHasLoadMore("Todo"), { timeout: 300_000 }).toEqual(true);

      await driver.layoutsStallIssuesGet(15_000);
      try {
        const settled = driver.layoutsListGroupLoadMore("Todo");
        await expect.poll(async () => driver.layoutsSkeletonVisible(), { timeout: 300_000 }).toEqual(true);
        await settled;
      } finally {
        await driver.layoutsReleaseStalls();
      }
    } finally {
      for (let batch = 0; batch < createdIds.length; batch += 12) {
        await Promise.all(
          createdIds
            .slice(batch, batch + 12)
            .map((id) => serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session))
        );
      }
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-006"], "bug:NEWFRONT-170 quick-add reconciles with no temp row while pending"),
  { tag: specTags(["ISS-006"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await driver.openAuthenticated(projectIssuesPath(seed), sessionBrowserCookies(session));
    await expect
      .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
      .toEqual(expect.arrayContaining([...seed.issueNames]));

    // Intended: a pulsing temp row from submit until the server
    // confirms. Actual: no temp row renders at any point; the row
    // appears only at confirm (bug:NEWFRONT-170).
    const title = `Parity optimistic ${uniqueSuffix()}`;
    await driver.layoutsStallIssueMutation(20_000);
    try {
      const settled = driver.layoutsListQuickAdd(title, "All work items");
      // The stall holds the POST a full 20s from fire; the count proves
      // the request fired and pended, so the temp watch below covers a
      // real pend window however late the request fires.
      await expect.poll(async () => driver.layoutsStalledMutationCount(), { timeout: 300_000 }).toBeGreaterThan(0);
      let sawTemp = false;
      const watchUntil = Date.now() + 25_000;
      while (Date.now() < watchUntil) {
        if (await driver.layoutsTempRowVisible()) {
          sawTemp = true;
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
      expect(sawTemp).toEqual(false);
      await driver.layoutsReleaseStalls();
      await settled;
      // Reconcile half passes: the row lands with its real server id.
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
        .toContain(title);
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const createdId = rows.find((row) => row.name === title)?.id ?? "";
      expect(createdId).not.toEqual("");
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, createdId, session);
    } finally {
      await driver.layoutsReleaseStalls().catch(() => {});
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session).catch(() => []);
      const stray = rows.find((row) => row.name === title);
      if (stray) await serverDeleteIssue(seed.workspaceSlug, seed.projectId, stray.id, session).catch(() => {});
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-006"], "bug:NEWFRONT-171,bug:NEWFRONT-168 fresh issues neither bypass nor highlight"),
  { tag: specTags(["ISS-006"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    // A full page of older issues: virtualization drops the bottom rows
    // from the DOM. The fresh issue is created through the UI after
    // load, through the group-header modal, which never scrolls.
    const prefix = `Parity stale ${uniqueSuffix()}`;
    const createdIds: string[] = [];
    // Sequential: the stale issues must sort in creation order.
    for (let batch = 0; batch < 95; batch++) {
      createdIds.push(await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, `${prefix} ${batch}`));
    }
    const freshName = `Parity fresh ${uniqueSuffix()}`;
    try {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
      await driver.openAuthenticated(projectIssuesPath(seed), sessionBrowserCookies(session));
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([...seed.issueNames]));
      expect(await driver.layoutsListGroupIssueNames("All work items")).not.toContain(`${prefix} 94`);

      await driver.layoutsGroupHeaderAddChoose("All work items", null);
      await driver.layoutsWorkItemModalSetTitle(freshName);
      await driver.layoutsWorkItemModalSubmit();
      // Intended: the fresh row renders immediately via the recency
      // bypass. Actual: the intersection observer clears the bypass on
      // the first frame, so the row never renders without scrolling
      // (bug:NEWFRONT-171). A live bypass renders within seconds — the
      // same flow lands in 5s on a small list — so a 45s watch that
      // never sees the row pins the absence.
      let sawFresh = false;
      const watchUntil = Date.now() + 45_000;
      while (Date.now() < watchUntil) {
        const rows = await driver.layoutsListGroupIssueNames("All work items");
        if (rows.includes(freshName)) {
          sawFresh = true;
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 3000));
      }
      expect(sawFresh).toEqual(false);
      // Intended: the fresh row also carries the transient highlight.
      // Actual: only drops highlight; creates never do (bug:NEWFRONT-168).
      expect(await driver.layoutsRowHighlighted(freshName)).toEqual(false);
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
      const fresh = rows.find((row) => row.name === freshName);
      expect(fresh?.id ?? "").not.toEqual("");
      if (fresh) createdIds.push(fresh.id);
    } finally {
      for (let batch = 0; batch < createdIds.length; batch += 12) {
        await Promise.all(
          createdIds
            .slice(batch, batch + 12)
            .map((id) => serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {}))
        );
      }
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

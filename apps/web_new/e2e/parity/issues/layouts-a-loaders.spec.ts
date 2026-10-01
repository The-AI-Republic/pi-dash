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
  serverDeleteIssue,
  serverDeleteProject,
  serverIssues,
  serverPatchProjectUserProperties,
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
    await driver.layoutsStallIssuesGet(15_000);
    try {
      await driver.openAuthenticated(projectIssuesPath(seed), sessionBrowserCookies(session));
      await expect.poll(async () => driver.layoutsSkeletonVisible(), { timeout: 120_000 }).toEqual(true);
    } finally {
      await driver.layoutsReleaseStalls();
    }
    await expect
      .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 120_000 })
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
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).not.toBeNull();
      expect(await driver.layoutsListVisible()).toEqual(false);

      await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, session, {
        display_filters: { layout: "calendar", group_by: null, order_by: "sort_order" },
      });
      await driver.openAuthenticated(freshPath, sessionBrowserCookies(session));
      expect(await driver.layoutsCalendarVisible()).toEqual(true);
      expect(await driver.layoutsEmptyTitle()).toBeNull();
    } finally {
      await serverDeleteProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-005"], "mutations show the floating spinner"),
  { tag: specTags(["ISS-005"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await driver.openAuthenticated(projectIssuesPath(seed), sessionBrowserCookies(session));
    const first = seed.issueNames[0] ?? "";
    await expect
      .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 120_000 })
      .toContain(first);

    await driver.layoutsStallIssueMutation(15_000);
    try {
      const settled = driver.layoutsRowSetPriority(first, "High");
      await expect.poll(async () => driver.layoutsMutationSpinnerVisible(), { timeout: 120_000 }).toEqual(true);
      await settled;
    } finally {
      await driver.layoutsReleaseStalls();
    }
    expect(await driver.layoutsMutationSpinnerVisible()).toEqual(false);
    await driver.layoutsRowSetPriority(first, "None");
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
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
      await expect.poll(async () => driver.layoutsListGroupHasLoadMore("Todo"), { timeout: 120_000 }).toEqual(true);

      await driver.layoutsStallIssuesGet(15_000);
      try {
        const settled = driver.layoutsListGroupLoadMore("Todo");
        await expect.poll(async () => driver.layoutsSkeletonVisible(), { timeout: 120_000 }).toEqual(true);
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
  specTitle(["ISS-006"], "quick-add shows a pulsing temp row until the server confirms"),
  { tag: specTags(["ISS-006"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await driver.openAuthenticated(projectIssuesPath(seed), sessionBrowserCookies(session));
    await expect
      .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 120_000 })
      .toEqual(expect.arrayContaining([...seed.issueNames]));

    const title = `Parity optimistic ${uniqueSuffix()}`;
    await driver.layoutsStallIssueMutation(15_000);
    let createdId = "";
    try {
      const settled = driver.layoutsListQuickAdd(title, "All work items");
      await expect.poll(async () => driver.layoutsTempRowVisible(), { timeout: 120_000 }).toEqual(true);
      await settled;
    } finally {
      await driver.layoutsReleaseStalls();
    }
    expect(await driver.layoutsTempRowVisible()).toEqual(false);
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    createdId = rows.find((row) => row.name === title)?.id ?? "";
    expect(createdId).not.toEqual("");
    await serverDeleteIssue(seed.workspaceSlug, seed.projectId, createdId, session);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

test(
  specTitle(["ISS-006"], "fresh issues render immediately and carry the highlight"),
  { tag: specTags(["ISS-006"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    // A full page of older issues: virtualization drops the bottom rows
    // from the DOM, while a fresh issue bypasses it.
    const prefix = `Parity stale ${uniqueSuffix()}`;
    const createdIds: string[] = [];
    for (let batch = 0; batch < 95; batch += 12) {
      const made = await Promise.all(
        Array.from({ length: Math.min(12, 95 - batch) }, (_, k) =>
          serverCreateIssue(seed.workspaceSlug, seed.projectId, session, `${prefix} ${batch + k}`)
        )
      );
      createdIds.push(...made);
    }
    const freshName = `Parity fresh ${uniqueSuffix()}`;
    const freshId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, freshName);
    createdIds.push(freshId);
    try {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
      await driver.openAuthenticated(projectIssuesPath(seed), sessionBrowserCookies(session));
      const rendered = await driver.layoutsListGroupIssueNames("All work items");
      expect(rendered.length).toBeGreaterThan(0);
      // The oldest bottom rows stay virtualized out of the DOM...
      expect(rendered).not.toContain(`${prefix} 94`);
      // ...while the fresh issue renders immediately with its highlight.
      expect(rendered).toContain(freshName);
      expect(await driver.layoutsRowHighlighted(freshName)).toEqual(true);
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

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-117): per-row quick actions — project menu
// (ISS-060), archive gating (ISS-061), cycle/module contexts (ISS-062/063),
// archived rows (ISS-064), all-issues rows (ISS-065), detail/peek headers
// (ISS-066), the whole-list ellipsis (ISS-067), and group-header
// create/add-existing (ISS-012).
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  seedProjectUserProperties,
  serverAddIssuesToCycle,
  serverAddIssuesToModule,
  serverArchivedIssues,
  serverCreateCycle,
  serverCreateIssue,
  serverCreateModule,
  serverCreateProject,
  serverCreateState,
  serverCycleIssueIds,
  serverDeleteCycle,
  serverDeleteIssue,
  serverDeleteModule,
  serverDeleteProject,
  serverDeleteState,
  serverIssues,
  serverListStates,
  serverModuleIssueIds,
  serverPatchIssue,
  serverPatchProject,
  serverPatchProjectUserProperties,
  serverProjectDetails,
  serverRestoreIssue,
  sessionBrowserCookies,
  signInSession,
  uniqueSuffix,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.use({ viewport: { width: 1600, height: 900 } });

const PROJECT_MENU = ["Edit", "Make a copy", "Open in new tab", "Copy link", "Move to project", "Archive", "Delete"];

async function resetPrefs(workspaceSlug: string, projectId: string, session: string): Promise<void> {
  const seed = seedProjectUserProperties();
  await serverPatchProjectUserProperties(workspaceSlug, projectId, session, {
    display_filters: seed.display_filters,
    display_properties: seed.display_properties,
  });
}

async function openPath(
  driver: Pick<ParityDriver, "openAuthenticated">,
  path: string,
  sessionCookie: string
): Promise<void> {
  await driver.openAuthenticated(path, sessionBrowserCookies(sessionCookie));
}

function projectIssuesPath(seed: Pick<ParitySeedFacts, "workspaceSlug" | "projectId">): string {
  return `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;
}

async function completedStateId(
  seed: Pick<ParitySeedFacts, "workspaceSlug" | "projectId">,
  session: string
): Promise<{ id: string; created: boolean }> {
  const states = await serverListStates(seed.workspaceSlug, seed.projectId, session);
  const existing = states.find((row) => row.group === "completed");
  if (existing) return { id: existing.id, created: false };
  const name = `Parity Done ${uniqueSuffix()}`;
  const id = await serverCreateState(seed.workspaceSlug, seed.projectId, name, "completed", session);
  return { id, created: true };
}

test(
  specTitle(["ISS-060"], "project row menu matches the context menu"),
  { tag: specTags(["ISS-060"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
    const first = seed.issueNames[0] ?? "";
    expect(await driver.layoutsRowMenuItems(first)).toEqual(PROJECT_MENU);
    expect(await driver.layoutsRowContextMenuItems(first)).toEqual(PROJECT_MENU);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

test(
  specTitle(["ISS-060"], "guests see only open and copy-link"),
  { tag: specTags(["ISS-060"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    if (!seed.guestEmail || !seed.guestPassword) {
      throw new Error("[parity] seed facts carry no guest; re-run the stack seed step (see stack/README.md).");
    }
    // Guests see only issues they created (the server drops guest
    // assignees silently and guests cannot create), so the owner flips
    // guest_view_all_features for this test and restores it after.
    const ownerSession = await signInSession(seed.email, seed.password);
    const first = seed.issueNames[0] ?? "";
    await serverPatchProject(seed.workspaceSlug, seed.projectId, ownerSession, { guest_view_all_features: true });
    const guestSession = await signInSession(seed.guestEmail, seed.guestPassword);
    try {
      await resetPrefs(seed.workspaceSlug, seed.projectId, ownerSession);
      await openPath(driver, projectIssuesPath(seed), guestSession);
      expect(await driver.layoutsRowMenuItems(first)).toEqual(["Open in new tab", "Copy link"]);
    } finally {
      await serverPatchProject(seed.workspaceSlug, seed.projectId, ownerSession, { guest_view_all_features: false });
    }
  }
);

test(
  specTitle(["ISS-060"], "edit and duplicate open the work-item modal"),
  { tag: specTags(["ISS-060"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
    const first = seed.issueNames[0] ?? "";

    await driver.layoutsRowMenuChoose(first, "Edit");
    await expect.poll(async () => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toEqual(true);
    expect(await driver.layoutsWorkItemModalTitle()).toEqual("Update");
    expect(await driver.layoutsWorkItemModalHasText(first)).toEqual(true);
    await driver.layoutsWorkItemModalClose();
    expect(await driver.layoutsWorkItemModalVisible()).toEqual(false);

    // OSS duplication goes through the create modal (native duplication is
    // cloud-only); nothing is created until the modal submits.
    await driver.layoutsRowMenuChoose(first, "Make a copy");
    await expect.poll(async () => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toEqual(true);
    expect(await driver.layoutsWorkItemModalTitle()).toEqual("Create new work item");
    await driver.layoutsWorkItemModalClose();
    const names = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    expect(names.some((row) => row.name.includes("(copy)"))).toEqual(false);

    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

test(
  specTitle(["ISS-060"], "copy-link toasts and open-in-new-tab deep-links"),
  { tag: specTags(["ISS-060"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
    const first = seed.issueNames[0] ?? "";
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, session);
    const firstRow = rows.find((row) => row.name === first);
    const details = await serverProjectDetails(seed.workspaceSlug, seed.projectId, session);
    const deep = `/browse/${details.identifier}-${firstRow?.sequenceId}/`;

    await driver.layoutsRowMenuChoose(first, "Copy link");
    expect(await driver.rulesReadClipboard()).toContain(deep);
    const toast = await driver.rulesLastToast();
    expect(toast?.title).toEqual("Link copied");

    const url = await driver.layoutsRowMenuOpenNewTabUrl(first);
    expect(url).toContain(deep);

    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

test(
  specTitle(["ISS-060"], "move an issue to another project"),
  { tag: specTags(["ISS-060"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const suffix = uniqueSuffix();
    const projectName = `Parity Move ${suffix}`;
    const identifier = `Q${Date.now().toString().slice(-4)}`;
    const projectB = await serverCreateProject(seed.workspaceSlug, session, projectName, identifier);
    const title = `Parity mover ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    try {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
      await openPath(driver, projectIssuesPath(seed), session);

      await driver.layoutsRowMenuChoose(title, "Move to project");
      await expect.poll(async () => driver.layoutsMoveModalVisible(), { timeout: 60_000 }).toEqual(true);
      await driver.layoutsMoveModalChoose(projectName);
      expect(await driver.layoutsMoveModalVisible()).toEqual(false);

      await expect
        .poll(async () => serverIssues(seed.workspaceSlug, projectB, session), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ id })]));

      // The move navigates to the moved issue, so come back to the
      // source list before asserting the row is gone there.
      await openPath(driver, projectIssuesPath(seed), session);
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
        .not.toContain(title);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, projectB, id, session).catch(() =>
        serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {})
      );
      await serverDeleteProject(seed.workspaceSlug, projectB, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-060"], "delete an issue from the row menu"),
  { tag: specTags(["ISS-060"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const title = `Parity doomed ${uniqueSuffix()}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    try {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
      await openPath(driver, projectIssuesPath(seed), session);

      await driver.layoutsRowMenuChoose(title, "Delete");
      await expect.poll(async () => driver.layoutsDeleteModalVisible(), { timeout: 60_000 }).toEqual(true);
      await driver.layoutsDeleteModalConfirm();
      expect(await driver.layoutsDeleteModalVisible()).toEqual(false);

      await expect
        .poll(async () => serverIssues(seed.workspaceSlug, seed.projectId, session), { timeout: 300_000 })
        .not.toEqual(expect.arrayContaining([expect.objectContaining({ id })]));
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
        .not.toContain(title);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-060", "ISS-061"], "archive a completed issue from the row menu"),
  { tag: specTags(["ISS-060", "ISS-061"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const done = await completedStateId(seed, session);
    const title = `Parity arch ${uniqueSuffix()}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { state_id: done.id }, session);
    try {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
      await openPath(driver, projectIssuesPath(seed), session);

      expect(await driver.layoutsRowMenuItemDisabled(title, "Archive")).toEqual(false);
      expect(await driver.layoutsRowMenuItemNote(title, "Archive")).toBeNull();

      await driver.layoutsRowMenuChoose(title, "Archive");
      await expect.poll(async () => driver.layoutsArchiveModalVisible(), { timeout: 60_000 }).toEqual(true);
      await driver.layoutsArchiveModalConfirm();
      expect(await driver.layoutsArchiveModalVisible()).toEqual(false);

      await expect
        .poll(async () => serverArchivedIssues(seed.workspaceSlug, seed.projectId, session), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ id })]));
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
        .not.toContain(title);

      await serverRestoreIssue(seed.workspaceSlug, seed.projectId, id, session);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      if (done.created) await serverDeleteState(seed.workspaceSlug, seed.projectId, done.id, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-061"], "archive stays disabled outside completed/cancelled states"),
  { tag: specTags(["ISS-061"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
    const first = seed.issueNames[0] ?? "";
    expect(await driver.layoutsRowMenuItems(first)).toContain("Archive");
    expect(await driver.layoutsRowMenuItemDisabled(first, "Archive")).toEqual(true);
    const note = await driver.layoutsRowMenuItemNote(first, "Archive");
    expect(note ?? "").toContain("Only completed or canceled");
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

test(
  specTitle(["ISS-062"], "cycle rows remove from cycle; edit prefills the cycle"),
  { tag: specTags(["ISS-062"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const suffix = uniqueSuffix();
    const cycleName = `Parity Cycle ${suffix}`;
    const now = new Date();
    const iso = (d: Date): string => d.toISOString().slice(0, 10);
    const start = new Date(now);
    start.setUTCDate(start.getUTCDate() - 3);
    const end = new Date(now);
    end.setUTCDate(end.getUTCDate() + 11);
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      cycleName,
      iso(start),
      iso(end),
      session
    );
    const title = `Parity cycrow ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverAddIssuesToCycle(seed.workspaceSlug, seed.projectId, cycleId, [id], session);
    const cyclePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycleId}`;
    try {
      await openPath(driver, cyclePath, session);
      expect(await driver.layoutsRowMenuItems(title)).toEqual([
        "Edit",
        "Make a copy",
        "Open in new tab",
        "Copy link",
        "Remove from cycle",
        "Archive",
        "Delete",
      ]);

      // The modal renders no cycle field, so the prefill is proven
      // through submit: renaming keeps the issue in its cycle.
      await driver.layoutsRowMenuChoose(title, "Edit");
      await expect.poll(async () => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toEqual(true);
      const renamed = `${title} renamed`;
      await driver.layoutsWorkItemModalSetTitle(renamed);
      await driver.layoutsWorkItemModalSubmit();
      await expect
        .poll(async () => serverIssues(seed.workspaceSlug, seed.projectId, session), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ id, name: renamed })]));
      await expect
        .poll(async () => serverCycleIssueIds(seed.workspaceSlug, seed.projectId, cycleId, session), {
          timeout: 300_000,
        })
        .toContain(id);

      await driver.layoutsRowMenuChoose(renamed, "Remove from cycle");
      await expect
        .poll(async () => serverCycleIssueIds(seed.workspaceSlug, seed.projectId, cycleId, session), {
          timeout: 300_000,
        })
        .not.toContain(id);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, session);
    }
  }
);

test(
  specTitle(["ISS-063"], "module rows remove from module; edit prefills the module"),
  { tag: specTags(["ISS-063"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const suffix = uniqueSuffix();
    const moduleName = `Parity Module ${suffix}`;
    const moduleId = await serverCreateModule(seed.workspaceSlug, seed.projectId, moduleName, session);
    const title = `Parity modrow ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverAddIssuesToModule(seed.workspaceSlug, seed.projectId, moduleId, [id], session);
    const modulePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${moduleId}`;
    try {
      await openPath(driver, modulePath, session);
      expect(await driver.layoutsRowMenuItems(title)).toEqual([
        "Edit",
        "Make a copy",
        "Open in new tab",
        "Copy link",
        "Remove from module",
        "Archive",
        "Delete",
      ]);

      // The modal renders no module field, so the prefill is proven
      // through submit: renaming keeps the issue in its module.
      await driver.layoutsRowMenuChoose(title, "Edit");
      await expect.poll(async () => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toEqual(true);
      const renamed = `${title} renamed`;
      await driver.layoutsWorkItemModalSetTitle(renamed);
      await driver.layoutsWorkItemModalSubmit();
      await expect
        .poll(async () => serverIssues(seed.workspaceSlug, seed.projectId, session), { timeout: 300_000 })
        .toEqual(expect.arrayContaining([expect.objectContaining({ id, name: renamed })]));
      await expect
        .poll(async () => serverModuleIssueIds(seed.workspaceSlug, seed.projectId, moduleId, session), {
          timeout: 300_000,
        })
        .toContain(id);

      await driver.layoutsRowMenuChoose(renamed, "Remove from module");
      await expect
        .poll(async () => serverModuleIssueIds(seed.workspaceSlug, seed.projectId, moduleId, session), {
          timeout: 300_000,
        })
        .not.toContain(id);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteModule(seed.workspaceSlug, seed.projectId, moduleId, session);
    }
  }
);

test(
  specTitle(["ISS-064"], "bug:NEWFRONT-165 archived rows restore or delete only"),
  { tag: specTags(["ISS-064"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const done = await completedStateId(seed, session);
    const title = `Parity restore ${uniqueSuffix()}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { state_id: done.id }, session);
    // Archive through the UI so the archived list below holds a real row.
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
    await driver.layoutsRowMenuChoose(title, "Archive");
    await driver.layoutsArchiveModalConfirm();
    await expect
      .poll(async () => serverArchivedIssues(seed.workspaceSlug, seed.projectId, session), { timeout: 300_000 })
      .toEqual(expect.arrayContaining([expect.objectContaining({ id })]));
    const archivedPath = `/${seed.workspaceSlug}/projects/${seed.projectId}/archives/issues`;
    try {
      await openPath(driver, archivedPath, session);
      // bug:NEWFRONT-165 — the menu omits Delete (readOnly wiring hides
      // it); ISS-064 intends Restore, Open in new tab, Copy link, Delete.
      expect(await driver.layoutsRowMenuItems(title)).toEqual(["Restore", "Open in new tab", "Copy link"]);

      await driver.layoutsRowMenuChoose(title, "Restore");
      // The toast fires after the restore API resolves, so poll for
      // the title instead of reading the pre-toast gap.
      await expect
        .poll(async () => (await driver.rulesLastToast())?.title ?? "", { timeout: 60_000 })
        .toContain("Restore");
      await expect
        .poll(async () => serverArchivedIssues(seed.workspaceSlug, seed.projectId, session), { timeout: 300_000 })
        .not.toEqual(expect.arrayContaining([expect.objectContaining({ id })]));

      // Reload before the presence read: the restore landed from the
      // archived page, and the project list can serve its cached rows
      // (from before the restore) instead of refetching on navigation.
      await openPath(driver, projectIssuesPath(seed), session);
      await driver.layoutsReloadIssues();
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
        .toContain(title);
    } finally {
      await serverRestoreIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      if (done.created) await serverDeleteState(seed.workspaceSlug, seed.projectId, done.id, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-065"], "all-issues rows offer the full project menu"),
  { tag: specTags(["ISS-065"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await openPath(driver, `/${seed.workspaceSlug}/workspace-views/all-issues`, session);
    const first = seed.issueNames[0] ?? "";
    expect(await driver.layoutsRowMenuItems(first)).toEqual(PROJECT_MENU);
  }
);

test(
  specTitle(["ISS-066"], "detail header menu and peek header menu"),
  { tag: specTags(["ISS-066"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
    // The second seed issue: the first is intake-pending, and pending
    // triage replaces the detail header ellipsis with Accept/Decline,
    // so the standard header menu is pinned on a triage-free issue.
    const first = seed.issueNames[1] ?? "";
    const href = await driver.layoutsRowHref(first);
    expect(href).toContain("/browse/");

    await openPath(driver, href ?? "", session);
    expect(await driver.layoutsDetailMenuItems()).toEqual([
      "Make a copy",
      "Open in new tab",
      "Move to project",
      "Archive",
      "Delete",
    ]);
    expect(await driver.layoutsPeekCopyLinkVisible()).toEqual(false);

    await openPath(driver, projectIssuesPath(seed), session);
    await driver.layoutsRowOpenPeek(first);
    expect(await driver.layoutsPeekVisible()).toEqual(true);
    expect(await driver.layoutsDetailMenuItems()).toEqual([
      "Make a copy",
      "Open in new tab",
      "Move to project",
      "Archive",
      "Delete",
    ]);
    expect(await driver.layoutsPeekCopyLinkVisible()).toEqual(true);
    await driver.layoutsPeekClose();

    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

test(
  specTitle(["ISS-067"], "whole-list ellipsis copies the list link"),
  { tag: specTags(["ISS-067"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    // The whole-list ellipsis lives on view pages (the project header
    // carries no list-level menu); all-issues offers Open/Copy.
    const session = await signInSession(seed.email, seed.password);
    await openPath(driver, `/${seed.workspaceSlug}/workspace-views/all-issues`, session);
    expect(await driver.layoutsListPageMenuItems()).toEqual(["Open in new tab", "Copy link"]);

    await driver.layoutsListPageMenuChoose("Copy link");
    const url = await driver.layoutsCurrentUrl();
    expect(await driver.rulesReadClipboard()).toContain(url.replace(/^https?:\/\/[^/]+/, ""));
    const toast = await driver.rulesLastToast();
    expect(toast?.message).toContain("link copied to clipboard");
  }
);

test(
  specTitle(["ISS-012"], "cycle group-header creates or attaches with the section value"),
  { tag: specTags(["ISS-012"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const suffix = uniqueSuffix();
    const cycleName = `Parity GHC ${suffix}`;
    const now = new Date();
    const iso = (d: Date): string => d.toISOString().slice(0, 10);
    const start = new Date(now);
    start.setUTCDate(start.getUTCDate() - 3);
    const end = new Date(now);
    end.setUTCDate(end.getUTCDate() + 11);
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      cycleName,
      iso(start),
      iso(end),
      session
    );
    const title = `Parity ghseed ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverAddIssuesToCycle(seed.workspaceSlug, seed.projectId, cycleId, [id], session);
    const plainName = `Parity ghplain ${suffix}`;
    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, plainName);
    const cyclePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycleId}`;
    let createdId = "";
    try {
      await openPath(driver, cyclePath, session);
      // The group read is immediate; the list body renders after the
      // page chrome, so poll for the first group like the list specs.
      await expect.poll(async () => driver.layoutsListGroups(), { timeout: 300_000 }).not.toHaveLength(0);
      const groups = await driver.layoutsListGroups();
      expect(groups.length).toBeGreaterThan(0);
      const group = groups[0] ?? "";
      expect(await driver.layoutsGroupHeaderAddMenu(group)).toEqual(["Create work item", "Add an existing work item"]);

      // The modal renders no cycle field, so the section value is
      // proven through submit: the created issue lands in the cycle.
      await driver.layoutsGroupHeaderAddChoose(group, "Create work item");
      expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
      const createdName = `Parity ghcreated ${suffix}`;
      await driver.layoutsWorkItemModalSetTitle(createdName);
      await driver.layoutsWorkItemModalSubmit();
      await expect
        .poll(
          async () =>
            (await serverIssues(seed.workspaceSlug, seed.projectId, session)).some((row) => row.name === createdName),
          {
            timeout: 300_000,
          }
        )
        .toEqual(true);
      createdId =
        (await serverIssues(seed.workspaceSlug, seed.projectId, session)).find((row) => row.name === createdName)?.id ??
        "";
      await expect
        .poll(async () => serverCycleIssueIds(seed.workspaceSlug, seed.projectId, cycleId, session), {
          timeout: 300_000,
        })
        .toContain(createdId);

      await driver.layoutsGroupHeaderAddChoose(group, "Add an existing work item");
      expect(await driver.layoutsAddExistingModalVisible()).toEqual(true);
      await driver.layoutsAddExistingModalChoose(plainName);
      await expect
        .poll(async () => serverCycleIssueIds(seed.workspaceSlug, seed.projectId, cycleId, session), {
          timeout: 300_000,
        })
        .toContain(plainId);
    } finally {
      if (createdId !== "")
        await serverDeleteIssue(seed.workspaceSlug, seed.projectId, createdId, session).catch(() => {});
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, plainId, session).catch(() => {});
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, session);
    }
  }
);

test(
  specTitle(["ISS-012"], "module group-header mirrors cycle; plain project opens the modal directly"),
  { tag: specTags(["ISS-012"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const suffix = uniqueSuffix();
    const moduleName = `Parity GHM ${suffix}`;
    const moduleId = await serverCreateModule(seed.workspaceSlug, seed.projectId, moduleName, session);
    const title = `Parity ghmseed ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverAddIssuesToModule(seed.workspaceSlug, seed.projectId, moduleId, [id], session);
    const modulePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${moduleId}`;
    let createdId = "";
    try {
      await openPath(driver, modulePath, session);
      await expect.poll(async () => driver.layoutsListGroups(), { timeout: 300_000 }).not.toHaveLength(0);
      const groups = await driver.layoutsListGroups();
      const group = groups[0] ?? "";
      expect(await driver.layoutsGroupHeaderAddMenu(group)).toEqual(["Create work item", "Add an existing work item"]);
      // The modal renders no module field, so the section value is
      // proven through submit: the created issue lands in the module.
      await driver.layoutsGroupHeaderAddChoose(group, "Create work item");
      expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
      const createdName = `Parity ghmcreated ${suffix}`;
      await driver.layoutsWorkItemModalSetTitle(createdName);
      await driver.layoutsWorkItemModalSubmit();
      await expect
        .poll(
          async () =>
            (await serverIssues(seed.workspaceSlug, seed.projectId, session)).some((row) => row.name === createdName),
          {
            timeout: 300_000,
          }
        )
        .toEqual(true);
      createdId =
        (await serverIssues(seed.workspaceSlug, seed.projectId, session)).find((row) => row.name === createdName)?.id ??
        "";
      await expect
        .poll(async () => serverModuleIssueIds(seed.workspaceSlug, seed.projectId, moduleId, session), {
          timeout: 300_000,
        })
        .toContain(createdId);
    } finally {
      if (createdId !== "")
        await serverDeleteIssue(seed.workspaceSlug, seed.projectId, createdId, session).catch(() => {});
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteModule(seed.workspaceSlug, seed.projectId, moduleId, session);
    }

    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
    await expect.poll(async () => driver.layoutsListGroups(), { timeout: 300_000 }).not.toHaveLength(0);
    const groups = await driver.layoutsListGroups();
    const group = groups[0] ?? "";
    expect(await driver.layoutsGroupHeaderAddMenu(group)).toBeNull();
    await driver.layoutsGroupHeaderAddChoose(group, null);
    expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
    expect(await driver.layoutsWorkItemModalTitle()).toEqual("Create new work item");
    await driver.layoutsWorkItemModalClose();
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  }
);

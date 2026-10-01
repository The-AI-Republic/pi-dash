// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
  serverPatchProjectUserProperties,
  serverRestoreIssue,
  serverWorkspaceUserId,
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
  const id = await serverCreateState(seed.workspaceSlug, seed.projectId, session, name, "completed");
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
    const ownerSession = await signInSession(seed.email, seed.password);
    const guestId = await serverWorkspaceUserId(seed.workspaceSlug, seed.guestEmail, ownerSession);
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, ownerSession);
    const first = seed.issueNames[0] ?? "";
    const firstId = rows.find((row) => row.name === first)?.id ?? "";
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, firstId, { assignee_ids: [guestId] }, ownerSession);
    const guestSession = await signInSession(seed.guestEmail, seed.guestPassword);
    try {
      await resetPrefs(seed.workspaceSlug, seed.projectId, ownerSession);
      await openPath(driver, projectIssuesPath(seed), guestSession);
      expect(await driver.layoutsRowMenuItems(first)).toEqual(["Open in new tab", "Copy link"]);
    } finally {
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, firstId, { assignee_ids: [] }, ownerSession);
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
    expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
    expect(await driver.layoutsWorkItemModalTitle()).toEqual("Update");
    expect(await driver.layoutsWorkItemModalHasText(first)).toEqual(true);
    await driver.layoutsWorkItemModalClose();
    expect(await driver.layoutsWorkItemModalVisible()).toEqual(false);

    // OSS duplication goes through the create modal (native duplication is
    // cloud-only); nothing is created until the modal submits.
    await driver.layoutsRowMenuChoose(first, "Make a copy");
    expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
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
    const firstId = rows.find((row) => row.name === first)?.id ?? "";

    await driver.layoutsRowMenuChoose(first, "Copy link");
    expect(await driver.rulesReadClipboard()).toContain(firstId);
    const toast = await driver.rulesLastToast();
    expect(toast?.title).toEqual("Link copied");

    const url = await driver.layoutsRowMenuOpenNewTabUrl(first);
    expect(url).toContain(firstId);

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
      expect(await driver.layoutsMoveModalVisible()).toEqual(true);
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
      expect(await driver.layoutsDeleteModalVisible()).toEqual(true);
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
      expect(await driver.layoutsArchiveModalVisible()).toEqual(true);
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
    const cycle = await serverCreateCycle(seed.workspaceSlug, seed.projectId, session, cycleName, iso(start), iso(end));
    const title = `Parity cycrow ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverAddIssuesToCycle(seed.workspaceSlug, seed.projectId, cycle.id, [id], session);
    const cyclePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}`;
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

      await driver.layoutsRowMenuChoose(title, "Edit");
      expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
      expect(await driver.layoutsWorkItemModalHasText(cycleName)).toEqual(true);
      await driver.layoutsWorkItemModalClose();

      await driver.layoutsRowMenuChoose(title, "Remove from cycle");
      await expect
        .poll(async () => serverCycleIssueIds(seed.workspaceSlug, seed.projectId, cycle.id, session), {
          timeout: 300_000,
        })
        .not.toContain(id);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, session);
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
    const module = await serverCreateModule(seed.workspaceSlug, seed.projectId, session, moduleName);
    const title = `Parity modrow ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverAddIssuesToModule(seed.workspaceSlug, seed.projectId, module.id, [id], session);
    const modulePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${module.id}`;
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

      await driver.layoutsRowMenuChoose(title, "Edit");
      expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
      expect(await driver.layoutsWorkItemModalHasText(moduleName)).toEqual(true);
      await driver.layoutsWorkItemModalClose();

      await driver.layoutsRowMenuChoose(title, "Remove from module");
      await expect
        .poll(async () => serverModuleIssueIds(seed.workspaceSlug, seed.projectId, module.id, session), {
          timeout: 300_000,
        })
        .not.toContain(id);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteModule(seed.workspaceSlug, seed.projectId, module.id, session);
    }
  }
);

test(
  specTitle(["ISS-064"], "archived rows restore or delete only"),
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
      expect(await driver.layoutsRowMenuItems(title)).toEqual(["Restore", "Open in new tab", "Copy link", "Delete"]);

      await driver.layoutsRowMenuChoose(title, "Restore");
      const toast = await driver.rulesLastToast();
      expect(toast?.title).toContain("Restore");
      await expect
        .poll(async () => serverArchivedIssues(seed.workspaceSlug, seed.projectId, session), { timeout: 300_000 })
        .not.toEqual(expect.arrayContaining([expect.objectContaining({ id })]));

      await openPath(driver, projectIssuesPath(seed), session);
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
    const first = seed.issueNames[0] ?? "";
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
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
    expect(await driver.layoutsListPageMenuItems()).toEqual(["Open in new tab", "Copy link"]);

    await driver.layoutsListPageMenuChoose("Copy link");
    const url = await driver.layoutsCurrentUrl();
    expect(await driver.rulesReadClipboard()).toContain(url.replace(/^https?:\/\/[^/]+/, ""));
    const toast = await driver.rulesLastToast();
    expect(toast?.message).toContain("link copied to clipboard");

    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
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
    const cycle = await serverCreateCycle(seed.workspaceSlug, seed.projectId, session, cycleName, iso(start), iso(end));
    const title = `Parity ghseed ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverAddIssuesToCycle(seed.workspaceSlug, seed.projectId, cycle.id, [id], session);
    const plainName = `Parity ghplain ${suffix}`;
    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, plainName);
    const cyclePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}`;
    try {
      await openPath(driver, cyclePath, session);
      const groups = await driver.layoutsListGroups();
      expect(groups.length).toBeGreaterThan(0);
      const group = groups[0] ?? "";
      expect(await driver.layoutsGroupHeaderAddMenu(group)).toEqual(["Create work item", "Add an existing work item"]);

      await driver.layoutsGroupHeaderAddChoose(group, "Create work item");
      expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
      expect(await driver.layoutsWorkItemModalHasText(cycleName)).toEqual(true);
      await driver.layoutsWorkItemModalClose();

      await driver.layoutsGroupHeaderAddChoose(group, "Add an existing work item");
      expect(await driver.layoutsAddExistingModalVisible()).toEqual(true);
      await driver.layoutsAddExistingModalChoose(plainName);
      await expect
        .poll(async () => serverCycleIssueIds(seed.workspaceSlug, seed.projectId, cycle.id, session), {
          timeout: 300_000,
        })
        .toContain(plainId);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, plainId, session).catch(() => {});
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, session);
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
    const module = await serverCreateModule(seed.workspaceSlug, seed.projectId, session, moduleName);
    const title = `Parity ghmseed ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverAddIssuesToModule(seed.workspaceSlug, seed.projectId, module.id, [id], session);
    const modulePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${module.id}`;
    try {
      await openPath(driver, modulePath, session);
      const groups = await driver.layoutsListGroups();
      const group = groups[0] ?? "";
      expect(await driver.layoutsGroupHeaderAddMenu(group)).toEqual(["Create work item", "Add an existing work item"]);
      await driver.layoutsGroupHeaderAddChoose(group, "Create work item");
      expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
      expect(await driver.layoutsWorkItemModalHasText(moduleName)).toEqual(true);
      await driver.layoutsWorkItemModalClose();
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteModule(seed.workspaceSlug, seed.projectId, module.id, session);
    }

    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await openPath(driver, projectIssuesPath(seed), session);
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

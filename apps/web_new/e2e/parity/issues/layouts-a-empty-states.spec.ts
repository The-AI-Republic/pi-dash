// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-117): empty states — project (ISS-068),
// cycle (ISS-069), module (ISS-070), archived (ISS-071), saved view
// (ISS-072), workspace all-issues (ISS-073), and profile tabs (ISS-074).
// Filtered-empty branches filter by a member who owns nothing, through the
// same user-properties endpoints the filter bar writes.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import type { FreshUser } from "../helpers/api";
import {
  browserCookies,
  createWorkspaceViaApi,
  markOnboarded,
  requireMentionMember,
  seedIssueFilters,
  seedProjectUserProperties,
  serverArchiveIssue,
  serverCreateCycle,
  serverCreateIssue,
  serverCreateModule,
  serverCreateProject,
  serverCreateState,
  serverCreateView,
  serverCycleIssueIds,
  serverDeleteCycle,
  serverDeleteIssue,
  serverDeleteModule,
  serverDeleteProject,
  serverDeleteState,
  serverDeleteView,
  serverIssues,
  serverListStates,
  serverModuleIssueIds,
  serverPatchCycleUserProperties,
  serverPatchIssue,
  serverPatchModuleUserProperties,
  serverPatchProjectUserProperties,
  serverRestoreIssue,
  sessionBrowserCookies,
  setLastWorkspace,
  signInSession,
  signUpFreshUser,
  uniqueSlug,
  uniqueSuffix,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.use({ viewport: { width: 1600, height: 900 } });

async function resetPrefs(workspaceSlug: string, projectId: string, session: string): Promise<void> {
  const seed = seedProjectUserProperties();
  await serverPatchProjectUserProperties(workspaceSlug, projectId, session, {
    filters: seedIssueFilters(),
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

/** A workspace with zero projects, owned by a fresh member (ISS-073/074). */
async function freshEmptyWorkspace(): Promise<{ slug: string; user: FreshUser }> {
  const user = await signUpFreshUser("parity-empty");
  const slug = uniqueSlug("pwempty");
  const workspace = await createWorkspaceViaApi(user, { name: "Parity Empty", slug });
  await markOnboarded(user);
  await setLastWorkspace(user, workspace.id);
  return { slug, user };
}

test(
  specTitle(["ISS-068"], "project empty states: first item vs filtered-empty"),
  { tag: specTags(["ISS-068"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const member = requireMentionMember(seed);
    // A fresh project holds zero issues, so its list renders the empty
    // state without touching the seeded project.
    const suffix = uniqueSuffix();
    const projectId = await serverCreateProject(
      seed.workspaceSlug,
      session,
      `Parity Empty ${suffix}`,
      `E${Date.now().toString().slice(-4)}`
    );
    const freshPath = `/${seed.workspaceSlug}/projects/${projectId}/issues`;
    try {
      await openPath(driver, freshPath, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 })
        .toEqual("Start with your first work item.");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Create your first work item", disabled: false }]);

      await test.step("the create action opens the work-item modal", async () => {
        await driver.layoutsEmptyChoose("Create your first work item");
        expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
        await driver.layoutsWorkItemModalClose();
      });

      await test.step("a filter that matches nothing offers clearing filters", async () => {
        const title = `Parity keeper ${suffix}`;
        const id = await serverCreateIssue(seed.workspaceSlug, projectId, session, title);
        try {
          await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, session, {
            filters: { ...seedIssueFilters(), assignees: [member.id] },
          });
          await openPath(driver, freshPath, session);
          await expect
            .poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 })
            .toEqual("No matching results.");
          expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear all filters", disabled: false }]);
          await driver.layoutsEmptyChoose("Clear all filters");
          await expect
            .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 120_000 })
            .toContain(title);
        } finally {
          await serverDeleteIssue(seed.workspaceSlug, projectId, id, session);
        }
      });
    } finally {
      await serverDeleteProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-068"], "guests see disabled empty-state actions"),
  { tag: specTags(["ISS-068"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    if (!seed.guestEmail || !seed.guestPassword) {
      throw new Error("[parity] seed facts carry no guest; re-run the stack seed step (see stack/README.md).");
    }
    // Guests see only assigned issues; with none assigned, the seeded
    // project's list is empty for them.
    const guestSession = await signInSession(seed.guestEmail, seed.guestPassword);
    await openPath(driver, projectIssuesPath(seed), guestSession);
    await expect
      .poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 })
      .toEqual("Start with your first work item.");
    expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Create your first work item", disabled: true }]);
  }
);

test(
  specTitle(["ISS-069"], "cycle empty states: fresh, add flows, filtered"),
  { tag: specTags(["ISS-069"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const member = requireMentionMember(seed);
    const suffix = uniqueSuffix();
    const now = new Date();
    const iso = (d: Date): string => d.toISOString().slice(0, 10);
    const start = new Date(now);
    start.setUTCDate(start.getUTCDate() - 3);
    const end = new Date(now);
    end.setUTCDate(end.getUTCDate() + 11);
    const cycle = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Parity ECyc ${suffix}`,
      iso(start),
      iso(end)
    );
    const cyclePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}`;
    const plainName = `Parity ecplain ${suffix}`;
    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, plainName);
    try {
      await openPath(driver, cyclePath, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 })
        .toEqual("No work items to show in this cycle");
      expect(await driver.layoutsEmptyActions()).toEqual([
        { label: "Create work item", disabled: false },
        { label: "Add existing work item", disabled: false },
      ]);

      await test.step("create opens the modal; add-existing attaches a real issue", async () => {
        await driver.layoutsEmptyChoose("Create work item");
        expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
        await driver.layoutsWorkItemModalClose();
        await driver.layoutsEmptyChoose("Add existing work item");
        expect(await driver.layoutsAddExistingModalVisible()).toEqual(true);
        await driver.layoutsAddExistingModalChoose(plainName);
        await expect
          .poll(async () => serverCycleIssueIds(seed.workspaceSlug, seed.projectId, cycle.id, session), {
            timeout: 120_000,
          })
          .toContain(plainId);
      });

      await test.step("filtered-empty offers clearing filters", async () => {
        await serverPatchCycleUserProperties(seed.workspaceSlug, seed.projectId, cycle.id, session, {
          filters: { ...seedIssueFilters(), assignees: [member.id] },
        });
        await openPath(driver, cyclePath, session);
        await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toEqual("No matching results.");
        expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear filters", disabled: false }]);
        await driver.layoutsEmptyChoose("Clear filters");
        await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toBeNull();
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, plainId, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, session);
    }
  }
);

test(
  specTitle(["ISS-069"], "completed cycles show an informational empty state"),
  { tag: specTags(["ISS-069"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const suffix = uniqueSuffix();
    // A cycle entirely in the past reads as completed.
    const now = new Date();
    const iso = (d: Date): string => d.toISOString().slice(0, 10);
    const start = new Date(now);
    start.setUTCDate(start.getUTCDate() - 20);
    const end = new Date(now);
    end.setUTCDate(end.getUTCDate() - 10);
    const cycle = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Parity ECycDone ${suffix}`,
      iso(start),
      iso(end)
    );
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycle.id}`, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 })
        .toEqual("No work items in the cycle");
      expect(await driver.layoutsEmptyActions()).toEqual([]);
    } finally {
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, session);
    }
  }
);

test(
  specTitle(["ISS-070"], "module empty states: fresh, add flows, filtered"),
  { tag: specTags(["ISS-070"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const member = requireMentionMember(seed);
    const suffix = uniqueSuffix();
    const module = await serverCreateModule(seed.workspaceSlug, seed.projectId, session, `Parity EMod ${suffix}`);
    const modulePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${module.id}`;
    const plainName = `Parity emplain ${suffix}`;
    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, plainName);
    try {
      await openPath(driver, modulePath, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 })
        .toEqual("No work items to show in this Module");
      expect(await driver.layoutsEmptyActions()).toEqual([
        { label: "Create work item", disabled: false },
        { label: "Add existing work item", disabled: false },
      ]);

      await driver.layoutsEmptyChoose("Add existing work item");
      expect(await driver.layoutsAddExistingModalVisible()).toEqual(true);
      await driver.layoutsAddExistingModalChoose(plainName);
      await expect
        .poll(async () => serverModuleIssueIds(seed.workspaceSlug, seed.projectId, module.id, session), {
          timeout: 120_000,
        })
        .toContain(plainId);

      await serverPatchModuleUserProperties(seed.workspaceSlug, seed.projectId, module.id, session, {
        filters: { ...seedIssueFilters(), assignees: [member.id] },
      });
      await openPath(driver, modulePath, session);
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toEqual("No matching results.");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear filters", disabled: false }]);
      await driver.layoutsEmptyChoose("Clear filters");
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toBeNull();
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, plainId, session).catch(() => {});
      await serverDeleteModule(seed.workspaceSlug, seed.projectId, module.id, session);
    }
  }
);

test(
  specTitle(["ISS-071"], "archived empty states link to automation, not creation"),
  { tag: specTags(["ISS-071"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const member = requireMentionMember(seed);
    const archivedPath = `/${seed.workspaceSlug}/projects/${seed.projectId}/archives/issues`;
    await openPath(driver, archivedPath, session);
    await expect
      .poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 })
      .toEqual("No archived work items yet");
    expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Set automation", disabled: false }]);

    await driver.layoutsEmptyChoose("Set automation");
    await expect.poll(async () => driver.layoutsCurrentUrl(), { timeout: 120_000 }).toContain("/settings/automation");

    // Filtered-empty needs a non-empty base: archive one issue first.
    const states = await serverListStates(seed.workspaceSlug, seed.projectId, session);
    let doneId = states.find((row) => row.group === "completed")?.id ?? "";
    let doneCreated = false;
    if (!doneId) {
      doneId = await serverCreateState(
        seed.workspaceSlug,
        seed.projectId,
        session,
        `Parity ADone ${uniqueSuffix()}`,
        "completed"
      );
      doneCreated = true;
    }
    const title = `Parity archold ${uniqueSuffix()}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { state_id: doneId }, session);
    await serverArchiveIssue(seed.workspaceSlug, seed.projectId, id, session);
    try {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        filters: { ...seedIssueFilters(), assignees: [member.id] },
      });
      await openPath(driver, archivedPath, session);
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toEqual("No matching results.");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear filters", disabled: false }]);
      await driver.layoutsEmptyChoose("Clear filters");
      // Positive read: the archived row renders again.
      expect(await driver.layoutsRowMenuItems(title)).toEqual(["Restore", "Open in new tab", "Copy link", "Delete"]);
    } finally {
      await serverRestoreIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      if (doneCreated) await serverDeleteState(seed.workspaceSlug, seed.projectId, doneId, session);
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-072"], "saved-view empty state offers a new work item"),
  { tag: specTags(["ISS-072"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const member = requireMentionMember(seed);
    // The view filters to a member who owns nothing, so it renders empty.
    const viewId = await serverCreateView(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Parity EView ${uniqueSuffix()}`,
      {},
      { assignees: [member.id] }
    );
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${seed.projectId}/views/${viewId}`, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 })
        .toEqual("View work items will appear here");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "New work item", disabled: false }]);
      await driver.layoutsEmptyChoose("New work item");
      expect(await driver.layoutsWorkItemModalVisible()).toEqual(true);
      await driver.layoutsWorkItemModalClose();
    } finally {
      await serverDeleteView(seed.workspaceSlug, seed.projectId, viewId, session);
    }
  }
);

test(
  specTitle(["ISS-073"], "workspace all-issues empty states: no project vs no views"),
  { tag: specTags(["ISS-073"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    await test.step("zero projects prompts starting the first", async () => {
      const fresh = await freshEmptyWorkspace();
      await driver.openAuthenticated(`/${fresh.slug}/workspace-views/all-issues`, browserCookies(fresh.user));
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toEqual("No project");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Start your first project", disabled: false }]);
    });

    await test.step("a member-less view prompts adding a work item", async () => {
      if (!seed.guestEmail || !seed.guestPassword) {
        throw new Error("[parity] seed facts carry no guest; re-run the stack seed step (see stack/README.md).");
      }
      // Defensive: a crashed run may have leaked a guest assignment; the
      // branch needs the guest to see zero issues.
      const ownerSession = await signInSession(seed.email, seed.password);
      const guestSession = await signInSession(seed.guestEmail, seed.guestPassword);
      const visible = await serverIssues(seed.workspaceSlug, seed.projectId, guestSession);
      for (const row of visible) {
        await serverPatchIssue(seed.workspaceSlug, seed.projectId, row.id, { assignee_ids: [] }, ownerSession);
      }
      await openPath(driver, `/${seed.workspaceSlug}/workspace-views/all-issues`, guestSession);
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toEqual("No Views yet");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Add work item", disabled: true }]);
    });
  }
);

test(
  specTitle(["ISS-074"], "profile tabs render informational empties; unknown renders nothing"),
  { tag: specTags(["ISS-074"]) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    // A fresh member owns nothing, so every profile tab renders empty.
    const fresh = await freshEmptyWorkspace();
    const base = `/${fresh.slug}/profile/${fresh.user.userId}`;
    const cases: Array<[string, string]> = [
      ["assigned", "No work items are assigned to you"],
      ["created", "No work items yet"],
      ["subscribed", "No work items yet"],
      ["activity", "No activities yet"],
    ];
    for (const [tab, title] of cases) {
      await driver.openAuthenticated(`${base}/${tab}`, browserCookies(fresh.user));
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toEqual(title);
      expect(await driver.layoutsEmptyActions()).toEqual([]);
    }
    await driver.openAuthenticated(`${base}/zzz-unknown`, browserCookies(fresh.user));
    await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toBeNull();
  }
);

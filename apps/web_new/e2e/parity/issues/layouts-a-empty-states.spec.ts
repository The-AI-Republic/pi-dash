// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-117): empty states — project (ISS-068),
// cycle (ISS-069), module (ISS-070), archived (ISS-071), saved view
// (ISS-072), workspace all-issues (ISS-073), and profile tabs (ISS-074).
// Filtered-empty branches seed a rich expression nothing matches
// (priority=urgent; no fixture carries it), through the same
// user-properties endpoints the filter bar writes — the pages build their
// filter state from rich_filters, not the legacy filters object. The seed
// alone empties the rows but leaves the fresh card, so each branch adds
// a second match-nothing condition through the filter row UI, which
// completes the filtered-empty branch. The archived page never reads
// server preferences, so its seed goes through its local store instead.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import type { FreshUser } from "../helpers/api";
import {
  browserCookies,
  createProjectView,
  createWorkspaceViaApi,
  markOnboarded,
  serverArchiveIssue,
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

// Rich-expression store form ({and:[{prop__in:value}]}); priority=urgent
// matches no seed or fixture issue, so it renders every filtered-empty.
const MATCH_NOTHING = { and: [{ priority__in: "urgent" }] };

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
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 })
        .toEqual("Start with your first work item.");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Create your first work item", disabled: false }]);

      await test.step("the create action opens the work-item modal", async () => {
        await driver.layoutsEmptyChoose("Create your first work item");
        await expect.poll(async () => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toEqual(true);
        await driver.layoutsWorkItemModalClose();
      });

      await test.step("a filter that matches nothing offers clearing filters", async () => {
        const title = `Parity keeper ${suffix}`;
        const id = await serverCreateIssue(seed.workspaceSlug, projectId, session, title);
        // The seeded condition empties the rows but leaves the fresh
        // card; a second condition added through the UI completes the
        // filtered-empty branch. Nothing ever sits in the temp state,
        // so plain delete is safe.
        const filterName = `Parity EFilt ${suffix}`;
        const filterStateId = await serverCreateState(seed.workspaceSlug, projectId, filterName, "unstarted", session);
        try {
          await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, session, {
            rich_filters: MATCH_NOTHING,
          });
          await openPath(driver, freshPath, session);
          await driver.layoutsFilterAddConditionViaRow("State", filterName);
          await expect
            .poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 })
            .toEqual("No matching results.");
          expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear all filters", disabled: false }]);
          await driver.layoutsEmptyChoose("Clear all filters");
          await expect
            .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 300_000 })
            .toContain(title);
        } finally {
          await serverDeleteIssue(seed.workspaceSlug, projectId, id, session);
          await serverDeleteState(seed.workspaceSlug, projectId, filterStateId, session);
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
      .poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 })
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
    const suffix = uniqueSuffix();
    const now = new Date();
    const iso = (d: Date): string => d.toISOString().slice(0, 10);
    const start = new Date(now);
    start.setUTCDate(start.getUTCDate() - 3);
    const end = new Date(now);
    end.setUTCDate(end.getUTCDate() + 11);
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      `Parity ECyc ${suffix}`,
      iso(start),
      iso(end),
      session
    );
    const cyclePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycleId}`;
    const plainName = `Parity ecplain ${suffix}`;
    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, plainName);
    try {
      await openPath(driver, cyclePath, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 })
        .toEqual("No work items to show in this cycle");
      expect(await driver.layoutsEmptyActions()).toEqual([
        { label: "Create work item", disabled: false },
        { label: "Add existing work item", disabled: false },
      ]);

      await test.step("create opens the modal; add-existing attaches a real issue", async () => {
        await driver.layoutsEmptyChoose("Create work item");
        await expect.poll(async () => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toEqual(true);
        await driver.layoutsWorkItemModalClose();
        await driver.layoutsEmptyChoose("Add existing work item");
        await expect.poll(async () => driver.layoutsAddExistingModalVisible(), { timeout: 60_000 }).toEqual(true);
        await driver.layoutsAddExistingModalChoose(plainName);
        await expect
          .poll(async () => serverCycleIssueIds(seed.workspaceSlug, seed.projectId, cycleId, session), {
            timeout: 300_000,
          })
          .toContain(plainId);
      });

      await test.step("filtered-empty offers clearing filters", async () => {
        await serverPatchCycleUserProperties(seed.workspaceSlug, seed.projectId, cycleId, session, {
          rich_filters: MATCH_NOTHING,
        });
        await openPath(driver, cyclePath, session);
        await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toEqual("No matching results.");
        expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear filters", disabled: false }]);
        await driver.layoutsEmptyChoose("Clear filters");
        await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toBeNull();
      });
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, plainId, session).catch(() => {});
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, session);
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
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      `Parity ECycDone ${suffix}`,
      iso(start),
      iso(end),
      session
    );
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycleId}`, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 })
        .toEqual("No work items in the cycle");
      expect(await driver.layoutsEmptyActions()).toEqual([]);
    } finally {
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, session);
    }
  }
);

test(
  specTitle(["ISS-070"], "module empty states: fresh, add flows, filtered"),
  { tag: specTags(["ISS-070"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const suffix = uniqueSuffix();
    const moduleId = await serverCreateModule(seed.workspaceSlug, seed.projectId, `Parity EMod ${suffix}`, session);
    const modulePath = `/${seed.workspaceSlug}/projects/${seed.projectId}/modules/${moduleId}`;
    const plainName = `Parity emplain ${suffix}`;
    const plainId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, plainName);
    // Second-condition fixture for the filtered-empty branch (nothing
    // ever sits in it, so plain delete is safe).
    const filterName = `Parity EFiltM ${suffix}`;
    const filterStateId = await serverCreateState(seed.workspaceSlug, seed.projectId, filterName, "unstarted", session);
    try {
      await openPath(driver, modulePath, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 })
        .toEqual("No work items to show in this Module");
      expect(await driver.layoutsEmptyActions()).toEqual([
        { label: "Create work item", disabled: false },
        { label: "Add existing work item", disabled: false },
      ]);

      await driver.layoutsEmptyChoose("Add existing work item");
      await expect.poll(async () => driver.layoutsAddExistingModalVisible(), { timeout: 60_000 }).toEqual(true);
      await driver.layoutsAddExistingModalChoose(plainName);
      await expect
        .poll(async () => serverModuleIssueIds(seed.workspaceSlug, seed.projectId, moduleId, session), {
          timeout: 300_000,
        })
        .toContain(plainId);

      // The seeded condition empties the rows but leaves the fresh
      // card; a second condition added through the UI completes the
      // filtered-empty branch.
      await serverPatchModuleUserProperties(seed.workspaceSlug, seed.projectId, moduleId, session, {
        rich_filters: MATCH_NOTHING,
      });
      await openPath(driver, modulePath, session);
      await driver.layoutsFilterAddConditionViaRow("State", filterName);
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toEqual("No matching results.");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear filters", disabled: false }]);
      await driver.layoutsEmptyChoose("Clear filters");
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toBeNull();
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, plainId, session).catch(() => {});
      await serverDeleteModule(seed.workspaceSlug, seed.projectId, moduleId, session);
      await serverDeleteState(seed.workspaceSlug, seed.projectId, filterStateId, session).catch(() => {});
    }
  }
);

test(
  specTitle(["ISS-071"], "archived empty states link to automation, not creation"),
  { tag: specTags(["ISS-071"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const archivedPath = `/${seed.workspaceSlug}/projects/${seed.projectId}/archives/issues`;
    await openPath(driver, archivedPath, session);
    await expect
      .poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 })
      .toEqual("No archived work items yet");
    expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Set automation", disabled: false }]);

    await driver.layoutsEmptyChoose("Set automation");
    await expect.poll(async () => driver.layoutsCurrentUrl(), { timeout: 300_000 }).toContain("/automations/");

    // Filtered-empty needs a non-empty base: archive one issue first.
    const states = await serverListStates(seed.workspaceSlug, seed.projectId, session);
    let doneId = states.find((row) => row.group === "completed")?.id ?? "";
    let doneCreated = false;
    if (!doneId) {
      doneId = await serverCreateState(
        seed.workspaceSlug,
        seed.projectId,
        `Parity ADone ${uniqueSuffix()}`,
        "completed",
        session
      );
      doneCreated = true;
    }
    const title = `Parity archold ${uniqueSuffix()}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, title);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { state_id: doneId }, session);
    await serverArchiveIssue(seed.workspaceSlug, seed.projectId, id, session);
    // Second-condition fixture for the filtered-empty branch (nothing
    // ever sits in it, so plain delete is safe).
    const filterName = `Parity EFiltA ${uniqueSuffix()}`;
    const filterStateId = await serverCreateState(seed.workspaceSlug, seed.projectId, filterName, "unstarted", session);
    try {
      // The archived page never consults server preferences (and its
      // local read uses the wrong member — NEWFRONT-167), so the
      // match-nothing seed goes through its local store in the shape the
      // read path expects; a second condition added through the UI
      // completes the filtered-empty branch. Server preferences stay
      // untouched throughout.
      await openPath(driver, archivedPath, session);
      await driver.layoutsSeedArchivedLocalFilter(seed.workspaceSlug, seed.projectId, MATCH_NOTHING);
      await driver.layoutsFilterAddConditionViaRow("State", filterName);
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toEqual("No matching results.");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear filters", disabled: false }]);
      await driver.layoutsEmptyChoose("Clear filters");
      // Positive read: the archived row renders again.
      // bug:NEWFRONT-165 — the archived menu omits Delete; ISS-064 intends it.
      expect(await driver.layoutsRowMenuItems(title)).toEqual(["Restore", "Open in new tab", "Copy link"]);
    } finally {
      await serverRestoreIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await serverDeleteState(seed.workspaceSlug, seed.projectId, filterStateId, session).catch(() => {});
      if (doneCreated) await serverDeleteState(seed.workspaceSlug, seed.projectId, doneId, session);
    }
  }
);

test(
  specTitle(["ISS-072"], "saved-view empty state offers a new work item"),
  { tag: specTags(["ISS-072"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    // The view's rich expression matches nothing, so it renders empty.
    const viewId = await createProjectView(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Parity EView ${uniqueSuffix()}`,
      MATCH_NOTHING
    );
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${seed.projectId}/views/${viewId}`, session);
      await expect
        .poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 })
        .toEqual("View work items will appear here");
      expect(await driver.layoutsEmptyActions()).toEqual([{ label: "New work item", disabled: false }]);
      await driver.layoutsEmptyChoose("New work item");
      await expect.poll(async () => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toEqual(true);
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
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toEqual("No project");
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
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toEqual("No Views yet");
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
    ];
    for (const [tab, title] of cases) {
      await driver.openAuthenticated(`${base}/${tab}`, browserCookies(fresh.user));
      await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toEqual(title);
      expect(await driver.layoutsEmptyActions()).toEqual([]);
    }
    // The activity tab renders its Recent-activity chrome with an empty
    // list and no empty-state card.
    await driver.openAuthenticated(`${base}/activity`, browserCookies(fresh.user));
    await expect.poll(async () => driver.layoutsProfileActivityVisible(), { timeout: 300_000 }).toEqual(true);
    await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toBeNull();
    expect(await driver.layoutsEmptyActions()).toEqual([]);
    await driver.openAuthenticated(`${base}/zzz-unknown`, browserCookies(fresh.user));
    await expect.poll(async () => driver.layoutsCurrentUrl(), { timeout: 300_000 }).toContain("zzz-unknown");
    await expect.poll(async () => driver.layoutsEmptyTitle(), { timeout: 300_000 }).toBeNull();
  }
);

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-254): the active-cycle hero (progress,
// burn-down, and breakdown cards over the cycle covering today, plus the
// no-active-cycle empty view), the workspace paid upsell, timezone-offset
// rendering, background freshness, the guest read-only matrix, and
// touch/small-screen layout. Rows: CYC-040–CYC-045, CYC-047–CYC-048.
//
// Roles: the codebase models ADMIN/MEMBER/GUEST only (no separate viewer
// role), so the CYC-047 sweep runs as the seeded guest, the read-only role.
// Timezones: the seed stores UTC everywhere, so CYC-044 sets a scratch
// project's timezone and keeps the user on UTC (no shared user mutation).
import { test, expect } from "../fixtures";
import {
  createProjectLabel,
  parityApiBase,
  parityProjectIdentifier,
  requireMentionMember,
  serverAddProjectMembers,
  serverArchiveCycle,
  serverAttachCycleIssues,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateIssue,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverCycleDetail,
  serverCycleIssueIds,
  serverCycleProgress,
  serverCycleUserProperties,
  serverPatchCycle,
  serverPatchIssue,
  serverPatchProject,
  serverProjectCycles,
  serverProjectTimezone,
  serverRequestStatus,
  serverWorkspaceMemberId,
  signInSession,
  signInSessionRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Cycle dates that read CURRENT on the seeded stack (today is 2026-10-10). */
const CURRENT_START = "2026-09-01";
const CURRENT_END = "2026-12-31";
/** Cycle dates that read COMPLETED (the archive/transfer gate). */
const DONE_START = "2026-01-01";
const DONE_END = "2026-02-01";
/** Cycle dates that read UPCOMING. */
const UPCOMING_START = "2026-11-01";
const UPCOMING_END = "2026-12-31";

/** Project role numbers (backend ROLE enum): member edits, guest only reads. */
const ROLE_MEMBER = 15;
const ROLE_GUEST = 5;

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Scratch project with the cycles feature on; the caller deletes it. */
async function createCyclesProject(seed: ParitySeedFacts, name: string, session: string): Promise<string> {
  return serverCreateProjectWithFlags(
    seed.workspaceSlug,
    name,
    parityProjectIdentifier("N254"),
    { cycleView: true },
    session
  );
}

/**
 * Create a cycle reading COMPLETED: the create call takes current dates
 * (the server rejects completed-looking creates and refuses attaches to
 * finished cycles), then a patch backdates it into the gate.
 */
async function createCompletedCycle(
  seed: ParitySeedFacts,
  projectId: string,
  name: string,
  session: string,
  issueIds: string[] = []
): Promise<string> {
  const id = await serverCreateCycle(seed.workspaceSlug, projectId, name, CURRENT_START, CURRENT_END, session);
  if (issueIds.length > 0) await serverAttachCycleIssues(seed.workspaceSlug, projectId, id, issueIds, session);
  await serverPatchCycle(seed.workspaceSlug, projectId, id, { start_date: DONE_START, end_date: DONE_END }, session);
  return id;
}

/** Most recent toast text, polled until non-empty. */
async function toastText(driver: ParityDriver): Promise<string> {
  let text: string | null = null;
  await expect
    .poll(
      async () => {
        text = await driver.toastText();
        return text;
      },
      { timeout: 30_000 }
    )
    .not.toBeNull();
  return text ?? "";
}

test(
  specTitle(["CYC-040"], "detail sidebar shows a skeleton until the cycle data arrives"),
  { tag: specTags(["CYC-040"]) },
  async ({ driver, seed }) => {
    const tag = `NF254s40a ${Date.now().toString(36)}`;
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S40a ${Date.now().toString(36)} project`, ownerSession);
    try {
      const name = `S40a cycle ${tag}`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        name,
        CURRENT_START,
        CURRENT_END,
        ownerSession
      );
      await driver.cyclesHeroDelayDetailReads(10000);
      await driver.cyclesHeroOpenDetail(seed.workspaceSlug, projectId, cycleId);
      await expect.poll(() => driver.cyclesHeroSidebarSkeletonVisible(), { timeout: 30_000 }).toBe(true);
      expect(await driver.cyclesHeroSidebarName()).toBe(null);
      await expect.poll(() => driver.cyclesHeroSidebarName(), { timeout: 90_000 }).toBe(name);
      expect(await driver.cyclesHeroSidebarSkeletonVisible()).toBe(false);
      // The same read the skeleton waited on carries the cycle server-side.
      expect((await serverCycleDetail(seed.workspaceSlug, projectId, cycleId, ownerSession)).name).toBe(name);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-040"], "Quick-look panel shows a skeleton until the cycle data arrives"),
  { tag: specTags(["CYC-040"]) },
  async ({ driver, seed }) => {
    const tag = `NF254s40b ${Date.now().toString(36)}`;
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S40b ${Date.now().toString(36)} project`, ownerSession);
    try {
      const name = `S40b cycle ${tag}`;
      const anchor = `S40b anchor ${tag}`;
      await serverCreateCycle(seed.workspaceSlug, projectId, anchor, UPCOMING_START, UPCOMING_END, ownerSession);
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        name,
        CURRENT_START,
        CURRENT_END,
        ownerSession
      );
      await driver.cyclesHeroDelayDetailReads(10000);
      await driver.cyclesHeroHideCycleFromListReads(cycleId);
      await driver.cyclesHeroOpenPeekRaw(seed.workspaceSlug, projectId, cycleId);
      await expect.poll(() => driver.cyclesHeroPeekSkeletonVisible(), { timeout: 30_000 }).toBe(true);
      expect(await driver.cyclesHeroPeekName()).toBe(null);
      await expect.poll(() => driver.cyclesHeroPeekName(), { timeout: 90_000 }).toBe(name);
      expect(await driver.cyclesHeroPeekSkeletonVisible()).toBe(false);
      expect((await serverCycleDetail(seed.workspaceSlug, projectId, cycleId, ownerSession)).name).toBe(name);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

/**
 * Active cycle with two assigned work items in different state groups,
 * one urgent and one carrying a label: the hero cards have something to
 * break down on every tab.
 */
async function setupHeroCycle(
  seed: ParitySeedFacts,
  projectId: string,
  tag: string,
  ownerSession: string
): Promise<{ cycleId: string; cycleName: string; startedIssue: string; backlogIssue: string; labelName: string }> {
  const member = requireMentionMember(seed);
  const memberId = await serverWorkspaceMemberId(seed.workspaceSlug, member.email, ownerSession);
  await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId, role: ROLE_MEMBER }], ownerSession);
  const labelName = `S41 label ${tag}`;
  const label = await createProjectLabel(seed.workspaceSlug, projectId, ownerSession, labelName);
  // Transitions to the default-seeded states are silently ignored; a
  // scenario-owned state in the started group takes the PATCH.
  const startedStateId = await serverCreateState(
    seed.workspaceSlug,
    projectId,
    `S41 started state ${tag}`,
    "started",
    ownerSession
  );
  const cycleName = `S41 active ${tag}`;
  const cycleId = await serverCreateCycle(
    seed.workspaceSlug,
    projectId,
    cycleName,
    CURRENT_START,
    CURRENT_END,
    ownerSession
  );
  const startedIssue = `S41 started ${tag}`;
  const backlogIssue = `S41 backlog ${tag}`;
  const startedId = await serverCreateIssue(seed.workspaceSlug, projectId, ownerSession, startedIssue);
  const backlogId = await serverCreateIssue(seed.workspaceSlug, projectId, ownerSession, backlogIssue);
  await serverPatchIssue(
    seed.workspaceSlug,
    projectId,
    startedId,
    { state_id: startedStateId, priority: "urgent", assignee_ids: [member.id], label_ids: [label.id] },
    ownerSession
  );
  await serverPatchIssue(seed.workspaceSlug, projectId, backlogId, { assignee_ids: [member.id] }, ownerSession);
  await serverAttachCycleIssues(seed.workspaceSlug, projectId, cycleId, [startedId, backlogId], ownerSession);
  return { cycleId, cycleName, startedIssue, backlogIssue, labelName };
}

test(
  specTitle(["CYC-041"], "Active-cycle hero shows progress, burn-down, and breakdown cards"),
  { tag: specTags(["CYC-041"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S41a ${tag} project`, ownerSession);
    try {
      const hero = await setupHeroCycle(seed, projectId, tag, ownerSession);
      await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);

      await test.step("progress card breaks the cycle down by state", async () => {
        expect(await driver.cyclesHeroGroupHeading()).toMatch(/active cycle/i);
        expect(await driver.cyclesHeroActiveName()).toBe(hero.cycleName);
        const progress = await driver.cyclesHeroProgressGroups();
        expect(progress.closed).toMatch(/closed/i);
        const names = progress.groups.map((group) => group.name.toLowerCase());
        expect(names).toContain("started");
        expect(names).toContain("backlog");
        expect(await driver.cyclesHeroProgressEmptyVisible()).toBe(false);
        const server = await serverCycleProgress(seed.workspaceSlug, projectId, hero.cycleId, ownerSession);
        expect(server.total).toBe(2);
      });

      await test.step("burn-down card shows the pending line and the chart", async () => {
        const burndown = await driver.cyclesHeroBurndown();
        expect(burndown.heading).toMatch(/burndown/i);
        expect(burndown.pending).toMatch(/pending/i);
        expect(burndown.chart).toBe(true);
        expect(await driver.cyclesHeroBurndownEmptyVisible()).toBe(false);
      });

      await test.step("breakdown card tabs split by priority, assignee, and label", async () => {
        expect(await driver.cyclesHeroBreakdownTabs()).toEqual(["Priority work items", "Assignees", "Labels"]);
        await driver.cyclesHeroBreakdownSelectTab("Priority work items");
        expect(await driver.cyclesHeroBreakdownEntries()).toContain(hero.startedIssue);
        await driver.cyclesHeroBreakdownSelectTab("Assignees");
        const assignees = await driver.cyclesHeroBreakdownEntries();
        expect(assignees.join("\n")).toContain(member.displayName);
        expect(await driver.cyclesHeroBreakdownEmptyVisible()).toBe(false);
        await driver.cyclesHeroBreakdownSelectTab("Labels");
        expect((await driver.cyclesHeroBreakdownEntries()).join("\n")).toContain(hero.labelName);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-041"], "Hero breakdown interactions filter the cycle view"),
  { tag: specTags(["CYC-041"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const member = requireMentionMember(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S41b ${tag} project`, ownerSession);
    try {
      const hero = await setupHeroCycle(seed, projectId, tag, ownerSession);
      // Prime the user-properties row the clicks persist through: opening
      // the detail once is the normal flow (bug NEWFRONT-266 covers the
      // unprimed first click, which dies on a PATCH 404).
      await driver.cyclesHeroOpenDetail(seed.workspaceSlug, projectId, hero.cycleId);
      await expect.poll(() => driver.cyclesHeroSidebarName(), { timeout: 60_000 }).toBe(hero.cycleName);
      await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);

      await test.step("a progress group opens the filtered detail", async () => {
        await driver.cyclesHeroClickProgressGroup("backlog");
        expect(await driver.currentPath()).toContain(`/cycles/${hero.cycleId}`);
        await expect
          .poll(
            async () =>
              JSON.stringify(
                (await serverCycleUserProperties(seed.workspaceSlug, projectId, hero.cycleId, ownerSession)).richFilters
              ),
            { timeout: 30_000 }
          )
          .toContain("backlog");
      });

      await test.step("an assignee entry opens the filtered detail", async () => {
        await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
        await driver.cyclesHeroBreakdownSelectTab("Assignees");
        await driver.cyclesHeroClickBreakdownEntry(0);
        expect(await driver.currentPath()).toContain(`/cycles/${hero.cycleId}`);
        await expect
          .poll(
            async () =>
              JSON.stringify(
                (await serverCycleUserProperties(seed.workspaceSlug, projectId, hero.cycleId, ownerSession)).richFilters
              ),
            { timeout: 30_000 }
          )
          .toContain(member.id);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-041"], "bug: NEWFRONT-266 fresh breakdown clicks die before the properties row exists"),
  { tag: specTags(["CYC-041"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S41d ${tag} project`, ownerSession);
    try {
      const hero = await setupHeroCycle(seed, projectId, tag, ownerSession);
      // No detail visit: the properties row does not exist yet, so the
      // click's PATCH 404s and the handler dies before navigating.
      await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
      await driver.cyclesHeroTapProgressGroup("backlog");
      await driver.page.waitForTimeout(5000);
      expect(await driver.currentPath()).not.toContain(`/cycles/${hero.cycleId}`);
      expect(
        (await serverCycleUserProperties(seed.workspaceSlug, projectId, hero.cycleId, ownerSession)).richFilters
      ).toEqual({});
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-041"], "Hero cards show per-card empty views with nothing to break down"),
  { tag: specTags(["CYC-041"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S41c ${tag} project`, ownerSession);
    try {
      const name = `S41 empty ${tag}`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        name,
        CURRENT_START,
        CURRENT_END,
        ownerSession
      );
      await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
      expect(await driver.cyclesHeroActiveName()).toBe(name);
      expect(await driver.cyclesHeroProgressEmptyVisible()).toBe(true);
      expect(await driver.cyclesHeroBurndownEmptyVisible()).toBe(true);
      expect(await driver.cyclesHeroBreakdownTabs()).toEqual(["Priority work items", "Assignees", "Labels"]);
      for (const tab of ["Priority work items", "Assignees", "Labels"]) {
        await driver.cyclesHeroBreakdownSelectTab(tab);
        expect(await driver.cyclesHeroBreakdownEmptyVisible()).toBe(true);
      }
      expect((await serverCycleProgress(seed.workspaceSlug, projectId, cycleId, ownerSession)).total).toBe(0);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-042"], "Hero explains the active cycle when none covers today"),
  { tag: specTags(["CYC-042"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S42 ${tag} project`, ownerSession);
    try {
      // Date windows avoiding today: no cycle covers it, so no hero cards.
      await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        `S42 future ${tag}`,
        UPCOMING_START,
        UPCOMING_END,
        ownerSession
      );
      await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
      expect(await driver.cyclesHeroGroupHeading()).toMatch(/active cycle/i);
      expect(await driver.cyclesHeroActiveName()).toBe(null);
      const empty = await driver.cyclesHeroEmptyCopy();
      expect(empty).not.toBe(null);
      expect(empty?.title ?? "").toMatch(/active cycle/i);
      expect(empty?.description ?? "").toMatch(/today/i);
      expect(await driver.cyclesHeroBreakdownTabs()).toEqual([]);
      const statuses = (await serverProjectCycles(seed.workspaceSlug, projectId, ownerSession)).map(
        (row) => row.status
      );
      expect(statuses).not.toContain("CURRENT");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-043"], "Workspace actives page is a paid upsell with a pricing path"),
  { tag: specTags(["CYC-043"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S43 ${tag} project`, ownerSession);
    try {
      // The gate is workspace-level: live cycles elsewhere change nothing.
      await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        `S43 active ${tag}`,
        CURRENT_START,
        CURRENT_END,
        ownerSession
      );
      await driver.cyclesHeroOpenWorkspaceActives(seed.workspaceSlug);
      const upsell = await driver.cyclesHeroUpsell();
      expect(upsell.heading).toMatch(/snapshots/i);
      expect(upsell.benefits).toBe(6);
      expect(upsell.upgradeHref ?? "").toContain("pricing");
      expect(upsell.upgradeTarget).toBe("_blank");
      expect(await driver.cyclesHeroUpsellBadgeVisible()).toBe(true);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-044"], "Cycle dates carry the project offset and user-timezone equivalents"),
  { tag: specTags(["CYC-044"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S44a ${tag} project`, ownerSession);
    try {
      const name = `S44 active ${tag}`;
      const cycleId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        name,
        CURRENT_START,
        CURRENT_END,
        ownerSession
      );
      await serverPatchProject(seed.workspaceSlug, projectId, ownerSession, { timezone: "America/New_York" });
      expect(await serverProjectTimezone(seed.workspaceSlug, projectId, ownerSession)).toBe("America/New_York");

      await test.step("active row shows the offset chip and hover equivalents", async () => {
        await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
        expect(await driver.cyclesHeroActiveRowOffset()).toMatch(/^UTC -0[45]:00$/);
        await driver.cyclesHeroHoverActiveRowDates();
        const tip = await driver.cyclesHeroTooltipText();
        expect(tip ?? "").toMatch(/your timezone/i);
        expect(tip ?? "").toContain("2026");
      });

      await test.step("detail sidebar shows the offset chip and hover equivalents", async () => {
        await driver.cyclesHeroOpenDetail(seed.workspaceSlug, projectId, cycleId);
        await expect.poll(() => driver.cyclesHeroSidebarName(), { timeout: 90_000 }).toBe(name);
        expect(await driver.cyclesHeroSidebarOffset()).toMatch(/^UTC -0[45]:00$/);
        await driver.cyclesHeroHoverSidebarDates();
        const tip = await driver.cyclesHeroTooltipText();
        expect(tip ?? "").toMatch(/your timezone/i);
        expect(tip ?? "").toContain("2026");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-044"], "No offset chip when the project timezone is UTC"),
  { tag: specTags(["CYC-044"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S44b ${tag} project`, ownerSession);
    try {
      expect(await serverProjectTimezone(seed.workspaceSlug, projectId, ownerSession)).toBe("UTC");
      await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        `S44 utc ${tag}`,
        CURRENT_START,
        CURRENT_END,
        ownerSession
      );
      await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
      expect(await driver.cyclesHeroActiveRowOffset()).toBe(null);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-045"], "Creating a cycle refreshes the list without a reload"),
  { tag: specTags(["CYC-045"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S45a ${tag} project`, ownerSession);
    try {
      const anchor = `S45 anchor ${tag}`;
      await serverCreateCycle(seed.workspaceSlug, projectId, anchor, UPCOMING_START, UPCOMING_END, ownerSession);
      await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
      await driver.cyclesHeroBeginTrafficSpy();
      const name = `S45 created ${tag}`;
      await driver.cyclesHeroOpenCreate();
      await driver.cyclesHeroFillCreateName(name);
      await driver.cyclesHeroSubmitCreate();
      // No reload between submit and the new row: the list refreshes itself.
      await expect.poll(() => driver.cyclesHeroVisibleNames(), { timeout: 30_000 }).toContain(name);
      const cycles = await serverProjectCycles(seed.workspaceSlug, projectId, ownerSession);
      expect(cycles.map((row) => row.name)).toContain(name);
      expect((await driver.cyclesHeroTrafficCounts()).writes).toBeGreaterThanOrEqual(1);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-045"], "bug: NEWFRONT-267 transfer refreshes the target hero but the source banner stays stale"),
  { tag: specTags(["CYC-045"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S45b ${tag} project`, ownerSession);
    try {
      const target = `S45 target ${tag}`;
      const targetId = await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        target,
        CURRENT_START,
        CURRENT_END,
        ownerSession
      );
      const leftover = `S45 leftover ${tag}`;
      const leftoverId = await serverCreateIssue(seed.workspaceSlug, projectId, ownerSession, leftover);
      const doneName = `S45 done ${tag}`;
      const doneId = await createCompletedCycle(seed, projectId, doneName, ownerSession, [leftoverId]);
      await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
      expect(await driver.cyclesHeroTransferBanner(doneName)).toMatch(/transfer/i);
      await driver.cyclesHeroOpenTransfer(doneName);
      expect(await driver.cyclesHeroTransferOptions()).toContain(target);
      await driver.cyclesHeroTransferSearch(target.slice(0, 12));
      expect(await driver.cyclesHeroTransferOptions()).toEqual([target]);
      await driver.cyclesHeroTransferPick(target);
      expect(await toastText(driver)).toMatch(/transferred successfully/i);
      // No reload: the target hero picks up the transferred work item.
      await expect
        .poll(async () => (await driver.cyclesHeroProgressGroups()).closed, { timeout: 30_000 })
        .toMatch(/0\/1/);
      const names = (await driver.cyclesHeroProgressGroups()).groups.map((group) => group.name.toLowerCase());
      expect(names).toContain("backlog");
      expect(await serverCycleIssueIds(seed.workspaceSlug, projectId, targetId, ownerSession)).toContain(leftoverId);
      expect(await serverCycleIssueIds(seed.workspaceSlug, projectId, doneId, ownerSession)).not.toContain(leftoverId);
      expect((await serverCycleProgress(seed.workspaceSlug, projectId, targetId, ownerSession)).total).toBe(1);
      // The bug: the source banner reads the stale snapshot, so it claims
      // leftovers that no longer exist until a reload repopulates it.
      await driver.page.waitForTimeout(5000);
      expect(await driver.cyclesHeroTransferBanner(doneName)).toMatch(/transfer/i);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-045"], "Archived list never refetches on window focus"),
  { tag: specTags(["CYC-045"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const ownerSession = await signInSession(seed.email, seed.password);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    const projectId = await createCyclesProject(seed, `NF254 S45c ${tag} project`, ownerSession);
    try {
      const archivedName = `S45 archived ${tag}`;
      const archivedId = await createCompletedCycle(seed, projectId, archivedName, ownerSession);
      await serverArchiveCycle(seed.workspaceSlug, projectId, archivedId, ownerSession);
      await serverCreateCycle(
        seed.workspaceSlug,
        projectId,
        `S45 live ${tag}`,
        CURRENT_START,
        CURRENT_END,
        ownerSession
      );
      // Live-first entry: fresh archived loads never settle (NEWFRONT-231).
      await driver.cyclesHeroBeginTrafficSpy();
      await driver.archivesCyclesOpenTabViaLive(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.archivesCyclesVisibleNames(), { timeout: 30_000 }).toContain(archivedName);
      const before = await driver.cyclesHeroTrafficCounts();
      expect(before.archivedReads).toBeGreaterThanOrEqual(1);
      await driver.cyclesHeroFocusWindow();
      await driver.page.waitForTimeout(3000);
      const after = await driver.cyclesHeroTrafficCounts();
      expect(after.archivedReads).toBe(before.archivedReads);
      expect(await driver.archivesCyclesVisibleNames()).toContain(archivedName);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
    }
  }
);

/** Scratch project with an active, an upcoming, and a finished cycle, readable by the seeded guest. */
async function setupGuestProject(
  seed: ParitySeedFacts,
  tag: string,
  ownerSession: string
): Promise<{
  projectId: string;
  activeName: string;
  activeId: string;
  upcomingName: string;
  doneName: string;
  doneId: string;
}> {
  const guestCreds = requireGuest(seed);
  const guestId = await serverWorkspaceMemberId(seed.workspaceSlug, guestCreds.email, ownerSession);
  const projectId = await createCyclesProject(seed, `NF254 S47 ${tag} project`, ownerSession);
  await serverPatchProject(seed.workspaceSlug, projectId, ownerSession, { guest_view_all_features: true });
  await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: guestId, role: ROLE_GUEST }], ownerSession);
  const activeName = `S47 active ${tag}`;
  const activeId = await serverCreateCycle(
    seed.workspaceSlug,
    projectId,
    activeName,
    CURRENT_START,
    CURRENT_END,
    ownerSession
  );
  const upcomingName = `S47 upcoming ${tag}`;
  await serverCreateCycle(seed.workspaceSlug, projectId, upcomingName, UPCOMING_START, UPCOMING_END, ownerSession);
  const leftoverId = await serverCreateIssue(seed.workspaceSlug, projectId, ownerSession, `S47 leftover ${tag}`);
  const doneName = `S47 done ${tag}`;
  const doneId = await createCompletedCycle(seed, projectId, doneName, ownerSession, [leftoverId]);
  return { projectId, activeName, activeId, upcomingName, doneName, doneId };
}

test(
  specTitle(["CYC-047"], "Guests browse, peek, and copy cycle links"),
  { tag: specTags(["CYC-047"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const guestCreds = requireGuest(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const project = await setupGuestProject(seed, tag, ownerSession);
    try {
      await driver.rulesEnsureSignedIn(guestCreds.email, guestCreds.password, seed.workspaceSlug);
      await driver.cyclesHeroOpenList(seed.workspaceSlug, project.projectId);

      await test.step("browse the hero and the groups", async () => {
        expect(await driver.cyclesHeroActiveName()).toBe(project.activeName);
        expect(await driver.cyclesHeroVisibleNames()).toContain(project.upcomingName);
      });

      await test.step("peek opens the quick-look panel", async () => {
        await driver.cyclesHeroOpenPeek(project.upcomingName);
        expect(await driver.cyclesHeroPeekName()).toBe(project.upcomingName);
        expect(await driver.currentPath()).toContain("peekCycle");
      });

      await test.step("copy link confirms", async () => {
        await driver.cyclesHeroGrantClipboard();
        await driver.cyclesHeroOpenRowMenu(project.upcomingName);
        await driver.cyclesHeroRowMenuClick("Copy link");
        expect(await toastText(driver)).toMatch(/copied/i);
        expect(await driver.cyclesHeroReadClipboard()).toContain("/cycles/");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, project.projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-047"], "bug: NEWFRONT-268 guests lose list search and filter to the create gate"),
  { tag: specTags(["CYC-047"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const guestCreds = requireGuest(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const project = await setupGuestProject(seed, tag, ownerSession);
    try {
      await test.step("search and filter work for members", async () => {
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.cyclesHeroOpenList(seed.workspaceSlug, project.projectId);
        expect(await driver.cyclesHeroSearchVisible()).toBe(true);
        await driver.cyclesHeroSearchOpen();
        // A query matching nothing filters the rows out (the hero reads
        // the unfiltered active id, so rows — not the hero — prove it).
        const noMatch = `zzz-no-cycle-${tag}`;
        await driver.cyclesHeroSearchType(noMatch);
        expect(await driver.cyclesHeroSearchText()).toBe(noMatch);
        await expect
          .poll(() => driver.cyclesHeroVisibleNames(), { timeout: 30_000 })
          .not.toContain(project.upcomingName);
        await driver.cyclesHeroSearchType("");
        await expect.poll(() => driver.cyclesHeroVisibleNames(), { timeout: 30_000 }).toContain(project.upcomingName);
        expect(await driver.cyclesHeroFilterVisible()).toBe(true);
        expect((await driver.cyclesHeroFilterMenuTexts()).length).toBeGreaterThan(0);
        await driver.cyclesHeroCloseMenus();
      });

      await test.step("both controls are absent for guests", async () => {
        // Sign-in is a no-op while a session lives, so sign the member
        // out first — otherwise this step still runs as the member.
        await driver.signOutViaAccountMenu();
        await driver.rulesEnsureSignedIn(guestCreds.email, guestCreds.password, seed.workspaceSlug);
        await driver.cyclesHeroOpenList(seed.workspaceSlug, project.projectId);
        expect(await driver.cyclesHeroSearchVisible()).toBe(false);
        expect(await driver.cyclesHeroFilterVisible()).toBe(false);
        // Read-only reach still works: the rows render and peek opens.
        expect(await driver.cyclesHeroVisibleNames()).toContain(project.upcomingName);
        await driver.cyclesHeroOpenPeek(project.upcomingName);
        expect(await driver.cyclesHeroPeekName()).toBe(project.upcomingName);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, project.projectId, ownerSession);
    }
  }
);

test(
  specTitle(["CYC-047"], "Guests see no working cycle mutation affordance"),
  { tag: specTags(["CYC-047"]) },
  async ({ driver, seed }) => {
    const tag = Date.now().toString(36);
    const guestCreds = requireGuest(seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    const guestSession = await signInSessionRetry(guestCreds.email, guestCreds.password);
    const project = await setupGuestProject(seed, tag, ownerSession);
    try {
      await driver.rulesEnsureSignedIn(guestCreds.email, guestCreds.password, seed.workspaceSlug);
      await driver.cyclesHeroOpenList(seed.workspaceSlug, project.projectId);

      await test.step("no creation entry point", async () => {
        expect(await driver.cyclesHeroCreateButtonState()).toBe("absent");
      });

      await test.step("row menus carry read-only entries only", async () => {
        for (const name of [project.activeName, project.upcomingName, project.doneName]) {
          await driver.cyclesHeroOpenRowMenu(name);
          const entries = (await driver.cyclesHeroRowMenuEntries()).join("\n");
          expect(entries).toContain("Copy link");
          expect(entries).not.toMatch(/edit|archive|restore|delete/i);
          await driver.cyclesHeroCloseMenus();
        }
        expect(await driver.cyclesHeroFavoriteVisible(project.upcomingName)).toBe(false);
      });

      await test.step("sidebar dates are locked", async () => {
        await driver.cyclesHeroOpenDetail(seed.workspaceSlug, project.projectId, project.activeId);
        await expect.poll(() => driver.cyclesHeroSidebarName(), { timeout: 30_000 }).toBe(project.activeName);
        expect(await driver.cyclesHeroSidebarDateDisabled()).toBe(true);
      });

      await test.step("transfer fails gracefully and the server refuses writes", async () => {
        await driver.cyclesHeroOpenList(seed.workspaceSlug, project.projectId);
        const banner = await driver.cyclesHeroTransferBanner(project.doneName);
        if (banner !== null) {
          const before = await serverCycleIssueIds(seed.workspaceSlug, project.projectId, project.doneId, ownerSession);
          await driver.cyclesHeroOpenTransfer(project.doneName);
          const options = await driver.cyclesHeroTransferOptions();
          if (options.length > 0) {
            await driver.cyclesHeroTransferPick(options[0] ?? "");
            expect(await toastText(driver)).toMatch(/unable|error|denied|forbidden/i);
          } else {
            await driver.cyclesHeroCloseMenus();
          }
          expect(
            await serverCycleIssueIds(seed.workspaceSlug, project.projectId, project.doneId, ownerSession)
          ).toEqual(before);
        }
        const refusal = await serverRequestStatus(
          "PATCH",
          `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${project.projectId}/cycles/${project.activeId}/`,
          guestSession,
          { name: `S47 renamed ${tag}` }
        );
        expect([401, 403]).toContain(refusal.status);
        expect(
          (await serverCycleDetail(seed.workspaceSlug, project.projectId, project.activeId, ownerSession)).name
        ).toBe(project.activeName);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, project.projectId, ownerSession);
    }
  }
);

test.describe("touch layouts", () => {
  test.use({
    viewport: { width: 390, height: 844 },
    userAgent:
      "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
  });

  test(
    specTitle(["CYC-048"], "Cycle row menus render inline on narrow screens"),
    { tag: specTags(["CYC-048"]) },
    async ({ driver, seed }) => {
      const tag = Date.now().toString(36);
      const ownerSession = await signInSession(seed.email, seed.password);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      const projectId = await createCyclesProject(seed, `NF254 S48a ${tag} project`, ownerSession);
      try {
        const activeName = `S48 active ${tag}`;
        await serverCreateCycle(seed.workspaceSlug, projectId, activeName, CURRENT_START, CURRENT_END, ownerSession);
        const upcomingName = `S48 upcoming ${tag}`;
        await serverCreateCycle(
          seed.workspaceSlug,
          projectId,
          upcomingName,
          UPCOMING_START,
          UPCOMING_END,
          ownerSession
        );
        await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
        await driver.cyclesHeroDismissSidebar();
        for (const name of [activeName, upcomingName]) {
          expect(await driver.cyclesHeroInlineActionsVisible(name)).toBe(true);
          expect(await driver.cyclesHeroHoverActionsVisible(name)).toBe(false);
        }
        // The inline menu opens the same entries without hovering.
        await driver.cyclesHeroOpenRowMenu(upcomingName);
        const entries = (await driver.cyclesHeroRowMenuEntries()).join("\n");
        expect(entries).toContain("Copy link");
        expect(entries).toContain("Edit");
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
      }
    }
  );

  test(
    specTitle(["CYC-048"], "Layout switching stays reachable on narrow screens"),
    { tag: specTags(["CYC-048"]) },
    async ({ driver, seed }) => {
      const tag = Date.now().toString(36);
      const ownerSession = await signInSession(seed.email, seed.password);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      const projectId = await createCyclesProject(seed, `NF254 S48b ${tag} project`, ownerSession);
      try {
        const activeName = `S48 active ${tag}`;
        const activeId = await serverCreateCycle(
          seed.workspaceSlug,
          projectId,
          activeName,
          CURRENT_START,
          CURRENT_END,
          ownerSession
        );

        await test.step("list layout switcher offers list and gallery only", async () => {
          await driver.cyclesHeroOpenList(seed.workspaceSlug, projectId);
          await driver.cyclesHeroDismissSidebar();
          expect(await driver.cyclesHeroLayoutMenuVisible()).toBe(true);
          expect(await driver.cyclesHeroLayoutOptions()).toEqual(["List layout", "Gallery layout"]);
          await driver.cyclesHeroPickLayout("Gallery layout");
          expect(await driver.cyclesHeroVisibleNames()).toContain(activeName);
        });

        await test.step("detail layout switcher changes the work-item layout", async () => {
          await driver.cyclesHeroOpenDetail(seed.workspaceSlug, projectId, activeId);
          await driver.cyclesHeroDismissSidebar();
          await expect.poll(() => driver.cyclesHeroSidebarName(), { timeout: 60_000 }).toBe(activeName);
          expect(await driver.cyclesHeroDetailLayoutMenuVisible()).toBe(true);
          const options = await driver.cyclesHeroDetailLayoutOptions();
          expect(options).toContain("Board");
          await driver.cyclesHeroDetailPickLayout("Board");
          await expect
            .poll(
              async () =>
                JSON.stringify(
                  (await serverCycleUserProperties(seed.workspaceSlug, projectId, activeId, ownerSession))
                    .displayFilters
                ),
              { timeout: 30_000 }
            )
            .toContain("kanban");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, ownerSession);
      }
    }
  );
});

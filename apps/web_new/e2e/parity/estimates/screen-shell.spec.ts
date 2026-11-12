// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-321): estimates settings screen shell —
// the admin gate with its not-authorized view (EST-001), loading
// skeletons (EST-002), the empty state with its add action (EST-003),
// the single settings URL with project-name title and no per-system
// deep link (EST-026), fetch-based freshness with no live push
// (EST-027), and the pinned non-behaviors (EST-028).
// Rows: EST-001–EST-003, EST-026–EST-028.
//
// Absence specs keep the marker words out of their own tags — the
// sidebar lists scenario projects in the page text.
//
// One browser role per test: the shared sign-in helper keeps an
// existing session, so switching users mid-test would silently keep
// the first session. Fresh contexts per test give each role a clean
// sign-in instead.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverCreateEstimate,
  serverCreateProjectWithFlags,
  serverEstimatesList,
  serverProjectMembers,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Seed guest user id, resolved live (never hardcoded across stacks). */
async function seedGuestUserId(seed: ParitySeedFacts, session: string): Promise<string> {
  const members = await serverProjectMembers(seed.workspaceSlug, seed.projectId, session);
  const guest = members.find((row) => row.role === 5);
  if (!guest) throw new Error("[parity] seed project has no role-5 guest member.");
  return guest.userId;
}

/** Remove a provisioned workspace member; lenient for finally-blocks. */
async function cleanupProvisionedMember(seed: ParitySeedFacts, userId: string, session: string): Promise<void> {
  const membership = (await serverWorkspaceMembers(seed.workspaceSlug, session).catch(() => [])) as {
    membershipId: string;
    userId: string;
  }[];
  const mine = membership.find((row) => row.userId === userId);
  if (mine) await serverCleanupWorkspaceMember(seed.workspaceSlug, mine.membershipId, session);
}

test(
  specTitle(["EST-001"], "project admins open estimates settings with breadcrumb and heading"),
  { tag: specTags(["EST-001"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 gate ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const guestId = await seedGuestUserId(seed, session);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf321gate", session);
    await serverCompleteOnboarding(member.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    await serverAddProjectMembers(
      seed.workspaceSlug,
      projectId,
      [
        { memberId: member.userId, role: 15 },
        { memberId: guestId, role: 5 },
      ],
      session
    );
    try {
      await test.step("the project carries exactly the admin, member and guest roles", async () => {
        const members = await serverProjectMembers(seed.workspaceSlug, projectId, session);
        expect(members.find((row) => row.userId === member.userId)?.role).toBe(15);
        expect(members.find((row) => row.userId === guestId)?.role).toBe(5);
        const adminRows = members.filter((row) => row.userId !== member.userId && row.userId !== guestId);
        expect(adminRows).toHaveLength(1);
        expect(adminRows[0]?.role).toBe(20);
      });

      await test.step("project admins see the breadcrumb and heading", async () => {
        await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
        await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
        expect(await driver.estimatesNotAuthorized()).toBe(false);
        expect(await driver.estimatesHeadingVisible()).toBe(true);
        expect(await driver.estimatesBreadcrumbText()).toContain("Estimates");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await cleanupProvisionedMember(seed, member.userId, session);
    }
  }
);

test(
  specTitle(["EST-001"], "non-admin members see the not-authorized view on estimates settings"),
  { tag: specTags(["EST-001"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 gate ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf321gatem", session);
    await serverCompleteOnboarding(member.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.userId, role: 15 }], session);
    try {
      expect(
        (await serverProjectMembers(seed.workspaceSlug, projectId, session)).find((row) => row.userId === member.userId)
          ?.role
      ).toBe(15);
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(true);
      expect(await driver.estimatesHeadingVisible()).toBe(false);
      expect(await driver.estimatesEmptyVisible()).toBe(false);
      expect(await driver.estimatesListedNames()).toEqual([]);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await cleanupProvisionedMember(seed, member.userId, session);
    }
  }
);

test(
  specTitle(["EST-001"], "guests see the not-authorized view on estimates settings"),
  { tag: specTags(["EST-001"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 gate ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guestId = await seedGuestUserId(seed, session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: guestId, role: 5 }], session);
    try {
      expect(
        (await serverProjectMembers(seed.workspaceSlug, projectId, session)).find((row) => row.userId === guestId)?.role
      ).toBe(5);
      await driver.rulesEnsureSignedIn(guestCreds.email, guestCreds.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(true);
      expect(await driver.estimatesHeadingVisible()).toBe(false);
      expect(await driver.estimatesEmptyVisible()).toBe(false);
      expect(await driver.estimatesListedNames()).toEqual([]);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-002"], "estimates settings shows skeletons while the systems load"),
  { tag: specTags(["EST-002"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 load ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      // The shared open settles on the loaded heading, past the skeleton
      // window — so hold the list answer and reload: the refetch shows
      // the skeleton first, then the real content replaces it.
      await driver.estimatesDelayListFetch(3_000);
      await driver.reloadPage();
      await expect.poll(() => driver.estimatesSkeletonVisible(), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.estimatesEmptyTitle(), { timeout: 30_000 }).toBe("No estimates yet");
      expect(await serverEstimatesList(seed.workspaceSlug, projectId, session)).toEqual([]);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-003"], "empty estimates settings offers the add action that opens creation"),
  { tag: specTags(["EST-003"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 void ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);

      await test.step("a project with no system shows the empty state", async () => {
        expect(await serverEstimatesList(seed.workspaceSlug, projectId, session)).toEqual([]);
        expect(await driver.estimatesEmptyVisible()).toBe(true);
        expect(await driver.estimatesEmptyTitle()).toBe("No estimates yet");
        expect(await driver.estimatesListedNames()).toEqual([]);
      });

      await test.step("the add action opens the creation dialog", async () => {
        expect(await driver.estimatesCreateDialogOpen()).toBe(false);
        await driver.estimatesEmptyAddClick();
        await expect.poll(() => driver.estimatesCreateDialogOpen(), { timeout: 20_000 }).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-003"], "unauthorized viewers get no working add action on estimates settings"),
  { tag: specTags(["EST-003"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 void ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf321void", session);
    await serverCompleteOnboarding(member.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.userId, role: 15 }], session);
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(true);
      expect(await driver.estimatesEmptyVisible()).toBe(false);
      expect(await driver.estimatesCreateDialogOpen()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await cleanupProvisionedMember(seed, member.userId, session);
    }
  }
);

test(
  specTitle(["EST-026"], "estimates settings lives at one URL with the project title and no system link"),
  { tag: specTags(["EST-026"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 addr ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectName = `${tag} project`;
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      projectName,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    const first = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} alpha`, ["1", "2"], session);
    const second = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} beta`, ["3", "5"], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);

      await test.step("the route renders the screen and the title tracks the project", async () => {
        expect(await driver.currentUrlPath()).toContain(`/settings/projects/${projectId}/estimates`);
        expect(await driver.estimatesHeadingVisible()).toBe(true);
        await expect.poll(() => driver.estimatesTabTitle(), { timeout: 20_000 }).toContain(projectName);
      });

      await test.step("a guessed per-system address isolates nothing", async () => {
        // Both systems exist server-side, so the guess names a real
        // system — yet the app has no per-system route to honor it.
        const names = (await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.name);
        expect(names).toContain(`${tag} alpha`);
        expect(names).toContain(`${tag} beta`);
        await driver.estimatesOpenSystemGuess(seed.workspaceSlug, projectId, first.id);
        expect(await driver.currentUrlPath()).toContain(first.id);
        expect(await driver.estimatesNotFoundVisible()).toBe(true);
        expect(await driver.estimatesHeadingVisible()).toBe(false);
        expect(await driver.estimatesListedNames()).toEqual([]);
        expect(second.id).not.toBe(first.id);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-027"], "estimates settings stays stale until a fresh fetch"),
  { tag: specTags(["EST-027"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 stale ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesEmptyVisible()).toBe(true);

      // Mutate behind the screen's back: the server holds the new system
      // while the open screen keeps showing the old state — no live push.
      const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} late`, ["1", "2"], session);
      await new Promise((resolve) => setTimeout(resolve, 3000));
      expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
        system.id
      );
      expect(await driver.estimatesEmptyVisible()).toBe(true);
      expect(await driver.estimatesListedNames()).toEqual([]);

      // A fresh fetch shows the latest state.
      await driver.reloadPage();
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesListedNames(), { timeout: 30_000 }).toContain(`${tag} late`);
      expect(await driver.estimatesEmptyVisible()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-028"], "estimates settings pins its absences"),
  { tag: specTags(["EST-028"]) },
  async ({ driver, seed }) => {
    const tag = `NF321 calm ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N321"),
      {},
      session
    );
    await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2", "3"], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);

      await test.step("the full surface renders to pin absences against", async () => {
        expect(await driver.estimatesHeadingVisible()).toBe(true);
        await expect.poll(() => driver.estimatesListedNames(), { timeout: 30_000 }).toContain(`${tag} prime`);
      });

      await test.step("no export, filter or bulk controls exist", async () => {
        expect(await driver.estimatesExportImportVisible()).toBe(false);
        expect(await driver.estimatesFilterSearchVisible()).toBe(false);
        expect(await driver.estimatesBulkVisible()).toBe(false);
      });

      await test.step("the screen text names none of the absent behaviors", async () => {
        const text = await driver.estimatesScreenText();
        expect(text).not.toMatch(/shortcut/i);
        expect(text).not.toMatch(/export/i);
        expect(text).not.toMatch(/import/i);
        expect(text).not.toMatch(/bulk/i);
        expect(text).not.toMatch(/filter/i);
        expect(text).not.toMatch(/search/i);
      });

      await test.step("keys that would drive shortcuts do nothing", async () => {
        const before = await driver.currentUrlPath();
        await driver.pressKey("?");
        await driver.pressKey("c");
        await driver.pressKey("/");
        expect(await driver.currentUrlPath()).toBe(before);
        expect(await driver.estimatesHeadingVisible()).toBe(true);
        expect(await driver.estimatesCreateDialogOpen()).toBe(false);
        await driver.pressKey("Escape");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

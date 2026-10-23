// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-app-first scenarios (NEWFRONT-606): project-settings AI Workers tab.
// Row PSET-064 (lost-runner rule, NEWFRONT-387). The old app has no such
// tab, so every leg skips the oracle driver. The save leg asserts the
// request contract rather than reload persistence: the API field lands
// with PDASHOSS01-297/304 and the backend drops it today (PATCH answers
// 200 without storing it), so persistence follows that tracker while the
// frontend intent is pinned here.
import { test, expect } from "../fixtures";
import {
  ROLE,
  serverAddProjectMembers,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

const ROW = ["PSET-064"];
const FAIL_TO_BACKLOG = "Fail the run and move the issue to Backlog";
const FALL_BACK = "Fall back to the pod";

/** Provision a workspace member with onboarding completed, for UI sign-in. */
async function provisionMember(
  seed: ParitySeedFacts,
  ownerSession: string,
  prefix: string
): Promise<{ userId: string; session: string; email: string; password: string }> {
  const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, prefix, ownerSession);
  await serverCompleteOnboarding(member.session);
  return member;
}

/**
 * Switch the signed-in UI user. Sign-in is a no-op while a session
 * lives, so the current user signs out first; the palette path is used
 * because settings pages render no account-menu sidebar.
 */
async function switchUser(driver: ParityDriver, email: string, password: string, workspaceSlug: string): Promise<void> {
  await driver.signOutViaCommandPalette();
  await driver.rulesEnsureSignedIn(email, password, workspaceSlug);
}

/** Workspace membership id for cleanup, resolved by provisioned user id. */
async function membershipIdFor(seed: ParitySeedFacts, ownerSession: string, userId: string): Promise<string> {
  const rows = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
  const row = rows.find((entry) => entry.userId === userId);
  if (!row) throw new Error("[parity] provisioned member missing from workspace list.");
  return row.membershipId;
}

test(
  specTitle(ROW, "admin opens the rule tab with the default preselected and saves the contract"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    test.skip(driver.target === "web", "new-app-first: the old app has no AI Workers tab.");
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

    await test.step("the stored rule preselects with both options and its explainer", async () => {
      await driver.aiWorkersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.aiWorkersRuleValue()).toBe(FAIL_TO_BACKLOG);
      expect(await driver.aiWorkersRuleOptions()).toEqual([FAIL_TO_BACKLOG, FALL_BACK]);
      expect(await driver.aiWorkersDescription()).toContain("Backlog");
      expect(await driver.aiWorkersRuleDisabled()).toBe(false);
      expect(await driver.aiWorkersFormDimmed()).toBe(false);
      expect(await driver.aiWorkersSaveVisible()).toBe(true);
    });

    await test.step("switching the rule swaps the explainer", async () => {
      await driver.aiWorkersSetRule(FALL_BACK);
      expect(await driver.aiWorkersRuleValue()).toBe(FALL_BACK);
      expect(await driver.aiWorkersDescription()).toContain("pod");
    });

    await test.step("saving writes the rule contract and confirms", async () => {
      const seen: Array<Record<string, unknown>> = [];
      await driver.page.route(`**/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/`, (route) => {
        if (route.request().method() === "PATCH") {
          try {
            seen.push(JSON.parse(route.request().postData() ?? "{}") as Record<string, unknown>);
          } catch {
            seen.push({});
          }
        }
        return route.continue();
      });
      try {
        await driver.aiWorkersSave();
        await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).not.toBeNull();
        const toast = await driver.rulesLastToast();
        expect(toast?.title).toBe("Success!");
        expect(seen).toHaveLength(1);
        expect(seen[0]?.["dedicated_runner_lost_action"]).toBe("fall_back");
        // The draft stays on the saved rule; reload persistence follows
        // the backend field (PDASHOSS01-297/304), so no reload leg here.
        expect(await driver.aiWorkersRuleValue()).toBe(FALL_BACK);
      } finally {
        await driver.page.unroute(`**/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/`);
      }
    });
  }
);

test(
  specTitle(ROW, "a refused save keeps the edits and explains"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    test.skip(driver.target === "web", "new-app-first: the old app has no AI Workers tab.");
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.aiWorkersOpen(seed.workspaceSlug, seed.projectId);
    await driver.aiWorkersSetRule(FALL_BACK);

    await driver.page.route(`**/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/`, (route) =>
      route.request().method() === "PATCH"
        ? route.fulfill({ status: 500, body: JSON.stringify({ detail: "boom" }) })
        : route.continue()
    );
    try {
      await driver.aiWorkersSave();
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).not.toBeNull();
      const toast = await driver.rulesLastToast();
      expect(toast?.title).toBe("Error!");
    } finally {
      await driver.page.unroute(`**/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/`);
    }

    // Nothing persisted and nothing reset: the draft still reads the pick.
    expect(await driver.aiWorkersRuleValue()).toBe(FALL_BACK);
    expect(await driver.aiWorkersRuleDisabled()).toBe(false);
    expect(await driver.aiWorkersSaveVisible()).toBe(true);
  }
);

test(
  specTitle(ROW, "members read the stored rule on an inert form"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    test.skip(driver.target === "web", "new-app-first: the old app has no AI Workers tab.");
    const session = await signInSession(seed.email, seed.password);
    const member = await provisionMember(seed, session, "nf606-aiworkers-member");
    await serverAddProjectMembers(
      seed.workspaceSlug,
      seed.projectId,
      [{ memberId: member.userId, role: ROLE.MEMBER }],
      session
    );
    try {
      await switchUser(driver, member.email, member.password, seed.workspaceSlug);
      await driver.aiWorkersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.aiWorkersRuleValue()).toBe(FAIL_TO_BACKLOG);
      expect(await driver.aiWorkersFormDimmed()).toBe(true);
      expect(await driver.aiWorkersRuleDisabled()).toBe(true);
      expect(await driver.aiWorkersSaveVisible()).toBe(false);
    } finally {
      await serverCleanupWorkspaceMember(
        seed.workspaceSlug,
        await membershipIdFor(seed, session, member.userId),
        session
      );
    }
  }
);

test(
  specTitle(ROW, "guests are refused and non-members get the join restriction"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    test.skip(driver.target === "web", "new-app-first: the old app has no AI Workers tab.");
    const session = await signInSession(seed.email, seed.password);
    if (!seed.guestEmail || !seed.guestPassword) {
      throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
    }

    await test.step("guests see the not-authorized view", async () => {
      await driver.rulesEnsureSignedIn(seed.guestEmail, seed.guestPassword, seed.workspaceSlug);
      await driver.aiWorkersOpen(seed.workspaceSlug, seed.projectId);
      expect(await driver.settingsShellDeniedVisible()).toBe(true);
    });

    await test.step("non-members get the restriction with its join action", async () => {
      const outsider = await provisionMember(seed, session, "nf606-aiworkers-outsider");
      try {
        await switchUser(driver, outsider.email, outsider.password, seed.workspaceSlug);
        await driver.aiWorkersOpen(seed.workspaceSlug, seed.projectId);
        expect(await driver.isProjectRestrictionVisible()).toBe(true);
        expect(await driver.restrictionJoinLabel()).toBe("Join project");
      } finally {
        await serverCleanupWorkspaceMember(
          seed.workspaceSlug,
          await membershipIdFor(seed, session, outsider.userId),
          session
        );
      }
    });
  }
);

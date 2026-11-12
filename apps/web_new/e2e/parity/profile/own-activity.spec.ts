// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-427): the settings activity tab pages the
// signed-in user's own history through a load-more control that shows only
// while further pages exist, and a member with no activity sees a themed
// empty illustration instead. Row: PROF-019.
//
// The paging scenario tops the seed history past one page with comments on
// a dedicated issue, keyed so reruns on one stack stay green; the empty
// scenario provisions a fresh member and cleans it up.
import { test, expect } from "../fixtures";
import {
  serverAddProjectMembers,
  serverCleanupWorkspaceMember,
  serverCreateComment,
  serverCreateIssue,
  serverOnboardSession,
  serverOwnActivities,
  serverProvisionWorkspaceMember,
  serverWorkspaceMembers,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["PROF-019"];

/** Entries the settings activity tab fetches per page. */
const PER_PAGE = 100;

test(
  specTitle(ROWS, "own activity pages through load-more until no pages remain"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // Slow by construction: topping past one page takes 100+ comment
    // creates plus fan-out lag plus a 100-editor render.
    test.setTimeout(600_000);
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const tag = `prof019-${Date.now().toString(36)}`;

    await test.step("top the own history past one page", async () => {
      const dedicated = await serverCreateIssue(
        seed.workspaceSlug,
        seed.projectId,
        session,
        `prof019 load-more ${tag}`
      );
      // Five past the page edge: a couple of entries may legitimately not
      // render (e.g. actor-update rows), so keep a buffer past 100. The
      // activity fan-out trails comment creation, so top up in rounds and
      // poll for each round to land rather than reading once.
      const target = PER_PAGE + 5;
      for (let round = 0; round < 3; round += 1) {
        const have = (await serverOwnActivities(session)).total_results;
        const needed = target - have;
        if (needed <= 0) break;
        // Small parallel batches: serial creates alone would eat the budget.
        for (let i = 0; i < needed; i += 5) {
          await Promise.all(
            Array.from({ length: Math.min(5, needed - i) }, (_, j) =>
              serverCreateComment(
                seed.workspaceSlug,
                seed.projectId,
                dedicated,
                session,
                `<p>prof019 paging ${tag} r${round} ${i + j}</p>`
              )
            )
          );
        }
        await expect
          .poll(async () => (await serverOwnActivities(session)).total_results, { timeout: 120_000 })
          .toBeGreaterThanOrEqual(target);
      }
      const after = await serverOwnActivities(session);
      expect(after.total_results).toBeGreaterThan(PER_PAGE);
      expect(after.total_pages).toBeGreaterThan(1);
    });

    await driver.settingsProfileOpenActivity();
    expect(await driver.settingsProfileOwnActivityEntryCount()).toBeGreaterThan(0);

    await test.step("load-more shows while pages remain, then clears", async () => {
      expect(await driver.settingsProfileOwnActivityLoadMoreVisible()).toBe(true);
      let pages = 0;
      for (;;) {
        const before = await driver.settingsProfileOwnActivityEntryCount();
        await driver.settingsProfileOwnActivityLoadMore();
        pages += 1;
        expect(await driver.settingsProfileOwnActivityEntryCount()).toBeGreaterThan(before);
        if (!(await driver.settingsProfileOwnActivityLoadMoreVisible())) break;
      }
      expect(pages).toBeGreaterThanOrEqual(1);
      // Exhausted: no load-more and no empty illustration either.
      expect(await driver.settingsProfileOwnActivityEntryCount()).toBeGreaterThan(PER_PAGE);
      expect(await driver.settingsProfileOwnActivityEmptyIllustration()).toBeNull();
    });
  }
);

test(
  specTitle(ROWS, "empty own activity shows a light/dark themed illustration"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const ownerSession = await signInSession(seed.email, seed.password);

    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "prof019empty", ownerSession);
    try {
      await serverAddProjectMembers(
        seed.workspaceSlug,
        seed.projectId,
        [{ memberId: provisioned.userId, role: 15 }],
        ownerSession
      );
      await serverOnboardSession(provisioned.session);
      expect((await serverOwnActivities(provisioned.session)).total_results).toBe(0);
      const cookies = sessionBrowserCookies(provisioned.session);

      await test.step("a quiet member sees the light illustration", async () => {
        await driver.openAuthenticated("/settings/profile/activity", cookies);
        await driver.settingsProfileOpenActivity();
        expect(await driver.settingsProfileOwnActivityEntryCount()).toBe(0);
        expect(await driver.settingsProfileOwnActivityLoadMoreVisible()).toBe(false);
        expect(await driver.settingsProfileOwnActivityEmptyIllustration()).toContain("activity-light");
      });

      await test.step("the illustration follows dark mode", async () => {
        // Picked through the UI: an out-of-band server PATCH would sit
        // behind the profile GET's HTTP cache for the running page.
        await driver.openAuthenticated("/settings/profile/preferences", cookies);
        await driver.settingsProfileOpenPreferences();
        await driver.settingsProfileSetTheme("dark");
        await driver.openAuthenticated("/settings/profile/activity", cookies);
        await driver.settingsProfileOpenActivity();
        expect(await driver.settingsProfileOwnActivityEmptyIllustration()).toContain("activity-dark");
      });
    } finally {
      const memberships = await serverWorkspaceMembers(seed.workspaceSlug, ownerSession);
      const membership = memberships.find((row) => row.userId === provisioned.userId);
      if (membership !== undefined) {
        await serverCleanupWorkspaceMember(seed.workspaceSlug, membership.membershipId, ownerSession);
      }
    }
  }
);

// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-201): flipping the property-change topic off
// stops its queued work-item emails while the inbox items still arrive,
// and flipping it back on resumes them. Row: NTF-025.
//
// Generation follows the notifications runtime pattern: unique-titled
// issues per fan-out with member edits, then poll the server for the
// unseen inbox rows, so reruns on one seeded stack stay green. Delivery
// is observed through the queued email-log rows the fan-out writes per
// topic preference — the same rows the mailer later sends.
//
// The comment and state topics do NOT gate independently: the fan-out's
// property_change catch-all re-enables their emails (bug NEWFRONT-215,
// pinned by the second scenario), so the property topic carries the
// row's disable/resume proof.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverEmailLogCount,
  serverEmailPreferences,
  serverNotificationsList,
  serverPatchIssue,
  serverUpdateEmailPreferences,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-025"];

test(
  specTitle(ROWS, "disabling property emails stops the queue while the inbox still arrives"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf025-${Date.now().toString(36)}`;
    const issueName = `Email gating ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);

    const inboxIds = async (): Promise<string[]> =>
      (await serverNotificationsList(seed.workspaceSlug, ownerSession))
        .filter((row) => row.entityIdentifier === issueId && !row.isMentioned)
        .map((row) => row.id);

    await test.step("with emails on, a property edit queues both inbox and email rows", async () => {
      await serverUpdateEmailPreferences(ownerSession, { property_change: true });
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, issueId, { priority: "high" }, memberSession);
      await expect.poll(inboxIds, { timeout: 90_000 }).toHaveLength(1);
      await expect.poll(() => serverEmailLogCount(seed.email, issueId), { timeout: 90_000 }).toBeGreaterThanOrEqual(1);
    });
    const baseline = await serverEmailLogCount(seed.email, issueId);

    await test.step("flipping the topic off stops the queue, not the inbox", async () => {
      await driver.notificationsOpenEmailPreferences();
      if ((await serverEmailPreferences(ownerSession)).property_change) {
        await driver.notificationsEmailPreferencesToggle("property_change");
        await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/updated successfully/i);
      }
      expect((await serverEmailPreferences(ownerSession)).property_change).toBe(false);
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, issueId, { priority: "low" }, memberSession);
      // The inbox row proves the fan-out ran; the queue must not grow after it.
      await expect.poll(inboxIds, { timeout: 90_000 }).toHaveLength(2);
      await new Promise((resolve) => setTimeout(resolve, 10_000));
      expect(await serverEmailLogCount(seed.email, issueId)).toBe(baseline);
    });

    await test.step("flipping the topic back on resumes the queue", async () => {
      await driver.notificationsEmailPreferencesToggle("property_change");
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/updated successfully/i);
      expect((await serverEmailPreferences(ownerSession)).property_change).toBe(true);
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, issueId, { priority: "urgent" }, memberSession);
      await expect.poll(inboxIds, { timeout: 90_000 }).toHaveLength(3);
      await expect.poll(() => serverEmailLogCount(seed.email, issueId), { timeout: 90_000 }).toBeGreaterThan(baseline);
    });
  }
);

// The fan-out's property_change catch-all re-enables comment emails even
// while the comment topic is off. This scenario pins that observed leak;
// the row records the intended independent gating with the linked issue.
test(
  specTitle(ROWS, "bug: NEWFRONT-215 comment emails still queue while comment is off"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf025b-${Date.now().toString(36)}`;
    const issueName = `Email leak ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, issueName);
    await serverCreateComment(seed.workspaceSlug, seed.projectId, issueId, ownerSession, `<p>subscribing ${tag}</p>`);

    await serverUpdateEmailPreferences(ownerSession, { comment: true, property_change: true });
    await driver.notificationsOpenEmailPreferences();
    await driver.notificationsEmailPreferencesToggle("comment");
    await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/updated successfully/i);
    const prefs = await serverEmailPreferences(ownerSession);
    expect(prefs.comment).toBe(false);
    expect(prefs.property_change).toBe(true);

    await serverCreateComment(
      seed.workspaceSlug,
      seed.projectId,
      issueId,
      memberSession,
      `<p>leaking comment ${tag}</p>`
    );
    await expect
      .poll(
        async () =>
          (await serverNotificationsList(seed.workspaceSlug, ownerSession)).filter(
            (row) => row.entityIdentifier === issueId && !row.isMentioned
          ).length,
        { timeout: 90_000 }
      )
      .toBe(1);
    // Intended: no queued email while the topic is off. Observed: it queues.
    await expect.poll(() => serverEmailLogCount(seed.email, issueId), { timeout: 90_000 }).toBeGreaterThanOrEqual(1);

    await test.step("restore the comment topic", async () => {
      await driver.notificationsEmailPreferencesToggle("comment");
      expect((await serverEmailPreferences(ownerSession)).comment).toBe(true);
    });
  }
);

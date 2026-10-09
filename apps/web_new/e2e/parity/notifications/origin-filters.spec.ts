// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-200): the inbox origin filter — a multi-select
// menu (assigned to me, created by me, subscribed by me) with checkmarks,
// applied-filter chips plus clear-all, reloading the stream from scratch on
// every change. Row: NTF-015.
//
// Generation follows the notifications runtime pattern: three issues whose
// notifications each match exactly one origin (owner-created, owner-assigned,
// owner-subscribed-only), fanned out with sequential member comments, then
// the assertions key on those titles, so reruns on one seeded stack stay
// green.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverCreateComment,
  serverCreateIssue,
  serverMe,
  serverNotificationsList,
  serverPatchIssue,
  signInSession,
  type NotificationsRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-015"];

async function fanOutForIssue(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  bodyHtml: string,
  ownerSession: string,
  memberSession: string
): Promise<NotificationsRow> {
  await serverCreateComment(workspaceSlug, projectId, issueId, memberSession, bodyHtml);
  let found: NotificationsRow | undefined;
  await expect
    .poll(
      async () => {
        const rows = await serverNotificationsList(workspaceSlug, ownerSession);
        found = rows.find((row) => row.entityIdentifier === issueId && !row.isMentioned);
        return found?.id ?? "";
      },
      { timeout: 90_000 }
    )
    .not.toBe("");
  return found!;
}

test(
  specTitle(ROWS, "origin filters narrow the stream with chips and reload from scratch"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf015-${Date.now().toString(36)}`;
    const createdName = `Filter created ${tag}`;
    const assignedName = `Filter assigned ${tag}`;
    const subscribedName = `Filter subscribed ${tag}`;

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);
    const owner = await serverMe(ownerSession);

    const createdId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, ownerSession, createdName);
    const assignedId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, memberSession, assignedName);
    const subscribedId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, memberSession, subscribedName);
    await test.step("fan out one notification per origin", async () => {
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        assignedId,
        { assignee_ids: [owner.id] },
        memberSession
      );
      for (const issueId of [createdId, assignedId, subscribedId]) {
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          issueId,
          ownerSession,
          `<p>subscribing ${tag}</p>`
        );
      }
      await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        createdId,
        `<p>created origin ${tag}</p>`,
        ownerSession,
        memberSession
      );
      await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        assignedId,
        `<p>assigned origin ${tag}</p>`,
        ownerSession,
        memberSession
      );
      await fanOutForIssue(
        seed.workspaceSlug,
        seed.projectId,
        subscribedId,
        `<p>subscribed origin ${tag}</p>`,
        ownerSession,
        memberSession
      );
    });

    await test.step("each notification matches exactly one server origin", async () => {
      const ours = [createdId, assignedId, subscribedId];
      const created = await serverNotificationsList(seed.workspaceSlug, ownerSession, { type: "created" });
      expect(created.map((row) => row.entityIdentifier)).toContain(createdId);
      expect(created.some((row) => row.entityIdentifier === assignedId)).toBe(false);
      expect(created.some((row) => row.entityIdentifier === subscribedId)).toBe(false);
      const assigned = await serverNotificationsList(seed.workspaceSlug, ownerSession, { type: "assigned" });
      expect(assigned.map((row) => row.entityIdentifier)).toContain(assignedId);
      expect(assigned.some((row) => row.entityIdentifier === createdId)).toBe(false);
      expect(assigned.some((row) => row.entityIdentifier === subscribedId)).toBe(false);
      const subscribed = await serverNotificationsList(seed.workspaceSlug, ownerSession, { type: "subscribed" });
      expect(subscribed.map((row) => row.entityIdentifier)).toContain(subscribedId);
      expect(subscribed.some((row) => row.entityIdentifier === createdId)).toBe(false);
      expect(subscribed.some((row) => row.entityIdentifier === assignedId)).toBe(false);
      expect(ours).toHaveLength(3);
    });

    const entry = await test.step("enter the inbox with all three listed", async () => {
      const query = await driver.notificationsEntryListQuery(seed.workspaceSlug);
      expect(query.type).toBeNull();
      expect(query.read).toBeNull();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(assignedName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).toContain(createdName);
      expect(titles).toContain(subscribedName);
      expect(await driver.notificationsAppliedChips()).toEqual([]);
      return query;
    });

    await test.step("the filter menu lists three unchecked origins", async () => {
      await driver.notificationsOpenFilterMenu();
      const options = await driver.notificationsFilterOptions();
      expect(options.map((option) => option.value)).toEqual(["assigned", "created", "subscribed"]);
      for (const option of options) {
        expect(option.checked).toBe(false);
        expect(option.label).not.toBe("");
      }
    });

    await test.step("one origin shows only its stream with a chip", async () => {
      const query = await driver.notificationsToggleFilterOrigin("assigned");
      expect(query.type).toBe("assigned");
      expect(query.cursor).toBe(entry.cursor);
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(assignedName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(createdName);
      expect(titles).not.toContain(subscribedName);
      const chips = await driver.notificationsAppliedChips();
      expect(chips.map((chip) => chip.origin)).toEqual(["assigned"]);
      expect(chips[0]!.label).not.toBe("");
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession, { type: "assigned" });
      expect(rows.map((row) => row.issueName)).toContain(assignedName);
      expect(rows.some((row) => row.issueName === createdName)).toBe(false);
      expect(rows.some((row) => row.issueName === subscribedName)).toBe(false);
    });

    await test.step("a second origin widens the stream", async () => {
      await driver.notificationsOpenFilterMenu();
      const before = await driver.notificationsFilterOptions();
      expect(before.find((option) => option.value === "assigned")?.checked).toBe(true);
      const query = await driver.notificationsToggleFilterOrigin("created");
      expect(query.type).toBe("assigned,created");
      expect(query.cursor).toBe(entry.cursor);
      await driver.notificationsCloseMenus();
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(createdName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).toContain(assignedName);
      expect(titles).not.toContain(subscribedName);
      const chips = await driver.notificationsAppliedChips();
      expect(chips.map((chip) => chip.origin).sort()).toEqual(["assigned", "created"]);
    });

    await test.step("a chip toggles its origin back off", async () => {
      const query = await driver.notificationsRemoveFilterChip("assigned");
      expect(query.type).toBe("created");
      expect(query.cursor).toBe(entry.cursor);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(createdName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).not.toContain(assignedName);
      expect(titles).not.toContain(subscribedName);
      const chips = await driver.notificationsAppliedChips();
      expect(chips.map((chip) => chip.origin)).toEqual(["created"]);
    });

    await test.step("clear-all resets every origin", async () => {
      const query = await driver.notificationsClearFilters();
      expect(query.type).toBeNull();
      expect(query.cursor).toBe(entry.cursor);
      expect(await driver.notificationsAppliedChips()).toEqual([]);
      await expect
        .poll(async () => (await driver.notificationsCards()).map((card) => card.title), {
          timeout: 30_000,
        })
        .toContain(subscribedName);
      const titles = (await driver.notificationsCards()).map((card) => card.title);
      expect(titles).toContain(createdName);
      expect(titles).toContain(assignedName);
      await driver.notificationsOpenFilterMenu();
      const options = await driver.notificationsFilterOptions();
      expect(options.every((option) => !option.checked)).toBe(true);
      await driver.notificationsCloseMenus();
      const rows = await serverNotificationsList(seed.workspaceSlug, ownerSession);
      expect(rows.map((row) => row.entityIdentifier)).toContain(createdId);
      expect(rows.map((row) => row.entityIdentifier)).toContain(assignedId);
      expect(rows.map((row) => row.entityIdentifier)).toContain(subscribedId);
    });
  }
);

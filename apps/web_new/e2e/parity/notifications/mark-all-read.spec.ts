// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-199): the mark-all-read control marks every
// notification in scope read at once — listed cards flip read, the current
// tab's counts reset — while showing progress and ignoring repeat presses
// mid-flight. Row: NTF-014.
//
// Isolation runs as a freshly provisioned user: mark-all-read rewrites
// shared inbox state, so the owner's stream is left for the other
// scenarios. Two plain fan-outs plus one mention prove the scope covers
// the stream; the request body proves the filter scope rode along. (The
// navigation count has no badge to reset — NEWFRONT-204 — so only the tab
// count and the server totals are asserted.)
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverAddProjectMembers,
  serverCleanupWorkspaceMember,
  serverCreateComment,
  serverCreateIssue,
  serverMe,
  serverMentionHtml,
  serverNotificationMarkUnread,
  serverNotificationsList,
  serverNotificationsUnread,
  serverOnboardSession,
  serverPatchIssue,
  serverProvisionWorkspaceMember,
  serverUnsubscribeIssue,
  serverWorkspaceMembers,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["NTF-014"];

test(
  specTitle(ROWS, "mark-all-read reads the scope at once with progress shown"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf014-${Date.now().toString(36)}`;
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "ntf014", ownerSession);
    try {
      await serverAddProjectMembers(
        seed.workspaceSlug,
        seed.projectId,
        [{ memberId: provisioned.userId, role: 15 }],
        ownerSession
      );
      await serverOnboardSession(provisioned.session);
      const fresh = await serverMe(provisioned.session);
      const cookies = sessionBrowserCookies(provisioned.session);

      const firstName = `Bulk read first ${tag}`;
      const secondName = `Bulk read second ${tag}`;
      const mentionName = `Bulk read mention ${tag}`;
      await test.step("fan two plain plus one mention out to the fresh user", async () => {
        for (const issueName of [firstName, secondName]) {
          const issueId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, provisioned.session, issueName);
          await serverCreateComment(
            seed.workspaceSlug,
            seed.projectId,
            issueId,
            provisioned.session,
            `<p>subscribing ${tag}</p>`
          );
          await serverCreateComment(
            seed.workspaceSlug,
            seed.projectId,
            issueId,
            memberSession,
            `<p>bulk check ${tag}</p>`
          );
        }
        const mentionId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, provisioned.session, mentionName);
        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          mentionId,
          memberSession,
          serverMentionHtml(provisioned.userId, fresh.displayName, `mention check ${tag}`)
        );
        await expect
          .poll(async () => serverNotificationsUnread(seed.workspaceSlug, provisioned.session), {
            timeout: 90_000,
          })
          .toEqual({ total: 2, mentions: 1 });
      });

      await test.step("double-pressing runs one scoped request with progress shown", async () => {
        await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
        await driver.notificationsOpenInbox(seed.workspaceSlug);
        // Both entry fetches must land first: badge (unread) and cards
        // (list), so the bulk action cannot race a stale entry response.
        await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe("2");
        await expect.poll(async () => (await driver.notificationsCards()).length, { timeout: 30_000 }).toBe(2);
        const held = await driver.notificationsMarkAllReadHeld(4_000);
        expect(held.progress).toBe(true);
        expect(held.requests).toBe(1);
        expect(held.scopeBody).not.toBeNull();
        const scope = JSON.parse(held.scopeBody!) as Record<string, unknown>;
        expect("snoozed" in scope).toBe(true);
        expect("archived" in scope).toBe(true);
      });

      await test.step("every listed card reads and the counts reset", async () => {
        const cards = await driver.notificationsCards();
        expect(cards.length).toBeGreaterThan(0);
        expect(cards.every((card) => !card.unread)).toBe(true);
        expect(await driver.notificationsTabBadge("all")).toBeNull();
        const unread = await serverNotificationsUnread(seed.workspaceSlug, provisioned.session);
        expect(unread).toEqual({ total: 0, mentions: 0 });
        const rows = await serverNotificationsList(seed.workspaceSlug, provisioned.session);
        expect(rows.length).toBeGreaterThan(0);
        expect(rows.every((row) => row.readAt !== null)).toBe(true);
        const mentions = await serverNotificationsList(seed.workspaceSlug, provisioned.session, {
          mentioned: true,
        });
        expect(mentions.every((row) => row.readAt !== null)).toBe(true);
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

// Bulk-read scope fidelity (NTF-032, bug NEWFRONT-665). The list accepts a
// comma-joined origin set (assigned, created, subscribed); the bulk endpoint
// has its own vocabulary (a lone assigned or created, and subscriptions
// called "watching") and ignores the read facet. A subscribed-only scope
// therefore matches no branch, and so does a two-origin one: both widen the
// read to every unread item in the workspace. This scenario pins that
// observed widening for the old app. The row records the intended narrow
// scope (the area spec §7a); the fix is a server-side contract change.
//
// A fresh member owns the whole stream, so every row here is theirs and the
// owner's inbox is untouched. Two staged rounds keep the assertions
// unambiguous: the first marks the subscribed-only scope read, the second
// re-arms one item per origin and marks a two-origin scope read. Fan-out is
// asynchronous, so every read here is scoped to the three fixture titles and
// waits for the rows to settle before pressing the control; extra rows from
// the same issues only ever make the widening more visible.
const SCOPE_ROWS = ["NTF-032"];

interface ScopeFixture {
  assignedId: string;
  createdId: string;
  subscribedId: string;
}

test(
  specTitle(SCOPE_ROWS, "bug: NEWFRONT-665 subscribed-only and two-origin scopes widen the bulk read"),
  { tag: specTags(SCOPE_ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);
    const tag = `ntf032-${Date.now().toString(36)}`;
    const ownerSession = await signInSession(seed.email, seed.password);
    const memberSession = await signInSession(member.email, member.password);

    const provisioned = await serverProvisionWorkspaceMember(seed.workspaceSlug, "ntf032", ownerSession);
    try {
      await serverAddProjectMembers(
        seed.workspaceSlug,
        seed.projectId,
        [{ memberId: provisioned.userId, role: 15 }],
        ownerSession
      );
      await serverOnboardSession(provisioned.session);
      const fresh = await serverMe(provisioned.session);
      const cookies = sessionBrowserCookies(provisioned.session);

      const names = {
        assigned: `Scope assigned ${tag}`,
        created: `Scope created ${tag}`,
        subscribed: `Scope subscribed ${tag}`,
      };
      // Unread state per fixture issue. The list returns every read state,
      // so a row that no filtered read reached stays visible here as unread.
      const unreadByIssue = async (): Promise<Map<string, boolean>> => {
        const rows = await serverNotificationsList(seed.workspaceSlug, provisioned.session);
        const unread = new Map<string, boolean>();
        for (const row of rows) {
          if (row.isMentioned) continue;
          const seen = unread.get(row.entityIdentifier) ?? false;
          unread.set(row.entityIdentifier, seen || row.readAt === null);
        }
        return unread;
      };

      let fixture: ScopeFixture;
      await test.step("stage one notification per origin for the fresh member", async () => {
        // Creator: the fresh member creates and subscribes to this item, so
        // the list reports it under "created" only.
        const createdId = await serverCreateIssue(
          seed.workspaceSlug,
          seed.projectId,
          provisioned.session,
          names.created
        );
        // Assignee: a colleague creates the item and assigns the fresh
        // member. The assignment itself fans out the one "assigned"
        // notification, so this issue gets no comment of its own: a second
        // fan-out would put two cards (same title) on one issue and break
        // the one-notification-per-origin fixture. Unsubscribing afterwards
        // keeps it out of "subscribed".
        const assignedId = await serverCreateIssue(seed.workspaceSlug, seed.projectId, memberSession, names.assigned);
        await serverPatchIssue(
          seed.workspaceSlug,
          seed.projectId,
          assignedId,
          { assignee_ids: [fresh.id] },
          memberSession
        );
        // Subscriber: the fresh member neither creates nor is assigned to
        // this item and only picks it up from its own comment.
        const subscribedId = await serverCreateIssue(
          seed.workspaceSlug,
          seed.projectId,
          memberSession,
          names.subscribed
        );

        await serverCreateComment(
          seed.workspaceSlug,
          seed.projectId,
          subscribedId,
          provisioned.session,
          `<p>subscribing ${tag}</p>`
        );
        for (const issueId of [createdId, subscribedId]) {
          await serverCreateComment(
            seed.workspaceSlug,
            seed.projectId,
            issueId,
            memberSession,
            `<p>scope check ${tag}</p>`
          );
        }
        // Wait for the assignment fan-out to create its notification before
        // unsubscribing: dropping the subscriber row earlier would let the
        // async worker skip the "assigned" item entirely.
        await expect
          .poll(
            async () => {
              const unread = await unreadByIssue();
              return [unread.get(createdId), unread.get(assignedId), unread.get(subscribedId)];
            },
            { timeout: 90_000 }
          )
          .toEqual([true, true, true]);

        await serverUnsubscribeIssue(seed.workspaceSlug, seed.projectId, assignedId, provisioned.session);

        const created = await serverNotificationsList(seed.workspaceSlug, provisioned.session, { type: "created" });
        expect(created.map((row) => row.entityIdentifier)).toContain(createdId);
        expect(created.some((row) => row.entityIdentifier === assignedId)).toBe(false);
        const assigned = await serverNotificationsList(seed.workspaceSlug, provisioned.session, { type: "assigned" });
        expect(assigned.map((row) => row.entityIdentifier)).toContain(assignedId);
        expect(assigned.some((row) => row.entityIdentifier === createdId)).toBe(false);
        // The unsubscribe must land before the origins are read: until it
        // does, the assigned item also sits under "subscribed".
        await expect
          .poll(
            async () => {
              const subscribed = await serverNotificationsList(seed.workspaceSlug, provisioned.session, {
                type: "subscribed",
              });
              const ids = subscribed.map((row) => row.entityIdentifier);
              return [ids.includes(subscribedId), ids.includes(createdId), ids.includes(assignedId)];
            },
            { timeout: 90_000 }
          )
          .toEqual([true, false, false]);

        fixture = { assignedId, createdId, subscribedId };
      });

      await test.step("a subscribed-only scope reads everything, not just the subscribed stream", async () => {
        await driver.notificationsProjectNavBadge(seed.workspaceSlug, seed.projectId, cookies);
        await driver.notificationsOpenInbox(seed.workspaceSlug);
        await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe("3");
        await expect
          .poll(
            async () =>
              (await driver.notificationsCards())
                .map((card) => card.title)
                .filter((title) => Object.values(names).includes(title))
                .sort(),
            { timeout: 30_000 }
          )
          .toEqual(Object.values(names).sort());

        await driver.notificationsOpenFilterMenu();
        const query = await driver.notificationsToggleFilterOrigin("subscribed");
        expect(query.type).toBe("subscribed");
        await driver.notificationsCloseMenus();
        await expect
          .poll(async () => (await driver.notificationsCards()).map((card) => card.title), { timeout: 30_000 })
          .toEqual([names.subscribed]);

        const held = await driver.notificationsMarkAllReadHeld(2_000);
        expect(held.requests).toBe(1);
        const scope = JSON.parse(held.scopeBody ?? "{}") as Record<string, unknown>;
        expect(scope["type"]).toBe("subscribed");

        // Intended: the other two origins stay unread. Observed: the bulk
        // endpoint matches no branch for "subscribed" and reads them too.
        const unread = await unreadByIssue();
        expect(unread.get(fixture.subscribedId)).toBe(false);
        expect(unread.get(fixture.assignedId)).toBe(false);
        expect(unread.get(fixture.createdId)).toBe(false);
        expect(await driver.notificationsTabBadge("all")).toBeNull();
      });

      await test.step("a two-origin scope reads everything, not just the union", async () => {
        // Re-arm all three: the subscribed-only item must be unread when
        // the two-origin scope lands, otherwise a widening cannot show.
        for (const issueId of [fixture.assignedId, fixture.createdId, fixture.subscribedId]) {
          const rows = await serverNotificationsList(seed.workspaceSlug, provisioned.session);
          const row = rows.find((entry) => entry.entityIdentifier === issueId && !entry.isMentioned);
          expect(row).toBeDefined();
          await serverNotificationMarkUnread(seed.workspaceSlug, row!.id, provisioned.session);
        }
        await driver.notificationsOpenInbox(seed.workspaceSlug);
        await expect.poll(() => driver.notificationsTabBadge("all"), { timeout: 30_000 }).toBe("3");
        await expect
          .poll(
            async () =>
              (await driver.notificationsCards())
                .map((card) => card.title)
                .filter((title) => Object.values(names).includes(title))
                .sort(),
            { timeout: 30_000 }
          )
          .toEqual(Object.values(names).sort());

        await driver.notificationsOpenFilterMenu();
        const first = await driver.notificationsToggleFilterOrigin("assigned");
        expect(first.type).toBe("assigned");
        await driver.notificationsCloseMenus();
        await expect
          .poll(async () => (await driver.notificationsCards()).map((card) => card.title), { timeout: 30_000 })
          .toEqual([names.assigned]);

        await driver.notificationsOpenFilterMenu();
        const joined = await driver.notificationsToggleFilterOrigin("created");
        expect(joined.type).toBe("assigned,created");
        await driver.notificationsCloseMenus();
        await expect
          .poll(
            async () => (await driver.notificationsCards()).map((card) => card.title).then((titles) => titles.sort()),
            {
              timeout: 30_000,
            }
          )
          .toEqual([names.assigned, names.created].sort());

        const held = await driver.notificationsMarkAllReadHeld(2_000);
        expect(held.requests).toBe(1);
        const scope = JSON.parse(held.scopeBody ?? "{}") as Record<string, unknown>;
        expect(scope["type"]).toBe("assigned,created");

        // Intended: the subscribed-only item stays unread. Observed: the
        // joined value matches no branch and the bulk read widens.
        const unread = await unreadByIssue();
        expect(unread.get(fixture.assignedId)).toBe(false);
        expect(unread.get(fixture.createdId)).toBe(false);
        expect(unread.get(fixture.subscribedId)).toBe(false);
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

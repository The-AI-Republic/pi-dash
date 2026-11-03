// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: notification inbox (list, read/unread, archive/unarchive,
// snooze, mark-all-read) and email delivery preferences against live
// Django, seeded with contract/seed-contracts.py.
import { describe, expect, it } from "vitest";
import {
  archiveNotification,
  getNotificationPreferences,
  getUnreadNotifications,
  listNotifications,
  listNotificationsPage,
  markAllNotificationsRead,
  markNotificationRead,
  markNotificationUnread,
  snoozeNotification,
  unarchiveNotification,
  updateNotificationPreferences,
} from "../contracts/notifications.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_TITLE = "Contract notification probe";

async function probeId(client: ApiClient): Promise<string> {
  const rows = await listNotifications(client, CONTRACT_WORKSPACE, { snoozed: false, archived: false });
  const probe = rows.find((row) => row.title === PROBE_TITLE);
  expect(probe).toBeDefined();
  return probe?.id ?? "";
}

/**
 * Idempotent repair: a dead run may have left the probe read, archived
 * or snoozed (the seed repairs it too, but suites can run without a
 * fresh re-seed). Every restore is a no-op on a pristine row.
 */
async function ensureProbePristine(client: ApiClient, id: string): Promise<void> {
  await markNotificationUnread(client, CONTRACT_WORKSPACE, id);
  await unarchiveNotification(client, CONTRACT_WORKSPACE, id);
  await snoozeNotification(client, CONTRACT_WORKSPACE, id, null);
}

describe.skipIf(!contractEnabled)("notifications contract", () => {
  it("answers total and mention unread counts", async () => {
    const client = await sharedSignedIn();
    const unread = await getUnreadNotifications(client, CONTRACT_WORKSPACE);
    expect(unread.total_unread_notifications_count).toBeGreaterThanOrEqual(0);
    expect(unread.mention_unread_notifications_count).toBeGreaterThanOrEqual(0);
  });

  it("lists the default stream, the mentions tab and unread-only mode", async () => {
    const client = await sharedSignedIn();
    const id = await probeId(client);
    await ensureProbePristine(client, id);

    const rows = await listNotifications(client, CONTRACT_WORKSPACE, { snoozed: false, archived: false });
    const probe = rows.find((row) => row.id === id);
    expect(probe).toBeDefined();
    expect(probe?.read_at).toBeNull();
    expect(probe?.archived_at).toBeNull();
    expect(probe?.snoozed_till).toBeNull();
    expect(probe?.entity_name).toBe("issue");
    expect(probe?.triggered_by_details).not.toBeNull();
    expect(probe?.is_mentioned_notification).toBe(false);

    const page = await listNotificationsPage(client, CONTRACT_WORKSPACE, {
      snoozed: false,
      archived: false,
      perPage: 300,
      cursor: "300:0:0",
    });
    expect(page.total_count).toBeGreaterThanOrEqual(1);
    expect(page.results.some((row) => row.id === id)).toBe(true);
    expect(page.next_page_results).toBe(false);

    const mentions = await listNotifications(client, CONTRACT_WORKSPACE, {
      snoozed: false,
      archived: false,
      mentioned: true,
    });
    expect(mentions.some((row) => row.id === id)).toBe(false);
    expect(mentions.every((row) => row.is_mentioned_notification)).toBe(true);

    const unreadOnly = await listNotifications(client, CONTRACT_WORKSPACE, {
      snoozed: false,
      archived: false,
      read: false,
    });
    expect(unreadOnly.some((row) => row.id === id)).toBe(true);
  });

  it("counts the probe in the unread totals", async () => {
    const client = await sharedSignedIn();
    const id = await probeId(client);
    await ensureProbePristine(client, id);
    const unread = await getUnreadNotifications(client, CONTRACT_WORKSPACE);
    expect(unread.total_unread_notifications_count).toBeGreaterThanOrEqual(1);
  });

  it("marks one card read and unread again", async () => {
    const client = await sharedSignedIn();
    const id = await probeId(client);
    await ensureProbePristine(client, id);

    const read = await markNotificationRead(client, CONTRACT_WORKSPACE, id);
    expect(read.id).toBe(id);
    expect(read.read_at).not.toBeNull();
    // Mutation echoes carry no stream annotations (absent, not false).
    expect(read.is_mentioned_notification).toBeUndefined();

    const readOnly = await listNotifications(client, CONTRACT_WORKSPACE, {
      snoozed: false,
      archived: false,
      read: false,
    });
    expect(readOnly.some((row) => row.id === id)).toBe(false);

    const unread = await markNotificationUnread(client, CONTRACT_WORKSPACE, id);
    expect(unread.read_at).toBeNull();
  });

  it("archives one card and returns it to the stream", async () => {
    const client = await sharedSignedIn();
    const id = await probeId(client);
    await ensureProbePristine(client, id);

    const archived = await archiveNotification(client, CONTRACT_WORKSPACE, id);
    expect(archived.archived_at).not.toBeNull();

    const stream = await listNotifications(client, CONTRACT_WORKSPACE, { snoozed: false, archived: false });
    expect(stream.some((row) => row.id === id)).toBe(false);
    const archivedMode = await listNotifications(client, CONTRACT_WORKSPACE, {
      snoozed: false,
      archived: true,
    });
    expect(archivedMode.some((row) => row.id === id)).toBe(true);

    const restored = await unarchiveNotification(client, CONTRACT_WORKSPACE, id);
    expect(restored.archived_at).toBeNull();
  });

  it("snoozes to a resume moment and clears it again", async () => {
    const client = await sharedSignedIn();
    const id = await probeId(client);
    await ensureProbePristine(client, id);

    const resume = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString();
    const snoozed = await snoozeNotification(client, CONTRACT_WORKSPACE, id, resume);
    expect(snoozed.snoozed_till).not.toBeNull();

    const snoozedMode = await listNotifications(client, CONTRACT_WORKSPACE, {
      snoozed: true,
      archived: false,
    });
    expect(snoozedMode.some((row) => row.id === id)).toBe(true);

    const cleared = await snoozeNotification(client, CONTRACT_WORKSPACE, id, null);
    expect(cleared.snoozed_till).toBeNull();
    const snoozedAfter = await listNotifications(client, CONTRACT_WORKSPACE, {
      snoozed: true,
      archived: false,
    });
    expect(snoozedAfter.some((row) => row.id === id)).toBe(false);
  });

  it("marks every notification in scope read at once", async () => {
    const client = await sharedSignedIn();
    const id = await probeId(client);
    await ensureProbePristine(client, id);

    const result = await markAllNotificationsRead(client, CONTRACT_WORKSPACE, {
      snoozed: false,
      archived: false,
    });
    expect(result.message).toBe("Successful");

    const unreadOnly = await listNotifications(client, CONTRACT_WORKSPACE, {
      snoozed: false,
      archived: false,
      read: false,
    });
    expect(unreadOnly.some((row) => row.id === id)).toBe(false);

    await markNotificationUnread(client, CONTRACT_WORKSPACE, id);
  });

  it("reads preferences and flips one toggle at a time", async () => {
    const client = await sharedSignedIn();
    const loaded = await getNotificationPreferences(client);
    expect(loaded.property_change).toBe(true);
    expect(loaded.state_change).toBe(true);
    expect(loaded.comment).toBe(true);
    expect(loaded.mention).toBe(true);
    expect(loaded.issue_completed).toBe(true);

    const updated = await updateNotificationPreferences(client, { mention: false });
    expect(updated.mention).toBe(false);
    expect(updated.property_change).toBe(true);
    expect(updated.state_change).toBe(true);
    expect(updated.comment).toBe(true);
    expect(updated.issue_completed).toBe(true);

    const restored = await updateNotificationPreferences(client, { mention: true });
    expect(restored.mention).toBe(true);
  });
});

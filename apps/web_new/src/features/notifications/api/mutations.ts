// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Card writes (written from parity/specs/notifications.md §9, NTF-018-021).
// Each toggle moves the card and the totals at once, then waits for the
// server's echo. A failure restores every touched cache and stays silent —
// no success toast, no changed state — while a success reports through a
// toast. Stream membership follows the write: a card leaves a stream whose
// flags exclude its new state and joins the matching one. Toggling a
// failed write again therefore retries cleanly.
import { useMutation, useQueryClient, type InfiniteData, type QueryClient, type QueryKey } from "@tanstack/react-query";
import {
  archiveNotification,
  markNotificationRead,
  markNotificationUnread,
  snoozeNotification,
  unarchiveNotification,
  type ApiClient,
  type NotificationItem,
  type NotificationListQuery,
  type NotificationListResponse,
  type UnreadNotifications,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifySuccess } from "../../../shared/shell/index.js";
import { notificationKeys } from "./keys.js";
import { notificationListQueryFromKey } from "./streamQuery.js";

export const MARKED_READ_TITLE = "Notification marked as read";
export const MARKED_UNREAD_TITLE = "Notification marked as unread";
export const MARKED_ARCHIVED_TITLE = "Notification marked as archived";
export const UNARCHIVED_TITLE = "Notification unarchived";
export const SNOOZED_TITLE = "Notification snoozed";
export const UNSNOOZED_TITLE = "Notification unsnoozed";

/** One read toggle: the card and the state it should land in. */
export interface ReadToggle {
  id: string;
  unread: boolean;
  flags: StreamFlags;
}

/** One archive toggle. */
export interface ArchiveToggle {
  id: string;
  archived: boolean;
  flags: StreamFlags;
}

/** One snooze write; a null resume moment clears an existing snooze. */
export interface SnoozeWrite {
  id: string;
  /** Resume instant as an ISO string, or null to unsnooze. */
  snoozedTill: string | null;
  flags: StreamFlags;
}

/** Stream annotations carried over from the listed row. */
export type StreamFlags = Pick<NotificationItem, "is_inbox_issue" | "is_intake_issue" | "is_mentioned_notification">;

/** Put the listed row's stream annotations back on a rewritten row. */
function withStreamFlags(row: NotificationItem, flags: StreamFlags): NotificationItem {
  return { ...row, ...flags };
}

/** What a cached stream page answers to. */
type StreamPage = NotificationListResponse | undefined;

/** A cached stream: either the first page alone or the cursor pages. */
export type StreamData = NotificationListResponse | InfiniteData<NotificationListResponse, string>;

function isPage(value: unknown): value is NotificationListResponse {
  return typeof value === "object" && value !== null && Array.isArray((value as { results?: unknown }).results);
}

/** Every page a cached stream holds, oldest page last. */
export function streamPages(data: StreamData | undefined): NotificationListResponse[] {
  if (data === undefined) return [];
  if (isPage(data)) return [data];
  return data.pages;
}

/**
 * Write a row into a cached stream. The card keeps its place on whichever
 * page already lists it — earlier pages are walked first, so the cursor
 * pages never duplicate a card loaded from a later page (spec §6). A card
 * that joins the stream lands on its first page (reads walk from the
 * newest row); a card that leaves is dropped from whichever page holds it.
 */
export function writeRowToStream(
  data: StreamData | undefined,
  query: NotificationListQuery,
  row: NotificationItem
): StreamData | undefined {
  if (data === undefined) return undefined;
  if (isPage(data)) return applyRowToPage(data, query, row) ?? data;
  const listedOn = data.pages.findIndex((page) => page.results.some((entry) => entry.id === row.id));
  const target = listedOn === -1 ? 0 : listedOn;
  return {
    ...data,
    pages: data.pages.map((page, index) =>
      index === target ? (applyRowToPage(page, query, row) ?? page) : withoutRow(page, row.id)
    ),
  };
}

/** One cursor page with a card dropped, or the same object when absent. */
function withoutRow(page: NotificationListResponse, id: string): NotificationListResponse {
  return page.results.some((entry) => entry.id === id)
    ? { ...page, results: page.results.filter((entry) => entry.id !== id) }
    : page;
}

/**
 * Whether a card belongs in a stream: archived mode holds only archived
 * cards, snoozed mode only snoozed cards, the default stream excludes
 * both, and a stream carrying neither flag lists everything.
 */
export function rowBelongsToStream(row: NotificationItem, query: NotificationListQuery): boolean {
  if (query.archived === true) return row.archived_at !== null;
  if (query.snoozed === true) return row.snoozed_till !== null;
  if (query.archived === false) return row.archived_at === null && row.snoozed_till === null;
  return true;
}

/**
 * Write a row into a stream page. The row keeps its place when it still
 * belongs to the stream, joins the top when the stream now includes it,
 * and leaves otherwise.
 */
export function applyRowToPage(page: StreamPage, query: NotificationListQuery, row: NotificationItem): StreamPage {
  if (page === undefined) return undefined;
  const present = page.results.some((entry) => entry.id === row.id);
  const keep = rowBelongsToStream(row, query);
  if (!keep && !present) return page;
  if (keep && present) {
    return { ...page, results: page.results.map((entry) => (entry.id === row.id ? row : entry)) };
  }
  const retained = page.results.filter((entry) => entry.id !== row.id);
  return { ...page, results: keep ? [row, ...retained] : retained };
}

/**
 * A row as it should look after the write: the server echo keeps the
 * stream annotations the mutation endpoint drops (it serializes the bare
 * instance), so the cached row keeps its own flags.
 */
export function mergeWrittenRow(previous: NotificationItem, written: NotificationItem): NotificationItem {
  return {
    ...written,
    ...(previous.is_inbox_issue === undefined ? {} : { is_inbox_issue: previous.is_inbox_issue }),
    ...(previous.is_intake_issue === undefined ? {} : { is_intake_issue: previous.is_intake_issue }),
    is_mentioned_notification: previous.is_mentioned_notification ?? written.is_mentioned_notification,
  };
}

function unreadDelta(previous: NotificationItem, next: NotificationItem): number {
  if (previous.read_at === null && next.read_at !== null) return -1;
  if (previous.read_at !== null && next.read_at === null) return 1;
  return 0;
}

/**
 * Move one card through every cached stream of the workspace: the row
 * updates in place or leaves the stream, and the unread totals shift by
 * the same amount the read flag changed. Returns the snapshot a failure
 * restores.
 */
function applyCardWrite(
  client: QueryClient,
  workspaceSlug: string,
  id: string,
  flags: StreamFlags,
  quote: (previous: NotificationItem) => NotificationItem
): { snapshots: Array<{ queryKey: QueryKey; data: unknown }> } {
  const unreadKey = notificationKeys.unread(workspaceSlug);
  const isUnread = (key: QueryKey): boolean =>
    key.length === unreadKey.length && unreadKey.every((part, index) => key[index] === part);
  const cached = client.getQueryCache().findAll({ queryKey: notificationKeys.all(workspaceSlug) });
  const unreadEntries = cached.filter((query) => isUnread(query.queryKey));
  const listEntries = cached.flatMap((query) => {
    const streamQuery = notificationListQueryFromKey(workspaceSlug, query.queryKey);
    return streamQuery === null ? [] : [{ query, streamQuery }];
  });
  const snapshots = [
    ...unreadEntries.map((query) => ({ queryKey: query.queryKey, data: query.state.data })),
    ...listEntries.map(({ query }) => ({ queryKey: query.queryKey, data: query.state.data })),
  ];
  for (const query of unreadEntries) {
    const totals = query.state.data as UnreadNotifications | undefined;
    // Quote the row from the first stream that lists it, in cache order,
    // so the delta is computed once (archived and snoozed views can both
    // hold the same card).
    const source = listEntries
      .flatMap(({ query: listed }) => streamPages(listed.state.data as StreamData | undefined))
      .flatMap((page) => page.results)
      .find((entry) => entry.id === id);
    if (totals === undefined || source === undefined) continue;
    const delta = unreadDelta(source, quote(withStreamFlags(source, flags)));
    if (delta !== 0) {
      client.setQueryData<UnreadNotifications>(query.queryKey, {
        ...totals,
        total_unread_notifications_count: Math.max(0, totals.total_unread_notifications_count + delta),
      });
    }
  }
  for (const { query, streamQuery } of listEntries) {
    const row = streamPages(query.state.data as StreamData | undefined)
      .flatMap((page) => page.results)
      .find((entry) => entry.id === id);
    if (row === undefined) continue;
    const written = quote(withStreamFlags(row, flags));
    client.setQueryData(query.queryKey, (current: StreamData | undefined) =>
      writeRowToStream(current, streamQuery, written)
    );
  }
  return { snapshots };
}

function restore(client: QueryClient, snapshots: Array<{ queryKey: QueryKey; data: unknown }>): void {
  for (const snapshot of snapshots) {
    client.setQueryData(snapshot.queryKey, snapshot.data);
  }
}

async function settleWrite(
  client: QueryClient,
  workspaceSlug: string,
  written: NotificationItem,
  snapshots: Array<{ queryKey: QueryKey; data: unknown }>,
  flags: StreamFlags
): Promise<void> {
  for (const query of client.getQueryCache().findAll({ queryKey: notificationKeys.all(workspaceSlug) })) {
    const streamQuery = notificationListQueryFromKey(workspaceSlug, query.queryKey);
    if (streamQuery === null) continue;
    // The optimistic pass already placed the card where it now belongs;
    // settle only swaps the server's row in for the optimistic one,
    // keeping the annotations the mutation echo drops. The card's own
    // flags came from the listed row before the write.
    const listed = snapshots
      .filter((snapshot) => notificationListQueryFromKey(workspaceSlug, snapshot.queryKey) !== null)
      .flatMap((snapshot) => streamPages(snapshot.data as StreamData | undefined))
      .flatMap((page) => page.results)
      .find((entry) => entry.id === written.id);
    if (listed === undefined) continue;
    const fresh = mergeWrittenRow(withStreamFlags(listed, flags), written);
    client.setQueryData(query.queryKey, (current: StreamData | undefined) =>
      writeRowToStream(current, streamQuery, fresh)
    );
  }
}

function optimisticRow(previous: NotificationItem, patch: Partial<NotificationItem>): NotificationItem {
  return { ...previous, ...patch };
}

/** Flip one card's read state. */
export function useToggleNotificationRead(workspaceSlug: string) {
  const client = useQueryClient();
  return useMutation<NotificationItem, Error, ReadToggle>({
    mutationFn: async (toggle) => {
      const now = new Date().toISOString();
      const { snapshots } = applyCardWrite(client, workspaceSlug, toggle.id, toggle.flags, (previous) =>
        optimisticRow(previous, { read_at: toggle.unread ? now : null })
      );
      let written: NotificationItem;
      try {
        const api: ApiClient = getAppClient();
        written = toggle.unread
          ? await markNotificationRead(api, workspaceSlug, toggle.id)
          : await markNotificationUnread(api, workspaceSlug, toggle.id);
      } catch (error) {
        restore(client, snapshots);
        throw error;
      }
      await settleWrite(client, workspaceSlug, written, snapshots, toggle.flags);
      notifySuccess(written.read_at !== null ? MARKED_READ_TITLE : MARKED_UNREAD_TITLE);
      return written;
    },
  });
}

/** Archive or return one card. */
export function useToggleNotificationArchive(workspaceSlug: string) {
  const client = useQueryClient();
  return useMutation<NotificationItem, Error, ArchiveToggle>({
    mutationFn: async (toggle) => {
      const now = new Date().toISOString();
      const { snapshots } = applyCardWrite(client, workspaceSlug, toggle.id, toggle.flags, (previous) =>
        optimisticRow(previous, { archived_at: toggle.archived ? now : null })
      );
      let written: NotificationItem;
      try {
        const api: ApiClient = getAppClient();
        written = toggle.archived
          ? await archiveNotification(api, workspaceSlug, toggle.id)
          : await unarchiveNotification(api, workspaceSlug, toggle.id);
      } catch (error) {
        restore(client, snapshots);
        throw error;
      }
      await settleWrite(client, workspaceSlug, written, snapshots, toggle.flags);
      notifySuccess(written.archived_at !== null ? MARKED_ARCHIVED_TITLE : UNARCHIVED_TITLE);
      return written;
    },
  });
}

/** Snooze one card to a resume instant, or clear its snooze with null. */
export function useSnoozeNotification(workspaceSlug: string) {
  const client = useQueryClient();
  return useMutation<NotificationItem, Error, SnoozeWrite>({
    mutationFn: async (write) => {
      const { snapshots } = applyCardWrite(client, workspaceSlug, write.id, write.flags, (previous) =>
        optimisticRow(previous, { snoozed_till: write.snoozedTill })
      );
      let written: NotificationItem;
      try {
        written = await snoozeNotification(getAppClient(), workspaceSlug, write.id, write.snoozedTill);
      } catch (error) {
        restore(client, snapshots);
        throw error;
      }
      await settleWrite(client, workspaceSlug, written, snapshots, write.flags);
      notifySuccess(written.snoozed_till === null ? UNSNOOZED_TITLE : SNOOZED_TITLE);
      return written;
    },
  });
}

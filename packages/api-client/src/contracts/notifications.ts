// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notification contracts: the inbox list with its stream flags, the
// per-item read/archive toggles, snooze, the bulk mark-all-read scope and
// the email delivery preferences. The unread totals behind the shell badge
// live here too (thin shell slice; full ownership in notifications area).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { UserLite } from "./users.js";

export const UnreadNotifications = z.object({
  total_unread_notifications_count: z.number(),
  mention_unread_notifications_count: z.number(),
});
export type UnreadNotifications = z.infer<typeof UnreadNotifications>;

export async function getUnreadNotifications(client: ApiClient, slug: string): Promise<UnreadNotifications> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/users/notifications/unread/`);
  return client.parse(UnreadNotifications, body, `GET /api/workspaces/${slug}/users/notifications/unread/`);
}

/**
 * One inbox row. `data` and `message` stay opaque: the backend stores
 * whatever JSON the triggering activity produced (a dict for activity
 * rows, a plain string for mention rows), and the slices narrow it.
 * `triggered_by_details` is the nested actor projection (null when the
 * actor is gone); `triggered_by` is the same actor as a bare key.
 */
export const NotificationItem = z.object({
  id: uuid(),
  workspace: uuid(),
  project: nullableUuid(),
  data: z.nullable(z.unknown()),
  entity_identifier: nullableUuid(),
  entity_name: z.string(),
  title: z.string(),
  message: z.nullable(z.unknown()),
  message_html: z.string(),
  message_stripped: z.nullable(z.string()),
  sender: z.string(),
  triggered_by: nullableUuid(),
  triggered_by_details: z.nullable(UserLite),
  receiver: uuid(),
  read_at: nullableDateTime(),
  snoozed_till: nullableDateTime(),
  archived_at: nullableDateTime(),
  // Stream flags: real booleans on the annotated list rows, absent on
  // the per-item mutation echoes (those serialize the bare instance
  // without the list queryset's annotations, and DRF drops the
  // unresolvable flags). Slices treat null/undefined as false.
  is_inbox_issue: z.optional(z.nullable(z.boolean())),
  is_intake_issue: z.optional(z.nullable(z.boolean())),
  is_mentioned_notification: z.optional(z.nullable(z.boolean())),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type NotificationItem = z.infer<typeof NotificationItem>;

export const NotificationList = z.array(NotificationItem);
export type NotificationList = z.infer<typeof NotificationList>;

/** Cursor-paginated envelope for the inbox list (same shape as issues). */
export const NotificationListResponse = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_results: z.number(),
  results: NotificationList,
});
export type NotificationListResponse = z.infer<typeof NotificationListResponse>;

/**
 * Inbox list query. Flag encoding mirrors the backend: `snoozed`,
 * `archived` and `read` travel as "true"/"false" strings, while
 * `mentioned` must be omitted when false — the view truthiness-checks
 * the raw string, so even "false" would select the mentions stream.
 * `type` is the comma-joined origin filter (assigned, created,
 * subscribed), absent when no origin is selected. The default stream is
 * `snoozed: false, archived: false` with no type, read or mentioned.
 */
export interface NotificationListQuery {
  type?: string;
  snoozed?: boolean;
  archived?: boolean;
  read?: boolean;
  mentioned?: boolean;
}

function notificationListParams(query: NotificationListQuery): Record<string, string> {
  return {
    ...(query.type === undefined ? {} : { type: query.type }),
    ...(query.snoozed === undefined ? {} : { snoozed: query.snoozed ? "true" : "false" }),
    ...(query.archived === undefined ? {} : { archived: query.archived ? "true" : "false" }),
    ...(query.read === undefined ? {} : { read: query.read ? "true" : "false" }),
    ...(query.mentioned === true ? { mentioned: "true" } : {}),
  };
}

/**
 * Live inbox list. GET /api/workspaces/<slug>/users/notifications (no
 * trailing slash). Without page parameters the view answers the plain
 * row array.
 */
export async function listNotifications(
  client: ApiClient,
  slug: string,
  query: NotificationListQuery = {}
): Promise<NotificationList> {
  const path = `/api/workspaces/${slug}/users/notifications`;
  const body = await client.getJson<unknown>(path, { query: notificationListParams(query) });
  return client.parse(NotificationList, body, `GET ${path}`);
}

export interface NotificationPageQuery extends NotificationListQuery {
  perPage: number;
  cursor: string;
}

/**
 * Paginated inbox list: same endpoint with the page size (`per_page`,
 * fixed 300 in the app) and the opaque cursor (the initial cursor for
 * fresh loads, the stored next cursor for continuations).
 */
export async function listNotificationsPage(
  client: ApiClient,
  slug: string,
  query: NotificationPageQuery
): Promise<NotificationListResponse> {
  const path = `/api/workspaces/${slug}/users/notifications`;
  const body = await client.getJson<unknown>(path, {
    query: { ...notificationListParams(query), per_page: query.perPage, cursor: query.cursor },
  });
  return client.parse(NotificationListResponse, body, `GET ${path}`);
}

/**
 * Mark one card read. POST .../notifications/<id>/read/ answers the
 * updated row with the read timestamp set. No body.
 */
export async function markNotificationRead(client: ApiClient, slug: string, id: string): Promise<NotificationItem> {
  const path = `/api/workspaces/${slug}/users/notifications/${id}/read/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(NotificationItem, envelope.data, `POST ${path}`);
}

/**
 * Mark one card unread. DELETE answers the updated row with the read
 * timestamp cleared.
 */
export async function markNotificationUnread(client: ApiClient, slug: string, id: string): Promise<NotificationItem> {
  const path = `/api/workspaces/${slug}/users/notifications/${id}/read/`;
  const envelope = await client.sendJson<unknown>("DELETE", path, {});
  return client.parse(NotificationItem, envelope.data, `DELETE ${path}`);
}

/**
 * Archive one card. POST .../notifications/<id>/archive/ answers the
 * updated row with the archive timestamp set. No body.
 */
export async function archiveNotification(client: ApiClient, slug: string, id: string): Promise<NotificationItem> {
  const path = `/api/workspaces/${slug}/users/notifications/${id}/archive/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(NotificationItem, envelope.data, `POST ${path}`);
}

/**
 * Return one card to the default stream. DELETE answers the updated
 * row with the archive timestamp cleared.
 */
export async function unarchiveNotification(client: ApiClient, slug: string, id: string): Promise<NotificationItem> {
  const path = `/api/workspaces/${slug}/users/notifications/${id}/archive/`;
  const envelope = await client.sendJson<unknown>("DELETE", path, {});
  return client.parse(NotificationItem, envelope.data, `DELETE ${path}`);
}

/**
 * Snooze or unsnooze one card. PATCH .../notifications/<id>/ honors
 * only the resume timestamp: a set value snoozes to that moment, null
 * clears the snooze. Answers the updated row.
 */
export async function snoozeNotification(
  client: ApiClient,
  slug: string,
  id: string,
  snoozedTill: string | null
): Promise<NotificationItem> {
  const path = `/api/workspaces/${slug}/users/notifications/${id}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { snoozed_till: snoozedTill } });
  return client.parse(NotificationItem, envelope.data, `PATCH ${path}`);
}

/**
 * Bulk mark-all-read scope: the current list filters as the view reads
 * them (origin scope, snoozed and archived flags). Unlike the list
 * query this travels as a JSON body of real booleans.
 */
export interface MarkAllReadScope {
  snoozed?: boolean;
  archived?: boolean;
  type?: string;
}

export const MarkAllReadResponse = z.object({
  message: z.string(),
});
export type MarkAllReadResponse = z.infer<typeof MarkAllReadResponse>;

/**
 * Mark every unread notification in scope as read. POST
 * .../users/notifications/mark-all-read/ answers a success message.
 */
export async function markAllNotificationsRead(
  client: ApiClient,
  slug: string,
  scope: MarkAllReadScope = {}
): Promise<MarkAllReadResponse> {
  const path = `/api/workspaces/${slug}/users/notifications/mark-all-read/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      ...(scope.snoozed === undefined ? {} : { snoozed: scope.snoozed }),
      ...(scope.archived === undefined ? {} : { archived: scope.archived }),
      ...(scope.type === undefined ? {} : { type: scope.type }),
    },
  });
  return client.parse(MarkAllReadResponse, envelope.data, `POST ${path}`);
}

/**
 * Email delivery preferences. The five toggles map to the preference
 * topics: property changes, state changes, comments, mentions, and the
 * completed-only refinement under state changes. `workspace`/`project`
 * stay null for the per-user row the settings page edits.
 */
export const NotificationPreferences = z.object({
  id: uuid(),
  user: uuid(),
  workspace: nullableUuid(),
  project: nullableUuid(),
  property_change: z.boolean(),
  state_change: z.boolean(),
  comment: z.boolean(),
  mention: z.boolean(),
  issue_completed: z.boolean(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type NotificationPreferences = z.infer<typeof NotificationPreferences>;

export interface NotificationPreferencePatch {
  property_change?: boolean;
  state_change?: boolean;
  comment?: boolean;
  mention?: boolean;
  issue_completed?: boolean;
}

/** GET /api/users/me/notification-preferences/ loads the toggles. */
export async function getNotificationPreferences(client: ApiClient): Promise<NotificationPreferences> {
  const path = "/api/users/me/notification-preferences/";
  const body = await client.getJson<unknown>(path);
  return client.parse(NotificationPreferences, body, `GET ${path}`);
}

/**
 * PATCH the same path with the single changed key per toggle; answers
 * the full updated row.
 */
export async function updateNotificationPreferences(
  client: ApiClient,
  patch: NotificationPreferencePatch
): Promise<NotificationPreferences> {
  const path = "/api/users/me/notification-preferences/";
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(NotificationPreferences, envelope.data, `PATCH ${path}`);
}

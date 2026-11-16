// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Email delivery preference contracts, split out of notifications.ts and
// re-exported there. Only the lazy email-preferences page reads them, so
// keeping them out of the eager contract module keeps the shell chunk
// smaller (NEWFRONT-523).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

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

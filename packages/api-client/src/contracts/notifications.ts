// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notification contracts for the shell: the unread count behind the
// sidebar/top-bar badge. Full notification ownership stays in the
// notifications area; this is the slice the shell needs to compile.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

export const UnreadNotifications = z.object({
  total_unread_notifications_count: z.number(),
  mention_unread_notifications_count: z.number(),
});
export type UnreadNotifications = z.infer<typeof UnreadNotifications>;

export async function getUnreadNotifications(client: ApiClient, slug: string): Promise<UnreadNotifications> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/users/notifications/unread/`);
  return client.parse(UnreadNotifications, body, `GET /api/workspaces/${slug}/users/notifications/unread/`);
}

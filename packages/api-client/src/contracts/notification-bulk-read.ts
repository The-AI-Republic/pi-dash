// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk mark-all-read contract, split out of notifications.ts and re-exported
// there so the eager shell, which reads only the unread totals and the
// inbox list, does not carry this write into the initial chunk
// (NEWFRONT-523).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

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

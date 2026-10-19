// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook address check (parity/specs/workspace-settings.md §36,
// WSET-036). The shell asks the detail endpoint whether the id exists so
// an unknown id becomes a missing-or-deleted notice, not an endless
// spinner (NEWFRONT-417). The webhooks slice owns the webhook contracts;
// this read wants nothing from the body, only the answer's status.

import { queryOptions } from "@tanstack/react-query";

import type { ApiClient } from "@pidash/api-client";
import { workspaceScope } from "../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "../lib/settingsQuery.js";

export function workspaceWebhookPresenceKey(workspaceSlug: string, webhookId: string): readonly unknown[] {
  return [...workspaceScope(workspaceSlug), "workspace-settings", "webhook-presence", webhookId];
}

export function workspaceWebhookPresenceOptions(client: ApiClient, workspaceSlug: string, webhookId: string) {
  return queryOptions<true>({
    queryKey: workspaceWebhookPresenceKey(workspaceSlug, webhookId),
    queryFn: async (): Promise<true> => {
      await client.getJson<unknown>(`/api/workspaces/${workspaceSlug}/webhooks/${webhookId}/`);
      return true;
    },
    retry: false,
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
  });
}

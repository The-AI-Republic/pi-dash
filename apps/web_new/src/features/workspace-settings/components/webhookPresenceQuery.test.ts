// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook address check (WSET-036/038): one GET of the detail endpoint,
// scoped to the workspace and shaped by the settings fetch policy.
import { describe, expect, it } from "vitest";

import { createClient } from "@pidash/api-client";
import { workspaceScope } from "../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "../lib/settingsQuery.js";
import { workspaceWebhookPresenceKey, workspaceWebhookPresenceOptions } from "./webhookPresenceQuery.js";

const HOOK = "223e4567-e89b-12d3-a456-426614174001";

describe("workspaceWebhookPresenceOptions", () => {
  it("keys the read under the workspace scope", () => {
    expect(workspaceWebhookPresenceKey("acme", HOOK)).toEqual([
      ...workspaceScope("acme"),
      "workspace-settings",
      "webhook-presence",
      HOOK,
    ]);
  });

  it("reads the detail endpoint once and follows the mount-only policy", async () => {
    const urls: string[] = [];
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url) => {
        urls.push(url);
        return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
      },
    });
    const options = workspaceWebhookPresenceOptions(client, "acme", HOOK);
    expect(options.retry).toBe(false);
    expect(options.staleTime).toBe(WORKSPACE_SETTINGS_QUERY_POLICY.staleTime);
    expect(options.refetchOnWindowFocus).toBe(false);
    await options.queryFn?.(undefined as never);
    expect(urls).toEqual([`https://api.test/api/workspaces/acme/webhooks/${HOOK}/`]);
  });
});

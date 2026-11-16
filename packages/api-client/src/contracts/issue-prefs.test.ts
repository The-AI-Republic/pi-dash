// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project and workspace display-preference PATCH contracts against a stub
// client (no backend). The same PATCH semantics ride the cycle/module
// endpoints, proven live in issue-prefs.contract.test.ts.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { updateProjectDisplayPreferences, updateWorkspaceDisplayPreferences } from "./issue-prefs.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const AT = "2026-10-16T15:32:05.479661+09:00";

function stubClient(impl: { sendJson?: ApiClient["sendJson"] }): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

const PROJECT_ROW = {
  id: UUID,
  workspace: UUID,
  project: UUID2,
  user: UUID,
  filters: {},
  display_filters: { layout: "kanban", group_by: null, order_by: "sort_order" },
  display_properties: {},
  rich_filters: {},
  preferences: {},
  sort_order: 1,
  created_by: null,
  updated_by: null,
  created_at: AT,
  updated_at: AT,
};

const WORKSPACE_ROW = {
  id: UUID,
  workspace: UUID,
  user: UUID,
  filters: {},
  display_filters: { layout: "calendar", group_by: null, order_by: "-created_at" },
  display_properties: { state: false },
  rich_filters: {},
  navigation_project_limit: 0,
  navigation_control_preference: "ACCORDION",
  created_by: null,
  updated_by: null,
  created_at: AT,
  updated_at: AT,
};

describe("updateProjectDisplayPreferences", () => {
  it("patches the display keys through the project user-properties route", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: PROJECT_ROW, status: 201, url };
      }) as ApiClient["sendJson"],
    });
    const patch = { display_filters: { layout: "kanban", group_by: null, order_by: "sort_order" } };
    const parsed = await updateProjectDisplayPreferences(client, "acme", "pid", patch);
    expect(calls[0]?.method).toBe("PATCH");
    expect(calls[0]?.url).toBe("/api/workspaces/acme/projects/pid/user-properties/");
    expect(calls[0]?.json).toEqual(patch);
    expect(parsed.display_filters["layout"]).toBe("kanban");
  });
});

describe("updateWorkspaceDisplayPreferences", () => {
  it("patches the display keys through the workspace user-properties route", async () => {
    const calls: { method?: string; url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (method: string, url: string, options: { json?: unknown }) => {
        calls.push({ method, url, json: options.json });
        return { data: WORKSPACE_ROW, status: 201, url };
      }) as ApiClient["sendJson"],
    });
    const patch = {
      display_filters: { layout: "calendar", group_by: null, order_by: "-created_at" },
      display_properties: { state: false },
    };
    const parsed = await updateWorkspaceDisplayPreferences(client, "acme", patch);
    expect(calls[0]?.method).toBe("PATCH");
    expect(calls[0]?.url).toBe("/api/workspaces/acme/user-properties/");
    expect(calls[0]?.json).toEqual(patch);
    expect(parsed.display_filters["layout"]).toBe("calendar");
    expect(parsed.display_properties["state"]).toBe(false);
  });
});

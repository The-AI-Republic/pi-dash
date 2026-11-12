// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake route preload (NEWFRONT-869): the route reads the project row
// once, warms the active tab's list only while the feature is on, and
// never turns a refused row read into a navigation failure.
import { QueryClient } from "@tanstack/react-query";
import { createClient } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { preloadIntakeRoute } from "./preload.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function fakeApi(project: unknown, calls: string[]) {
  return createClient({
    baseUrl: "https://api.test",
    transport: (async (url: string) => {
      const parsed = new URL(url);
      calls.push(parsed.pathname + parsed.search);
      const body = (value: unknown, status = 200) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(value),
      });
      if (parsed.pathname.endsWith("/auth/get-csrf-token/")) return body({ csrf_token: "csrf" });
      if (parsed.pathname === `/api/workspaces/acme/projects/${PROJECT_ID}/`) return body(project);
      return body({ results: [], total_count: 0, next_cursor: "", count: 0 });
    }) as never,
  });
}

function project(inboxView: boolean) {
  return {
    id: PROJECT_ID,
    name: "Apollo",
    identifier: "APO",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    archived_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: false,
    inbox_view: inboxView,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
  };
}

describe("preloadIntakeRoute", () => {
  it("warms the active tab's list while the feature is on", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const calls: string[] = [];
    await preloadIntakeRoute(
      queryClient,
      fakeApi(project(true), calls),
      { ws: "acme", projectId: PROJECT_ID },
      { currentTab: "closed" }
    );
    const list = calls.find((call) => call.includes("/intake-issues/"));
    expect(list).toBeDefined();
    expect(new URLSearchParams((list ?? "").split("?")[1] ?? "").get("status")).toBe("1,-1,2");
  });

  it("reads no list while the feature is off", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const calls: string[] = [];
    await preloadIntakeRoute(queryClient, fakeApi(project(false), calls), { ws: "acme", projectId: PROJECT_ID }, {});
    expect(calls.some((call) => call.includes("/intake-issues/"))).toBe(false);
  });

  it("settles a refused project read without failing the navigation", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const calls: string[] = [];
    await expect(
      preloadIntakeRoute(queryClient, fakeApi({ error: "nope" }, calls), { ws: "acme", projectId: PROJECT_ID }, {})
    ).resolves.toBeUndefined();
    expect(calls.some((call) => call.includes("/intake-issues/"))).toBe(false);
  });
});

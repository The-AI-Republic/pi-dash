// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reads behind the General tab (PSET-005/PSET-008): request paths and
// parameters, the identifier precheck gate, repository status, and the
// cache seeding that keeps the shell's project read in step.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../../core/api/client.js";
import { authKeys } from "../../../auth/queries.js";
import {
  generalKeys,
  readRepositoryStatus,
  repositoryStatusQueryOptions,
  seedProjectRow,
  useGeneralDetail,
  useIdentifierCheck,
  useRepositoryStatus,
} from "./queries.js";

const UUID = "223e4567-e89b-12d3-a456-426614174001";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function projectRow(overrides: Record<string, unknown> = {}) {
  return {
    id: UUID,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    name: "Website relaunch",
    description: "Public site work",
    description_text: null,
    description_html: null,
    network: 0,
    identifier: "WEB",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: true,
    cycle_view: false,
    issue_views_view: true,
    page_view: false,
    intake_view: true,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "main",
    is_favorite: false,
    sort_order: null,
    member_role: null,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
    default_agent_executor: "local_runner",
    agent_ticking_enabled: true,
    agent_default_max_ticks: 5,
    agent_default_interval_seconds: 10_800,
    agent_review_default_interval_seconds: 10_800,
    agent_test_default_interval_seconds: 10_800,
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
}

function stubApi(routes: Array<{ method: string; path: string; status: number; body: unknown }>) {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ method, url });
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    const match = routes.find((route) => route.method === method && url.startsWith(`https://api.test${route.path}`));
    if (match === undefined) throw new Error(`unexpected fetch ${method} ${url}`);
    return jsonResponse(url, match.status, match.body);
  }) as never);
  return { calls };
}

function testClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("useGeneralDetail", () => {
  it("reads the project row from the workspace's project endpoint", async () => {
    const { calls } = stubApi([
      { method: "GET", path: `/api/workspaces/acme/projects/${UUID}/`, status: 200, body: projectRow() },
    ]);
    const hook = renderHook(() => useGeneralDetail("acme", UUID), { wrapper: wrapper(testClient()) });
    await waitFor(() => {
      if (hook.result.current.isError) throw hook.result.current.error;
      expect(hook.result.current.isSuccess).toBe(true);
    });
    expect(hook.result.current.data).toMatchObject({
      name: "Website relaunch",
      agent_default_max_ticks: 5,
      agent_default_interval_seconds: 10_800,
    });
    expect(calls.filter((call) => call.url.includes(`/projects/${UUID}/`))).toHaveLength(1);
  });
});

describe("useIdentifierCheck", () => {
  it("asks the availability endpoint for a changed code", async () => {
    const { calls } = stubApi([
      {
        method: "GET",
        path: "/api/workspaces/acme/project-identifiers/",
        status: 200,
        body: { exists: 0, identifiers: [] },
      },
    ]);
    const hook = renderHook(() => useIdentifierCheck("acme", "NEW"), { wrapper: wrapper(testClient()) });
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));
    expect(calls.some((call) => call.url.includes("project-identifiers/?name=NEW"))).toBe(true);
    expect(hook.result.current.data).toEqual({ exists: 0, identifiers: [] });
  });

  it("makes no request while the code is unchanged or empty", async () => {
    const { calls } = stubApi([]);
    const hook = renderHook(() => useIdentifierCheck("acme", null), { wrapper: wrapper(testClient()) });
    expect(hook.result.current.fetchStatus).toBe("idle");
    expect(calls).toEqual([]);
  });
});

describe("useRepositoryStatus", () => {
  it("reads the binding status only when enabled", async () => {
    const { calls } = stubApi([
      {
        method: "GET",
        path: `/api/workspaces/acme/projects/${UUID}/repository/`,
        status: 200,
        body: { bound: false },
      },
    ]);
    const queryClient = testClient();
    const disabled = renderHook(() => useRepositoryStatus("acme", UUID, false), { wrapper: wrapper(queryClient) });
    expect(disabled.result.current.fetchStatus).toBe("idle");
    expect(calls).toEqual([]);

    const enabled = renderHook(() => useRepositoryStatus("acme", UUID, true), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(enabled.result.current.isSuccess).toBe(true));
    expect(enabled.result.current.data).toEqual({ bound: false });
  });

  it("exposes fetch options callers can await outside a component", () => {
    expect(repositoryStatusQueryOptions({} as never, "acme", UUID)).toMatchObject({
      queryKey: generalKeys.repository("acme", UUID),
      queryFn: expect.any(Function),
    });
  });
});

describe("seedProjectRow", () => {
  it("writes the tab row and the shell's project read, then re-reads the collection", async () => {
    const queryClient = testClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    queryClient.setQueryData(authKeys.project("acme", UUID), {
      id: UUID,
      name: "Old",
      identifier: "OLD",
      logo_props: {},
      base_branch: "main",
    });
    const row = projectRow({ name: "Renamed", identifier: "REN" });

    seedProjectRow(queryClient, "acme", UUID, row);

    expect(queryClient.getQueryData(generalKeys.detail("acme", UUID))).toEqual(row);
    expect(queryClient.getQueryData(authKeys.project("acme", UUID))).toMatchObject({
      id: UUID,
      name: "Renamed",
      identifier: "REN",
      base_branch: "main",
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["ws", "acme", "projects"] });
  });

  it("starts the shell read from the row when no project was cached", () => {
    const queryClient = testClient();
    seedProjectRow(queryClient, "acme", UUID, projectRow({ name: "Fresh" }));
    expect(queryClient.getQueryData(authKeys.project("acme", UUID))).toMatchObject({ name: "Fresh" });
  });
});

describe("readRepositoryStatus", () => {
  it("fetches the binding once and caches it under the tab's key", async () => {
    const { calls } = stubApi([
      { method: "GET", path: `/api/workspaces/acme/projects/${UUID}/repository/`, status: 200, body: { bound: false } },
    ]);
    const queryClient = testClient();
    const api = getAppClient();
    const status = await readRepositoryStatus(queryClient, api, "acme", UUID);
    expect(status).toEqual({ bound: false });
    await readRepositoryStatus(queryClient, api, "acme", UUID);
    expect(calls.filter((call) => call.url.endsWith("/repository/"))).toHaveLength(1);
    expect(queryClient.getQueryData(generalKeys.repository("acme", UUID))).toEqual({ bound: false });
  });
});

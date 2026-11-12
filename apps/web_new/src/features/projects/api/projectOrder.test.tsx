// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { ProjectList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "./queries.js";
import { applyProjectOrder, PROJECT_ORDER_FAILED_TITLE, useUpdateProjectOrder } from "./projectOrder.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_A = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_B = "323e4567-e89b-12d3-a456-426614174002";
const USER_ID = "423e4567-e89b-12d3-a456-426614174003";

function row(id: string, sortOrder: number | null): ProjectList[number] {
  return {
    id,
    name: `Project ${id.slice(0, 1)}`,
    identifier: id.slice(0, 1).toUpperCase(),
    sort_order: sortOrder,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

function propertiesPayload(projectId: string, sortOrder: number): Record<string, unknown> {
  return {
    id: "523e4567-e89b-12d3-a456-426614174004",
    workspace: WS_ID,
    project: projectId,
    user: USER_ID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    preferences: {},
    sort_order: sortOrder,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

interface StubOptions {
  patchStatus?: number;
  hangGet?: boolean;
}

function stubClient(options: StubOptions = {}) {
  let server: ProjectList = [row(PROJECT_A, 10000), row(PROJECT_B, 20000)];
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (init.method === "PATCH") {
      const status = options.patchStatus ?? 200;
      if (status !== 200) {
        return { status, url, headers: { get: () => null }, text: async () => "{}" };
      }
      const sent = JSON.parse(init.body ?? "{}") as { sort_order?: number };
      const match = /\/projects\/([^/]+)\/user-properties\//.exec(url);
      const projectId = match?.[1] ?? "";
      server = server.map((entry) =>
        entry.id === projectId && sent.sort_order !== undefined ? { ...entry, sort_order: sent.sort_order } : entry
      );
      const stored = server.find((entry) => entry.id === projectId);
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(propertiesPayload(projectId, stored?.sort_order ?? 0)),
      };
    }
    if (options.hangGet === true) return new Promise<never>(() => undefined);
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(server),
    };
  }) as never);
  return { calls };
}

describe("applyProjectOrder", () => {
  it("applies the dragged position to the cached rows", () => {
    const next = applyProjectOrder([row(PROJECT_A, 10000), row(PROJECT_B, 20000)], PROJECT_B, 0);
    expect(next?.map((entry) => entry.sort_order)).toEqual([10000, 0]);
  });

  it("passes an empty cache through", () => {
    expect(applyProjectOrder(undefined, PROJECT_B, 0)).toBeUndefined();
  });
});

describe("useUpdateProjectOrder", () => {
  function renderHandle(options: StubOptions = {}) {
    const { calls } = stubClient(options);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    let save: ((input: { projectId: string; sort_order: number }) => void) | null = null;
    function Probe(): React.ReactElement {
      const list = useQuery(projectsQueryOptions(getAppClient(), "acme"));
      const mutation = useUpdateProjectOrder("acme");
      save = (input) => mutation.mutate(input);
      const data = list.data ?? [];
      return <p>{`${list.status}|${data.map((entry) => `${entry.id.slice(0, 1)}:${entry.sort_order}`).join(",")}`}</p>;
    }
    render(
      <QueryClientProvider client={queryClient}>
        <Probe />
      </QueryClientProvider>
    );
    return {
      calls,
      text: () => screen.getByText(/\|/).textContent ?? "",
      save: (input: { projectId: string; sort_order: number }) => save?.(input),
    };
  }

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetAppClient();
  });

  it("patches the dragged position and keeps the optimistic order", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe(`success|2:10000,3:20000`));
    h.save({ projectId: PROJECT_B, sort_order: 0 });
    await waitFor(() => expect(h.text()).toBe(`success|2:10000,3:0`));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/api/workspaces/acme/projects/${PROJECT_B}/user-properties/`);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sort_order: 0 });
  });

  it("reverts the optimistic write and toasts when the save fails", async () => {
    const h = renderHandle({ patchStatus: 500 });
    await waitFor(() => expect(h.text()).toBe(`success|2:10000,3:20000`));
    h.save({ projectId: PROJECT_B, sort_order: 0 });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(PROJECT_ORDER_FAILED_TITLE, expect.any(String)));
    expect(h.text()).toBe(`success|2:10000,3:20000`);
  });

  it("still sends the write while the first read hangs", async () => {
    const h = renderHandle({ hangGet: true });
    await screen.findByText("pending|");
    h.save({ projectId: PROJECT_B, sort_order: 0 });
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(notifyError).not.toHaveBeenCalled();
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "./keys.js";
import {
  useCreateWorkItem,
  WORK_ITEM_CREATED_TITLE,
  WORK_ITEM_CREATE_FAILED_TITLE,
  type WorkItemCreateInput,
} from "./create.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function createdRow(): Record<string, unknown> {
  return {
    id: "323e4567-e89b-12d3-a456-426614174002",
    name: "Fresh report",
    state_id: null,
    sort_order: 65535,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: PROJECT_ID,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    deleted_at: null,
  };
}

function stubClient(status: number) {
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
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => (status === 201 ? JSON.stringify(createdRow()) : "{}"),
    };
  }) as never);
  return { calls };
}

describe("useCreateWorkItem", () => {
  function renderHandle(status: number) {
    const { calls } = stubClient(status);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    let save: ((input: WorkItemCreateInput) => void) | null = null;
    function Probe(): React.ReactElement {
      const mutation = useCreateWorkItem("acme");
      save = (input) => mutation.mutate(input);
      return <p>{`create:${mutation.status}`}</p>;
    }
    render(
      <QueryClientProvider client={queryClient}>
        <Probe />
      </QueryClientProvider>
    );
    return {
      calls,
      invalidate,
      save: (input: WorkItemCreateInput) => save?.(input),
    };
  }

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetAppClient();
  });

  it("posts the title and announces the new row", async () => {
    const h = renderHandle(201);
    h.save({ projectId: PROJECT_ID, name: "Fresh report" });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(WORK_ITEM_CREATED_TITLE, expect.any(String)));
    const post = h.calls.find((call) => call.method === "POST");
    expect(post?.url).toBe(`https://api.test/api/workspaces/acme/projects/${PROJECT_ID}/issues/`);
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ name: "Fresh report" });
    await waitFor(() => expect(h.invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.all("acme") }));
    expect(screen.getByText("create:success")).toBeInTheDocument();
  });

  it("announces the failure without creating", async () => {
    const h = renderHandle(400);
    h.save({ projectId: PROJECT_ID, name: "Fresh report" });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(WORK_ITEM_CREATE_FAILED_TITLE, expect.any(String)));
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(screen.getByText("create:error")).toBeInTheDocument();
  });
});

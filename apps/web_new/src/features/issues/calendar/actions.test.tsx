// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  CALENDAR_CREATE_FAILED_TITLE,
  CALENDAR_CREATE_SUCCESS_TITLE,
  CALENDAR_ATTACH_FAILED_TITLE,
  CALENDAR_ATTACH_SUCCESS_TITLE,
  CALENDAR_DELETE_FAILED_TITLE,
  CALENDAR_DELETE_SUCCESS_TITLE,
  REDATE_FAILED_TITLE,
  REDATE_SUCCESS_TITLE,
  dropOutcome,
  useCalendarAttach,
  useCalendarCreate,
  useCalendarDelete,
  useRedateIssue,
} from "./actions.js";
import { issueKeys } from "../api/keys.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return React.createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

function createdRow() {
  return {
    id: ISSUE_ID,
    name: "Dated item",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    start_date: null,
    target_date: "2026-10-08",
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
    created_at: "2026-10-08T00:00:00Z",
    updated_at: "2026-10-08T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    deleted_at: null,
  };
}

function harness(status = 204) {
  const calls: Array<{ method: string; url: string; body: string }> = [];
  createAppClient("https://api.test", async (url, init) => {
    const method = init.method ?? "GET";
    if (url.includes("csrf")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "test" }),
      };
    }
    calls.push({
      method,
      url,
      body: typeof init.body === "string" ? init.body : "",
    });
    if (url.includes("/cycle-issues/") || url.includes("/modules/")) {
      return {
        status: 201,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ message: "ok" }),
      };
    }
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => (status === 201 ? JSON.stringify(createdRow()) : ""),
    };
  });
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false }, queries: { retry: false } } });
  return { calls, queryClient };
}

describe("calendar actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });
  afterEach(() => {
    resetAppClient();
  });

  it("decides drops without the mutation", () => {
    expect(
      dropOutcome({ issue: { id: ISSUE_ID, target_date: "2026-10-05", start_date: null }, destination: "2026-10-05" })
    ).toEqual({
      kind: "noop",
    });
    expect(
      dropOutcome({
        issue: { id: ISSUE_ID, target_date: "2026-10-05", start_date: "2026-10-08" },
        destination: "2026-10-06",
      })
    ).toEqual({ kind: "refuse", reason: "The target date cannot fall before the start date." });
  });

  it("patches the target date and refreshes the lists", async () => {
    const { calls, queryClient } = harness();
    const { result } = renderHook(() => useRedateIssue("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.reDate({
      issue: { id: ISSUE_ID, target_date: "2026-10-05", start_date: null },
      destination: "2026-10-08",
    });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(REDATE_SUCCESS_TITLE));
    expect(calls).toHaveLength(1);
    expect(calls[0]?.method).toBe("PATCH");
    expect(calls[0]?.url).toContain(`/issues/${ISSUE_ID}/`);
    expect(JSON.parse(calls[0]?.body ?? "{}")).toEqual({ target_date: "2026-10-08" });
  });

  it("skips the network for a same-day drop", async () => {
    const { calls, queryClient } = harness();
    const { result } = renderHook(() => useRedateIssue("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.reDate({
      issue: { id: ISSUE_ID, target_date: "2026-10-05", start_date: null },
      destination: "2026-10-05",
    });
    await waitFor(() => expect(queryClient.getQueryCache().getAll().length).toBe(0));
    expect(calls).toHaveLength(0);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("toasts a refused drop and sends nothing", async () => {
    const { calls, queryClient } = harness();
    const { result } = renderHook(() => useRedateIssue("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.reDate({
      issue: { id: ISSUE_ID, target_date: "2026-10-08", start_date: "2026-10-08" },
      destination: "2026-10-06",
    });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        REDATE_FAILED_TITLE,
        "The target date cannot fall before the start date."
      )
    );
    expect(calls).toHaveLength(0);
  });

  it("creates a dated work item and toasts success", async () => {
    const { calls, queryClient } = harness(201);
    const { result } = renderHook(() => useCalendarCreate("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.mutate({ name: "Dated item", day: "2026-10-08" });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(CALENDAR_CREATE_SUCCESS_TITLE));
    expect(calls[0]?.method).toBe("POST");
    expect(JSON.parse(calls[0]?.body ?? "{}")).toMatchObject({ name: "Dated item", target_date: "2026-10-08" });
  });

  it("toasts a failed create", async () => {
    const { queryClient } = harness(500);
    const { result } = renderHook(() => useCalendarCreate("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.mutate({ name: "Dated item", day: "2026-10-08" });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(CALENDAR_CREATE_FAILED_TITLE, expect.any(String)));
  });

  it("re-dates every selected item onto the day", async () => {
    const { calls, queryClient } = harness();
    const { result } = renderHook(() => useCalendarAttach("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.mutate({ issueIds: [ISSUE_ID, "423e4567-e89b-12d3-a456-426614174003"], day: "2026-10-09" });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(CALENDAR_ATTACH_SUCCESS_TITLE));
    expect(calls).toHaveLength(2);
    expect(calls.every((call) => call.method === "PATCH")).toBe(true);
    expect(JSON.parse(calls[0]?.body ?? "{}")).toEqual({ target_date: "2026-10-09" });
  });

  it("marks the calendar list window stale after attaching items", async () => {
    const { queryClient } = harness();
    const calendarKey = [...issueKeys.lists("acme", PROJECT_ID), "calendar", "2026-10-01", "2026-10-31", 4];
    queryClient.setQueryData(calendarKey, { results: {} });
    const { result } = renderHook(() => useCalendarAttach("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.mutate({ issueIds: [ISSUE_ID], day: "2026-10-09" });
    await waitFor(() => expect(queryClient.getQueryState(calendarKey)?.isInvalidated).toBe(true));
  });

  it("attaches a cycle and module when the calendar has contextual membership", async () => {
    const { calls, queryClient } = harness();
    const { result } = renderHook(
      () =>
        useCalendarAttach("acme", PROJECT_ID, {
          cycleId: "823e4567-e89b-12d3-a456-426614174007",
          moduleId: "923e4567-e89b-12d3-a456-426614174008",
        }),
      { wrapper: wrapper(queryClient) }
    );
    result.current.mutate({ issueIds: [ISSUE_ID], day: "2026-10-09" });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(CALENDAR_ATTACH_SUCCESS_TITLE));
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      `PATCH https://api.test/api/workspaces/acme/projects/${PROJECT_ID}/issues/${ISSUE_ID}/`,
      `POST https://api.test/api/workspaces/acme/projects/${PROJECT_ID}/cycles/823e4567-e89b-12d3-a456-426614174007/cycle-issues/`,
      `POST https://api.test/api/workspaces/acme/projects/${PROJECT_ID}/issues/${ISSUE_ID}/modules/`,
    ]);
    expect(JSON.parse(calls[2]?.body ?? "{}")).toEqual({ modules: ["923e4567-e89b-12d3-a456-426614174008"] });
  });

  it("attaches created items to the contextual cycle or module", async () => {
    const { calls, queryClient } = harness(201);
    const { result } = renderHook(
      () => useCalendarCreate("acme", PROJECT_ID, { cycleId: "823e4567-e89b-12d3-a456-426614174007" }),
      { wrapper: wrapper(queryClient) }
    );
    result.current.mutate({ name: "Dated item", day: "2026-10-08" });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(CALENDAR_CREATE_SUCCESS_TITLE));
    expect(calls.map((call) => call.method)).toEqual(["POST", "POST"]);
    expect(calls[1]?.url).toContain("/cycles/823e4567-e89b-12d3-a456-426614174007/cycle-issues/");
  });

  it("toasts an attach failure", async () => {
    const { queryClient } = harness(500);
    const { result } = renderHook(() => useCalendarAttach("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.mutate({ issueIds: [ISSUE_ID], day: "2026-10-09" });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(CALENDAR_ATTACH_FAILED_TITLE, expect.any(String)));
  });

  it("deletes a block's issue and reports failures", async () => {
    const { calls, queryClient } = harness(204);
    const { result } = renderHook(() => useCalendarDelete("acme", PROJECT_ID), { wrapper: wrapper(queryClient) });
    result.current.mutate(ISSUE_ID);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(CALENDAR_DELETE_SUCCESS_TITLE));
    expect(calls[0]?.method).toBe("DELETE");

    const failed = harness(500);
    const { result: failedResult } = renderHook(() => useCalendarDelete("acme", PROJECT_ID), {
      wrapper: wrapper(failed.queryClient),
    });
    failedResult.current.mutate(ISSUE_ID);
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(CALENDAR_DELETE_FAILED_TITLE, expect.any(String)));
  });
});

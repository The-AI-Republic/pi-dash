// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates data hooks (PSET-049): the list read options, the create
// payload and the delete-plus-unlink sequence.
import { QueryClient, QueryClientProvider, focusManager } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import {
  ESTIMATE_CREATE_FAILED_TITLE,
  ESTIMATE_DELETE_FAILED_TITLE,
  ESTIMATE_UPDATED_TITLE,
  ESTIMATE_UPDATE_FAILED_BODY,
  ESTIMATE_UPDATE_FAILED_TITLE,
  estimatesKeys,
  projectEstimatesQueryOptions,
  useCreateEstimate,
  useDeleteEstimate,
  useProjectEstimates,
  useToggleEstimates,
  useUpdateEstimate,
} from "./api.js";

const WORKSPACE = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const API_ROOT = "https://api.test";
const ESTIMATES_PATH = `${API_ROOT}/api/workspaces/${WORKSPACE}/projects/${PROJECT}/estimates/`;
const PROJECT_PATH = `${API_ROOT}/api/workspaces/${WORKSPACE}/projects/${PROJECT}/`;
const SYSTEM_ID = "44444444-4444-4444-8444-444444444444";
const POINT_A = "aaaaaaaa-0001-4000-8000-000000000001";
const POINT_B = "aaaaaaaa-0002-4000-8000-000000000002";
const POINT_C = "aaaaaaaa-0003-4000-8000-000000000003";

vi.mock("../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
}));

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

/** A project row complete enough for the strict detail contract. */
function projectDetail(estimate: string | null): Record<string, unknown> {
  return {
    id: PROJECT,
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "11111111-1111-4111-8111-111111111111",
    name: "Website",
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "WEB",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: true,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: true,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 20,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [],
    default_agent_executor: "",
  };
}

function system(id: string): Record<string, unknown> {
  return {
    id,
    workspace: "11111111-1111-4111-8111-111111111111",
    project: PROJECT,
    name: "Points",
    description: "",
    type: "points",
    last_used: true,
    points: [],
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
  };
}

function stubApi(
  options: {
    createStatus?: number;
    deleteStatus?: number;
    patchStatus?: number;
    pointStatus?: number;
    updateStatus?: number;
    updateBody?: unknown;
  } = {}
) {
  const calls: { method: string; url: string; body: unknown }[] = [];
  let systems: Record<string, unknown>[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body = init.body === undefined ? undefined : (JSON.parse(init.body) as unknown);
    calls.push({ method, url, body });
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url === ESTIMATES_PATH && method === "GET") return jsonResponse(url, 200, systems);
    if (url === ESTIMATES_PATH && method === "POST") {
      if ((options.createStatus ?? 200) !== 200) return jsonResponse(url, options.createStatus ?? 500, {});
      return jsonResponse(url, 200, system("99999999-9999-4999-8999-999999999999"));
    }
    if (url.includes("/estimate-points/") && method === "POST") {
      if ((options.pointStatus ?? 200) !== 200) return jsonResponse(url, options.pointStatus ?? 500, {});
      const input = body as { key: number; value: string };
      return jsonResponse(url, 200, {
        id: `99999999-9999-4999-8999-99999999990${input.key}`,
        key: input.key,
        value: input.value,
        description: "",
      });
    }
    if (url.includes("/estimate-points/") && method === "DELETE") {
      if ((options.pointStatus ?? 200) !== 200) return jsonResponse(url, options.pointStatus ?? 500, {});
      return jsonResponse(url, 200, []);
    }
    if (url.startsWith(ESTIMATES_PATH) && url !== ESTIMATES_PATH && method === "PATCH") {
      if ((options.updateStatus ?? 200) !== 200)
        return jsonResponse(url, options.updateStatus ?? 500, options.updateBody ?? {});
      return jsonResponse(url, 200, system(SYSTEM_ID));
    }
    if (url.startsWith(ESTIMATES_PATH) && method === "DELETE") {
      if ((options.deleteStatus ?? 204) !== 204) return jsonResponse(url, options.deleteStatus ?? 500, {});
      return { ...jsonResponse(url, 204, {}), text: async () => "" } as TransportResponse;
    }
    if (url === PROJECT_PATH && method === "PATCH") {
      if ((options.patchStatus ?? 200) !== 200) return jsonResponse(url, options.patchStatus ?? 500, {});
      return jsonResponse(url, 200, projectDetail((body as { estimate?: string | null }).estimate ?? null));
    }
    if (url === PROJECT_PATH && method === "GET") return jsonResponse(url, 200, projectDetail(null));
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { calls, setSystems: (rows: Record<string, unknown>[]) => (systems = rows) };
}

interface WrapperProps {
  children?: unknown;
}

function wrapFn(): ({ children }: WrapperProps) => React.ReactElement {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return ({ children }) => <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("estimates data hooks", () => {
  it("keys and describes the list read", () => {
    expect(estimatesKeys.project(WORKSPACE, PROJECT).slice(0, 3)).toEqual(["ws", WORKSPACE, "estimates"]);
    expect(estimatesKeys.project(WORKSPACE, PROJECT)).toContain(PROJECT);
    const options = projectEstimatesQueryOptions({} as never, WORKSPACE, PROJECT);
    expect(options.queryKey).toEqual(estimatesKeys.project(WORKSPACE, PROJECT));
    // EST-027: no staleness window and an explicit focus opt-in, so a
    // revisit or a focus return refetches instead of replaying a cache.
    expect(options.staleTime).toBe(0);
    expect(options.refetchOnWindowFocus).toBe(true);
  });

  it("keeps an external change stale until a refetch runs", async () => {
    const { calls, setSystems } = stubApi();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: WrapperProps): React.ReactElement => (
      <QueryClientProvider client={client}>{children as never}</QueryClientProvider>
    );
    const { result } = renderHook(() => useProjectEstimates(WORKSPACE, PROJECT), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual([]);
    expect(calls.filter((call) => call.method === "GET")).toHaveLength(1);

    // Mutate behind the hook's back: no live push, so the hook keeps
    // showing the old state without a new request.
    setSystems([system("44444444-4444-4444-8444-444444444444")]);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(result.current.data).toEqual([]);
    expect(calls.filter((call) => call.method === "GET")).toHaveLength(1);

    await result.current.refetch();
    await waitFor(() => expect(result.current.data).toHaveLength(1));
  });

  it("refetches the list on remount and on window focus", async () => {
    const { calls, setSystems } = stubApi();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: WrapperProps): React.ReactElement => (
      <QueryClientProvider client={client}>{children as never}</QueryClientProvider>
    );
    const first = renderHook(() => useProjectEstimates(WORKSPACE, PROJECT), { wrapper });
    await waitFor(() => expect(first.result.current.isSuccess).toBe(true));
    expect(calls.filter((call) => call.method === "GET")).toHaveLength(1);
    first.unmount();

    // A revisit mounts fresh: with no staleness window the mount
    // refetches and picks up the system created while away.
    setSystems([system("44444444-4444-4444-8444-444444444444")]);
    const second = renderHook(() => useProjectEstimates(WORKSPACE, PROJECT), { wrapper });
    await waitFor(() => expect(second.result.current.data).toHaveLength(1));
    expect(calls.filter((call) => call.method === "GET")).toHaveLength(2);

    // Returning to the window refetches the stale read as well.
    setSystems([system("44444444-4444-4444-8444-444444444444"), system("55555555-5555-4555-8555-555555555555")]);
    focusManager.setFocused(false);
    await act(async () => {
      focusManager.setFocused(true);
    });
    await waitFor(() => expect(second.result.current.data).toHaveLength(2));
    expect(calls.filter((call) => call.method === "GET")).toHaveLength(3);
    focusManager.setFocused(undefined);
  });

  it("creates the system as active with numbered points", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useCreateEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({ name: "Points", type: "points", values: ["1", "2"] });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({
      estimate: { name: "Points", type: "points", last_used: true },
      estimate_points: [
        { key: 1, value: "1" },
        { key: 2, value: "2" },
      ],
    });
  });

  it("reports a refused create", async () => {
    stubApi({ createStatus: 500 });
    const { result } = renderHook(() => useCreateEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({ name: "Points", type: "points", values: ["1"] });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(ESTIMATE_CREATE_FAILED_TITLE).toContain("creation failed");
  });

  it("deletes the active system and clears the project reference", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useDeleteEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({ estimate: system("44444444-4444-4444-8444-444444444444") as never, active: true });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ estimate: null });
  });

  it("leaves the project reference alone for a superseded system", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useDeleteEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({ estimate: system("55555555-5555-4555-8555-555555555555") as never, active: false });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("toggles with the active id and with null, tolerating a cold project cache", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useToggleEstimates(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({ enabled: true, activeId: "44444444-4444-4444-8444-444444444444" });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual({ estimate: "44444444-4444-4444-8444-444444444444" });
    expect(calls.filter((call) => call.method === "PATCH").map((call) => call.body)).toEqual([
      { estimate: "44444444-4444-4444-8444-444444444444" },
    ]);

    result.current.mutate({ enabled: false, activeId: "44444444-4444-4444-8444-444444444444" });
    await waitFor(() => expect(result.current.data).toEqual({ estimate: null }));
    expect(calls.filter((call) => call.method === "PATCH").map((call) => call.body)).toEqual([
      { estimate: "44444444-4444-4444-8444-444444444444" },
      { estimate: null },
    ]);
  });

  it("reports a refused delete", async () => {
    stubApi({ deleteStatus: 500 });
    const { result } = renderHook(() => useDeleteEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({ estimate: system("44444444-4444-4444-8444-444444444444") as never, active: false });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(ESTIMATE_DELETE_FAILED_TITLE).toBe("Couldn’t delete the estimate");
  });

  it("saves renames and reorders through the system PATCH alone", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useUpdateEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({
      estimateId: SYSTEM_ID,
      name: "Renamed",
      survivors: [
        { id: POINT_B, key: 1, value: "B!" },
        { id: POINT_A, key: 2, value: "A" },
      ],
      adds: [],
      removals: [],
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith(ESTIMATE_UPDATED_TITLE, expect.any(String));
    const writes = calls.filter((call) => call.method !== "GET");
    expect(writes).toHaveLength(1);
    expect(writes[0]).toEqual({
      method: "PATCH",
      url: `${ESTIMATES_PATH}${SYSTEM_ID}/`,
      body: {
        estimate: { name: "Renamed" },
        estimate_points: [
          { id: POINT_B, key: 1, value: "B!" },
          { id: POINT_A, key: 2, value: "A" },
        ],
      },
    });
  });

  it("deletes, then creates, then patches the full arranged set", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useUpdateEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({
      estimateId: SYSTEM_ID,
      name: "Points",
      survivors: [
        { id: POINT_C, key: 1, value: "C" },
        { id: POINT_A, key: 3, value: "A" },
      ],
      adds: [{ key: 2, value: "fresh" }],
      removals: [{ pointId: POINT_B, newEstimateId: POINT_C }, { pointId: "bbbbbbbb-0004-4000-8000-000000000004" }],
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const writes = calls.filter((call) => call.method !== "GET");
    expect(writes.map((call) => call.method)).toEqual(["DELETE", "DELETE", "POST", "PATCH"]);
    // The replacement path names the surviving value; the clear path
    // sends no body at all.
    expect(writes[0]?.body).toEqual({ new_estimate_id: POINT_C });
    expect(writes[1]?.body).toBeUndefined();
    expect(writes[2]?.body).toEqual({ key: 2, value: "fresh" });
    // The trailing PATCH carries survivors plus the created id, sorted
    // by final key.
    expect(writes[3]?.body).toEqual({
      estimate: { name: "Points" },
      estimate_points: [
        { id: POINT_C, key: 1, value: "C" },
        { id: "99999999-9999-4999-8999-999999999902", key: 2, value: "fresh" },
        { id: POINT_A, key: 3, value: "A" },
      ],
    });
  });

  it("patches an all-new set after its creates", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useUpdateEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({
      estimateId: SYSTEM_ID,
      name: "Replaced",
      survivors: [],
      adds: [
        { key: 1, value: "one" },
        { key: 2, value: "two" },
      ],
      removals: [{ pointId: POINT_A }, { pointId: POINT_B }],
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const writes = calls.filter((call) => call.method !== "GET");
    expect(writes.map((call) => call.method)).toEqual(["DELETE", "DELETE", "POST", "POST", "PATCH"]);
    expect(writes[4]?.body).toEqual({
      estimate: { name: "Replaced" },
      estimate_points: [
        { id: "99999999-9999-4999-8999-999999999901", key: 1, value: "one" },
        { id: "99999999-9999-4999-8999-999999999902", key: 2, value: "two" },
      ],
    });
  });

  it("refuses an empty set before anything is sent", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useUpdateEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({ estimateId: SYSTEM_ID, name: "Points", survivors: [], adds: [], removals: [] });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(calls).toHaveLength(0);
    expect(notifyError).toHaveBeenCalledWith(ESTIMATE_UPDATE_FAILED_TITLE, ESTIMATE_UPDATE_FAILED_BODY);
  });

  it("refuses an id-less survivor before anything is sent", async () => {
    const { calls } = stubApi();
    const { result } = renderHook(() => useUpdateEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({
      estimateId: SYSTEM_ID,
      name: "Points",
      survivors: [{ key: 1, value: "ghost" } as never],
      adds: [],
      removals: [],
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(calls).toHaveLength(0);
    expect(notifyError).toHaveBeenCalledWith(ESTIMATE_UPDATE_FAILED_TITLE, ESTIMATE_UPDATE_FAILED_BODY);
  });

  it("reports a duplicate-name refusal without claiming success", async () => {
    stubApi({ updateStatus: 400, updateBody: { error: "The payload is not valid" } });
    const { result } = renderHook(() => useUpdateEstimate(WORKSPACE, PROJECT), { wrapper: wrapFn() });
    result.current.mutate({
      estimateId: SYSTEM_ID,
      name: "Taken",
      survivors: [{ id: POINT_A, key: 1, value: "A" }],
      adds: [],
      removals: [],
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(ESTIMATE_UPDATE_FAILED_TITLE, ESTIMATE_UPDATE_FAILED_BODY);
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});

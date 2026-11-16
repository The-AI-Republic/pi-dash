// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// States tab data layer (parity/specs/project-settings-states.md,
// PSET-029–034): optimistic patch/rollback, server-answer transitions,
// notices per refusal kind and the project-flag write.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ProjectDetails, State, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import { projectQueryOptions } from "../auth/index.js";
import {
  appendState,
  applyDefaultFlag,
  applyStateMove,
  applyStatePatch,
  removeState,
  sequenceAfterGroup,
  snapshotsOf,
  stateKeys,
  useCreateState,
  useDeleteState,
  useMarkStateDefault,
  useMoveState,
  useSetMemberEditing,
  useUpdateState,
} from "./api.js";
import {
  MEMBER_EDITING_FAILED_TITLE,
  MEMBER_EDITING_OFF_TITLE,
  MEMBER_EDITING_ON_TITLE,
  STATE_CREATED_TITLE,
  STATE_CREATE_FAILED_TITLE,
  STATE_DEFAULT_FAILED_TITLE,
  STATE_DEFAULT_TITLE,
  STATE_DELETE_FAILED_TITLE,
  STATE_DELETE_OCCUPIED_MESSAGE,
  STATE_DELETE_OCCUPIED_TITLE,
  STATE_DELETED_TITLE,
  STATE_DUPLICATE_CREATE_MESSAGE,
  STATE_DUPLICATE_RENAME_MESSAGE,
  STATE_UPDATE_FAILED_TITLE,
  STATE_UPDATED_TITLE,
  stateFailureDetail,
} from "./notices.js";
import { TEST_PROJECT, TEST_WORKSPACE, state as row, stateList, testRowId } from "./testFixtures.js";

/** A complete project row for the settings reads and flag writes. */
function projectDetail(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: TEST_PROJECT,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: TEST_WORKSPACE,
    name: "Website relaunch",
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
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: true,
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
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

const KEY = stateKeys.list(TEST_WORKSPACE, TEST_PROJECT);

interface StubOptions {
  patchStatus?: number;
  patchBody?: unknown;
  createStatus?: number;
  createBody?: unknown;
  deleteStatus?: number;
  deleteBody?: unknown;
  defaultStatus?: number;
  defaultBody?: unknown;
  projectStatus?: number;
  projectBody?: unknown;
  calls: string[];
}

function response(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => (body === undefined ? "" : JSON.stringify(body)),
  } as TransportResponse;
}

function stubTransport(options: Partial<StubOptions> = {}): StubOptions {
  const settings: StubOptions = { calls: [], ...options };
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    settings.calls.push(`${method} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) return response(url, 200, { csrf_token: "csrf" });
    if (url.includes("/states/") && method === "PATCH") {
      const status = settings.patchStatus ?? 200;
      if (status >= 400) return response(url, status, settings.patchBody ?? { error: "refused" });
      return response(url, status, settings.patchBody ?? patchEcho(url, init?.body));
    }
    if (url.endsWith("/mark-default/")) {
      const status = settings.defaultStatus ?? 204;
      return response(url, status, status >= 400 ? (settings.defaultBody ?? { error: "refused" }) : undefined);
    }
    if (url.endsWith("/states/") && method === "POST") {
      const status = settings.createStatus ?? 200;
      if (status >= 400) return response(url, status, settings.createBody ?? { error: "refused" });
      return response(url, status, settings.createBody ?? createdRow(init?.body));
    }
    if (url.includes("/states/") && method === "DELETE") {
      const status = settings.deleteStatus ?? 204;
      return response(url, status, status >= 400 ? (settings.deleteBody ?? { error: "refused" }) : undefined);
    }
    if (url.endsWith(`/projects/${TEST_PROJECT}/`) && method === "PATCH") {
      const status = settings.projectStatus ?? 200;
      return response(url, status, status >= 400 ? { error: "refused" } : projectDetail());
    }
    throw new Error(`unexpected ${method} ${url}`);
  }) as never);
  return settings;
}

function patchEcho(url: string, body: string | undefined): State {
  const id = url.split("/states/")[1]?.replace(/\/$/, "") ?? "";
  const base = row({ name: "echo", group: "backlog" });
  return { ...base, id, ...(JSON.parse(body ?? "{}") as object) };
}

function createdRow(body: string | undefined): State {
  const input = JSON.parse(body ?? "{}") as { name: string; group: State["group"]; color: string };
  return row({ name: input.name, group: input.group, color: input.color, sequence: 99 });
}

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

function queryClient(initial: State[] = []): QueryClient {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  client.setQueryData(KEY, initial);
  return client;
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("pure cache transitions", () => {
  const rows = stateList([
    { name: "a", group: "backlog" },
    { name: "b", group: "backlog" },
  ]);

  it("patches one row and leaves absent caches absent", () => {
    expect(applyStatePatch(rows, testRowId("b"), { name: "renamed" })?.[1]?.name).toBe("renamed");
    expect(applyStatePatch(undefined, testRowId("b"), { name: "x" })).toBeUndefined();
  });

  it("appends, removes and moves rows", () => {
    const added = row({ name: "c", group: "test" });
    expect(appendState(rows, added)).toHaveLength(3);
    expect(appendState(undefined, added)).toEqual([added]);
    expect(removeState(rows, testRowId("a"))?.map((entry) => entry.id)).toEqual([testRowId("b")]);
    expect(removeState(undefined, testRowId("a"))).toBeUndefined();
    expect(applyStateMove(rows, { stateId: testRowId("a"), group: "test", sequence: 7 })?.[0]).toMatchObject({
      group: "test",
      sequence: 7,
    });
    expect(applyStateMove(undefined, { stateId: testRowId("a"), group: "test", sequence: 7 })).toBeUndefined();
  });

  it("flips the exclusive default flag", () => {
    const flagged = stateList([
      { name: "a", group: "backlog", default: true },
      { name: "b", group: "test" },
    ]);
    const next = applyDefaultFlag(flagged, testRowId("b"));
    expect(next?.map((entry) => entry.default)).toEqual([false, true]);
    expect(applyDefaultFlag(undefined, testRowId("b"))).toBeUndefined();
  });

  it("reads snapshots from a mutation context, or none", () => {
    expect(snapshotsOf(undefined)).toEqual([]);
    expect(snapshotsOf({ snapshots: [{ queryKey: ["a"], data: 1 }] })).toHaveLength(1);
  });

  it("computes the next sequence past a group's tail or starts at zero", () => {
    const rows = stateList([
      { name: "a", group: "backlog", sequence: 10 },
      { name: "b", group: "backlog", sequence: 30 },
    ]);
    expect(sequenceAfterGroup(rows, "backlog")).toBe(31);
    expect(sequenceAfterGroup(rows, "test")).toBe(0);
  });
});

describe("useCreateState", () => {
  it("posts the group and fields, notices success and refreshes the list", async () => {
    const calls = stubTransport().calls;
    const client = queryClient();
    const { result } = renderHook(() => useCreateState(), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.mutateAsync({
        workspaceSlug: TEST_WORKSPACE,
        projectId: TEST_PROJECT,
        group: "backlog",
        input: { name: "Fresh", description: "why", color: "#123456" },
      });
    });
    expect(calls).toContain(`POST https://api.test/api/workspaces/${TEST_WORKSPACE}/projects/${TEST_PROJECT}/states/`);
    expect(notifySuccess).toHaveBeenCalledWith(STATE_CREATED_TITLE);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("notices the specific duplicate message and keeps the caller's form state", async () => {
    stubTransport({
      createStatus: 400,
      createBody: { name: "The state name is already taken" },
    });
    const { result } = renderHook(() => useCreateState(), { wrapper: wrapper(queryClient()) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({
          workspaceSlug: TEST_WORKSPACE,
          projectId: TEST_PROJECT,
          group: "backlog",
          input: { name: "Fresh", description: "", color: "#123456" },
        })
      ).rejects.toBeDefined();
    });
    expect(notifyError).toHaveBeenCalledWith(STATE_CREATE_FAILED_TITLE, STATE_DUPLICATE_CREATE_MESSAGE);
  });

  it("falls back to the server detail for other refusals", async () => {
    stubTransport({ createStatus: 500, createBody: { detail: "Server is down" } });
    const { result } = renderHook(() => useCreateState(), { wrapper: wrapper(queryClient()) });
    let error: unknown;
    await act(async () => {
      try {
        await result.current.mutateAsync({
          workspaceSlug: TEST_WORKSPACE,
          projectId: TEST_PROJECT,
          group: "backlog",
          input: { name: "Fresh", description: "", color: "#123456" },
        });
      } catch (caught) {
        error = caught;
      }
    });
    expect(notifyError).toHaveBeenCalledWith(STATE_CREATE_FAILED_TITLE, stateFailureDetail(error));
  });

  it("refuses an unknown group before any request", async () => {
    const calls = stubTransport().calls;
    const { result } = renderHook(() => useCreateState(), { wrapper: wrapper(queryClient()) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({
          workspaceSlug: TEST_WORKSPACE,
          projectId: TEST_PROJECT,
          group: "triage" as never,
          input: { name: "Triage", description: "", color: "#123456" },
        })
      ).rejects.toThrow("Unknown workflow group");
    });
    expect(calls).toEqual([]);
  });
});

describe("useUpdateState", () => {
  it("patches optimistically, notices success and stores the server row", async () => {
    const initial = stateList([{ name: "a", group: "backlog" }]);
    const calls = stubTransport({ patchBody: row({ name: "renamed", group: "backlog" }) }).calls;
    const client = queryClient(initial);
    const { result } = renderHook(() => useUpdateState(), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.mutateAsync({
        workspaceSlug: TEST_WORKSPACE,
        projectId: TEST_PROJECT,
        stateId: testRowId("a"),
        patch: { name: "renamed", description: "d", color: "#abcdef" },
      });
    });
    expect(calls.some((call) => call.startsWith("PATCH ") && call.includes(`/states/${testRowId("a")}/`))).toBe(true);
    expect(notifySuccess).toHaveBeenCalledWith(STATE_UPDATED_TITLE);
    expect(client.getQueryData<State[]>(KEY)?.[0]?.name).toBe("renamed");
  });

  it("restores the row and shows the specific duplicate notice when the rename clashes", async () => {
    const initial = stateList([{ name: "a", group: "backlog" }]);
    stubTransport({ patchStatus: 400, patchBody: { name: "The state name is already taken" } });
    const client = queryClient(initial);
    const { result } = renderHook(() => useUpdateState(), { wrapper: wrapper(client) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({
          workspaceSlug: TEST_WORKSPACE,
          projectId: TEST_PROJECT,
          stateId: testRowId("a"),
          patch: { name: "clash" },
        })
      ).rejects.toBeDefined();
    });
    expect(notifyError).toHaveBeenCalledWith(STATE_UPDATE_FAILED_TITLE, STATE_DUPLICATE_RENAME_MESSAGE);
    expect(client.getQueryData<State[]>(KEY)?.[0]?.name).toBe("a");
  });

  it("restores the row with the generic notice for other refusals", async () => {
    const initial = stateList([{ name: "a", group: "backlog" }]);
    stubTransport({ patchStatus: 500, patchBody: { detail: "Boom" } });
    const client = queryClient(initial);
    const { result } = renderHook(() => useUpdateState(), { wrapper: wrapper(client) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({
          workspaceSlug: TEST_WORKSPACE,
          projectId: TEST_PROJECT,
          stateId: testRowId("a"),
          patch: { description: "new" },
        })
      ).rejects.toBeDefined();
    });
    expect(notifyError).toHaveBeenCalledWith(STATE_UPDATE_FAILED_TITLE, "Boom");
    expect(client.getQueryData<State[]>(KEY)?.[0]?.description).toBe("");
  });
});

describe("useMoveState", () => {
  it("moves optimistically and stores the server answer", async () => {
    const initial = stateList([
      { name: "a", group: "backlog", sequence: 10 },
      { name: "b", group: "test", sequence: 5 },
    ]);
    stubTransport();
    const client = queryClient(initial);
    // patchEcho derives the row from the id and the request body.
    const { result } = renderHook(() => useMoveState(), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.mutateAsync({
        workspaceSlug: TEST_WORKSPACE,
        projectId: TEST_PROJECT,
        move: { stateId: testRowId("a"), group: "test", sequence: 2.5 },
      });
    });
    expect(client.getQueryData<State[]>(KEY)?.[0]).toMatchObject({ group: "test", sequence: 2.5 });
  });

  it("rolls the placement back with a notice when the move is refused", async () => {
    const initial = stateList([
      { name: "a", group: "backlog", sequence: 10 },
      { name: "b", group: "test", sequence: 5 },
    ]);
    stubTransport({ patchStatus: 500, patchBody: { detail: "refused" } });
    const client = queryClient(initial);
    const { result } = renderHook(() => useMoveState(), { wrapper: wrapper(client) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({
          workspaceSlug: TEST_WORKSPACE,
          projectId: TEST_PROJECT,
          move: { stateId: testRowId("a"), group: "test", sequence: 2.5 },
        })
      ).rejects.toBeDefined();
    });
    expect(notifyError).toHaveBeenCalledWith(STATE_UPDATE_FAILED_TITLE, "refused");
    expect(client.getQueryData<State[]>(KEY)?.[0]).toMatchObject({ group: "backlog", sequence: 10 });
  });
});

describe("useDeleteState", () => {
  it("removes the row only after the server accepts", async () => {
    const client = queryClient(stateList([{ name: "a", group: "backlog" }]));
    stubTransport();
    const { result } = renderHook(() => useDeleteState(), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.mutateAsync({
        workspaceSlug: TEST_WORKSPACE,
        projectId: TEST_PROJECT,
        stateId: testRowId("a"),
      });
    });
    expect(notifySuccess).toHaveBeenCalledWith(STATE_DELETED_TITLE);
    expect(client.getQueryData<State[]>(KEY)).toEqual([]);
  });

  it("guides the operator to move work items when the state is occupied", async () => {
    const client = queryClient(stateList([{ name: "a", group: "backlog" }]));
    stubTransport({
      deleteStatus: 400,
      deleteBody: { error: "The state is not empty, only empty states can be deleted" },
    });
    const { result } = renderHook(() => useDeleteState(), { wrapper: wrapper(client) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({ workspaceSlug: TEST_WORKSPACE, projectId: TEST_PROJECT, stateId: testRowId("a") })
      ).rejects.toBeDefined();
    });
    expect(notifyError).toHaveBeenCalledWith(STATE_DELETE_OCCUPIED_TITLE, STATE_DELETE_OCCUPIED_MESSAGE);
    expect(client.getQueryData<State[]>(KEY)).toHaveLength(1);
  });

  it("uses the generic deletion notice for other refusals", async () => {
    const client = queryClient(stateList([{ name: "a", group: "backlog" }]));
    stubTransport({ deleteStatus: 500, deleteBody: { detail: "Boom" } });
    const { result } = renderHook(() => useDeleteState(), { wrapper: wrapper(client) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({ workspaceSlug: TEST_WORKSPACE, projectId: TEST_PROJECT, stateId: testRowId("a") })
      ).rejects.toBeDefined();
    });
    expect(notifyError).toHaveBeenCalledWith(STATE_DELETE_FAILED_TITLE, "Boom");
    expect(client.getQueryData<State[]>(KEY)).toHaveLength(1);
  });
});

describe("useMarkStateDefault", () => {
  it("flips the exclusive flag optimistically and confirms", async () => {
    const client = queryClient(
      stateList([
        { name: "a", group: "backlog", default: true },
        { name: "b", group: "test" },
      ])
    );
    stubTransport();
    const { result } = renderHook(() => useMarkStateDefault(), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.mutateAsync({
        workspaceSlug: TEST_WORKSPACE,
        projectId: TEST_PROJECT,
        stateId: testRowId("b"),
      });
    });
    expect(notifySuccess).toHaveBeenCalledWith(STATE_DEFAULT_TITLE);
    expect(client.getQueryData<State[]>(KEY)?.map((entry) => entry.default)).toEqual([false, true]);
  });

  it("restores the old default with a notice when the promotion fails", async () => {
    const client = queryClient(
      stateList([
        { name: "a", group: "backlog", default: true },
        { name: "b", group: "test" },
      ])
    );
    stubTransport({ defaultStatus: 500, defaultBody: { detail: "refused" } });
    const { result } = renderHook(() => useMarkStateDefault(), { wrapper: wrapper(client) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({ workspaceSlug: TEST_WORKSPACE, projectId: TEST_PROJECT, stateId: testRowId("b") })
      ).rejects.toBeDefined();
    });
    expect(notifyError).toHaveBeenCalledWith(STATE_DEFAULT_FAILED_TITLE, "refused");
    expect(client.getQueryData<State[]>(KEY)?.map((entry) => entry.default)).toEqual([true, false]);
  });
});

describe("useSetMemberEditing", () => {
  const projectKey = projectQueryOptions({} as never, TEST_WORKSPACE, TEST_PROJECT).queryKey;

  it("writes the flag, notices the new state and seeds the cached project", async () => {
    const calls = stubTransport({ calls: [] }).calls;
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false }, queries: { retry: false } } });
    client.setQueryData(projectKey, projectDetail({ members_can_edit_states: false }));
    const { result } = renderHook(() => useSetMemberEditing(), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.mutateAsync({ workspaceSlug: TEST_WORKSPACE, projectId: TEST_PROJECT, enabled: true });
    });
    expect(calls.some((call) => call.startsWith("PATCH ") && call.endsWith(`/projects/${TEST_PROJECT}/`))).toBe(true);
    expect(notifySuccess).toHaveBeenCalledWith(MEMBER_EDITING_ON_TITLE);
    await waitFor(() => {
      expect((client.getQueryData(projectKey) as { members_can_edit_states?: boolean }).members_can_edit_states).toBe(
        true
      );
    });
  });

  it("notices the off state", async () => {
    stubTransport();
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false }, queries: { retry: false } } });
    const { result } = renderHook(() => useSetMemberEditing(), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.mutateAsync({ workspaceSlug: TEST_WORKSPACE, projectId: TEST_PROJECT, enabled: false });
    });
    expect(notifySuccess).toHaveBeenCalledWith(MEMBER_EDITING_OFF_TITLE);
  });

  it("notices a refused flip with the server detail", async () => {
    stubTransport({ projectStatus: 403 });
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false }, queries: { retry: false } } });
    const { result } = renderHook(() => useSetMemberEditing(), { wrapper: wrapper(client) });
    await act(async () => {
      await expect(
        result.current.mutateAsync({ workspaceSlug: TEST_WORKSPACE, projectId: TEST_PROJECT, enabled: true })
      ).rejects.toBeDefined();
    });
    expect(notifyError).toHaveBeenCalledWith(MEMBER_EDITING_FAILED_TITLE, expect.any(String));
  });
});

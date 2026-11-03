// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Writes behind the General tab (PSET-006–008): the save body and its
// confirmation, the bind → canonical-write → status-refresh chain, the
// refusal path and the emblem upload's grant/bytes/confirm legs.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { authKeys } from "../../../auth/queries.js";

vi.mock("../../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import {
  EMBLEM_UPLOAD_FAILED_MESSAGE,
  EMBLEM_UPLOAD_FAILED_TITLE,
  PROJECT_SAVED_MESSAGE,
  PROJECT_SAVED_TITLE,
  REPO_BIND_FAILED_TITLE,
  REPO_BOUND_MESSAGE,
  REPO_BOUND_TITLE,
  useBindRepository,
  useSaveProjectGeneral,
  useUploadEmblem,
} from "./mutations.js";
import { generalKeys } from "./queries.js";

const UUID = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_PATH = `/api/workspaces/acme/projects/${UUID}/`;
const REPOSITORY_PATH = `/api/workspaces/acme/projects/${UUID}/repository/`;
const BIND_PATH = `/api/workspaces/acme/projects/${UUID}/repository/bind/`;
const ASSET_PATH = `/api/assets/v2/workspaces/acme/projects/${UUID}/`;
const WORKSPACE_ASSET_PATH = "/api/assets/v2/workspaces/acme/";

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
    description: "",
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

function binding(overrides: Record<string, unknown> = {}) {
  return {
    bound: true,
    id: "323e4567-e89b-12d3-a456-426614174002",
    provider: "github",
    provider_account_id: null,
    host_url: "https://github.com",
    repository: {
      id: "repo-1",
      provider: "github",
      host_url: "https://github.com",
      namespace: "acme",
      name: "site",
      full_name: "acme/site",
      web_url: "https://github.com/acme/site",
      clone_url_http: "https://github.com/acme/site.git",
      clone_url_ssh: "git@github.com:acme/site.git",
      default_branch: "main",
      private: true,
    },
    is_sync_enabled: false,
    clone_auth_mode: "runner_managed",
    last_synced_at: null,
    last_sync_error: "",
    degraded: false,
    degraded_reason: null,
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

interface StubOptions {
  patchStatus?: number;
  bindStatus?: number;
  patchRows?: Array<Record<string, unknown>>;
  grantStatus?: number;
  bytesOk?: boolean;
}

function stubApi(options: StubOptions = {}) {
  const calls: Call[] = [];
  const patchRows = [...(options.patchRows ?? [])];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ method, url, body });
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url === `https://api.test${PROJECT_PATH}` && method === "PATCH") {
      const status = options.patchStatus ?? 200;
      if (status !== 200) return jsonResponse(url, status, { detail: "refused", error_code: "refused" });
      const row = patchRows.shift();
      return jsonResponse(url, 200, row ?? projectRow((body as Record<string, unknown>) ?? {}));
    }
    if (url === `https://api.test${REPOSITORY_PATH}` && method === "GET") {
      return jsonResponse(url, 200, { bound: false });
    }
    if (url === `https://api.test${BIND_PATH}` && method === "POST") {
      const status = options.bindStatus ?? 201;
      if (status !== 201) return jsonResponse(url, status, { detail: "no provider account" });
      return jsonResponse(url, 201, { ...binding(), repo_url: "https://github.com/acme/site.git" });
    }
    if (url === `https://api.test${WORKSPACE_ASSET_PATH}` && method === "POST") {
      const status = options.grantStatus ?? 200;
      if (status !== 200) return jsonResponse(url, status, { detail: "no" });
      return jsonResponse(url, 200, {
        asset_id: "asset-1",
        asset_url: "/api/assets/v2/static/asset-1/",
        upload_data: { url: "https://uploads.test/signed", fields: { key: "emblems/asset-1.png" } },
      });
    }
    if (url === `https://api.test${WORKSPACE_ASSET_PATH}asset-1/` && method === "PATCH") {
      return jsonResponse(url, 204, {});
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { calls };
}

function testClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
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

describe("useSaveProjectGeneral", () => {
  it("PATCHes the row, confirms, and seeds the tab and shell caches", async () => {
    const { calls } = stubApi({ patchRows: [projectRow({ name: "Renamed", updated_at: "2026-03-01T00:00:00Z" })] });
    const queryClient = testClient();
    const hook = renderHook(() => useSaveProjectGeneral("acme", UUID), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate({ name: "Renamed", identifier: "WEB" });
    });
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));

    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ name: "Renamed", identifier: "WEB" });
    expect(notifySuccess).toHaveBeenCalledWith(PROJECT_SAVED_TITLE, PROJECT_SAVED_MESSAGE);
    expect(queryClient.getQueryData(generalKeys.detail("acme", UUID))).toMatchObject({
      name: "Renamed",
      updated_at: "2026-03-01T00:00:00Z",
    });
    expect(queryClient.getQueryData(authKeys.project("acme", UUID))).toMatchObject({ name: "Renamed" });
  });

  it("surfaces a failure to the caller and confirms nothing", async () => {
    stubApi({ patchStatus: 400 });
    const hook = renderHook(() => useSaveProjectGeneral("acme", UUID), { wrapper: wrapper(testClient()) });

    act(() => {
      hook.result.current.mutate({ name: "" });
    });
    await waitFor(() => expect(hook.result.current.isError).toBe(true));
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});

describe("useBindRepository", () => {
  it("binds, writes the canonical URL onto the project and refreshes status", async () => {
    const { calls } = stubApi({
      patchRows: [projectRow({ repo_url: "https://github.com/acme/site.git" })],
    });
    const queryClient = testClient();
    queryClient.setQueryData(generalKeys.detail("acme", UUID), projectRow());
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const hook = renderHook(() => useBindRepository("acme", UUID), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate({ repoUrl: "github.com/acme/site" });
    });
    await waitFor(() => expect(queryClient.getQueryData(generalKeys.repository("acme", UUID))).toBeDefined());

    expect(calls.find((call) => call.url === `https://api.test${BIND_PATH}`)?.body).toEqual({
      repo_url: "github.com/acme/site",
    });
    expect(calls.find((call) => call.url === `https://api.test${PROJECT_PATH}`)?.body).toEqual({
      repo_url: "https://github.com/acme/site.git",
    });
    expect(queryClient.getQueryData(generalKeys.detail("acme", UUID))).toMatchObject({
      repo_url: "https://github.com/acme/site.git",
    });
    expect(queryClient.getQueryData(generalKeys.repository("acme", UUID))).toMatchObject({
      bound: true,
      repo_url: "https://github.com/acme/site.git",
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: generalKeys.repository("acme", UUID) });
    expect(notifySuccess).toHaveBeenCalledWith(REPO_BOUND_TITLE, REPO_BOUND_MESSAGE);
  });

  it("skips the canonical write when the tab row is not cached", async () => {
    const { calls } = stubApi({});
    const queryClient = testClient();
    const hook = renderHook(() => useBindRepository("acme", UUID), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate({ repoUrl: "https://github.com/acme/site" });
    });
    await waitFor(() => expect(queryClient.getQueryData(generalKeys.repository("acme", UUID))).toBeDefined());
    expect(calls.some((call) => call.url === `https://api.test${PROJECT_PATH}`)).toBe(false);
    expect(notifySuccess).toHaveBeenCalledWith(REPO_BOUND_TITLE, REPO_BOUND_MESSAGE);
  });

  it("toasts the server's refusal and writes nothing", async () => {
    const { calls } = stubApi({ bindStatus: 409 });
    const hook = renderHook(() => useBindRepository("acme", UUID), { wrapper: wrapper(testClient()) });

    act(() => {
      hook.result.current.mutate({ repoUrl: "https://github.com/acme/site" });
    });
    await waitFor(() => expect(hook.result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(REPO_BIND_FAILED_TITLE, expect.any(String));
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});

describe("useUploadEmblem", () => {
  it("runs grant, bytes and confirm against the project asset pipeline", async () => {
    const { calls } = stubApi({});
    const uploads: string[] = [];
    const hook = renderHook(() => useUploadEmblem("acme", UUID), { wrapper: wrapper(testClient()) });
    const file = new File(["bytes"], "emblem.png", { type: "image/png" });

    act(() => {
      hook.result.current.mutate({
        file,
        uploadBytes: async (url) => {
          uploads.push(url);
          return { ok: true, status: 204 };
        },
      });
    });
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));
    expect(hook.result.current.data).toBe("/api/assets/v2/static/asset-1/");
    // The workspace route assigns project_id once from entity_identifier;
    // the project-scoped route is not a shape the server accepts.
    expect(calls.find((call) => call.url === `https://api.test${WORKSPACE_ASSET_PATH}`)?.body).toMatchObject({
      entity_type: "PROJECT_COVER",
      entity_identifier: UUID,
      name: "emblem.png",
      type: "image/png",
    });
    expect(calls.some((call) => call.url === `https://api.test${ASSET_PATH}` && call.method === "POST")).toBe(false);
    expect(uploads).toEqual(["https://uploads.test/signed"]);
    expect(
      calls.some((call) => call.method === "PATCH" && call.url === `https://api.test${WORKSPACE_ASSET_PATH}asset-1/`)
    ).toBe(true);
    expect(calls.some((call) => call.url === "https://api.test/api/assets/v2/user-assets/")).toBe(false);
  });

  it("toasts the pinned failure copy when the bytes leg is refused", async () => {
    stubApi({});
    const hook = renderHook(() => useUploadEmblem("acme", UUID), { wrapper: wrapper(testClient()) });
    const file = new File(["bytes"], "emblem.png", { type: "image/png" });

    act(() => {
      hook.result.current.mutate({ file, uploadBytes: async () => ({ ok: false, status: 500 }) });
    });
    await waitFor(() => expect(hook.result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(EMBLEM_UPLOAD_FAILED_TITLE, EMBLEM_UPLOAD_FAILED_MESSAGE);
  });

  it("toasts when the grant is refused", async () => {
    stubApi({ grantStatus: 500 });
    const hook = renderHook(() => useUploadEmblem("acme", UUID), { wrapper: wrapper(testClient()) });
    const file = new File(["bytes"], "emblem.png", { type: "image/png" });

    act(() => {
      hook.result.current.mutate({ file, uploadBytes: async () => ({ ok: true, status: 204 }) });
    });
    await waitFor(() => expect(hook.result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(EMBLEM_UPLOAD_FAILED_TITLE, EMBLEM_UPLOAD_FAILED_MESSAGE);
  });
});

// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Write hooks behind the general page (WSET-003–006): request bodies,
// cache re-seeding, toast copy, failure paths and the delete landing
// cache refresh.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { sessionKeys } from "../../../../core/session/index.js";
import {
  DETAILS_SAVED_MESSAGE,
  DETAILS_SAVED_TITLE,
  LOGO_REMOVED_MESSAGE,
  LOGO_REMOVED_TITLE,
  LOGO_REMOVE_FAILED_TITLE,
  LOGO_UPLOAD_FAILED_TITLE,
  WORKSPACE_DELETED_TITLE,
  WORKSPACE_DELETE_FAILED_MESSAGE,
  WORKSPACE_DELETE_FAILED_TITLE,
  applyLogoToDetails,
  detailsUpdateInput,
  useDeleteWorkspace,
  useRemoveWorkspaceLogo,
  useSaveWorkspaceDetails,
  useUploadWorkspaceLogo,
} from "./mutations.js";
import { workspaceDetailsKey } from "./details.js";

vi.mock("../../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const DETAILS_PATH = "/api/workspaces/acme/";
const ASSET_PATH = "/api/assets/v2/workspaces/acme/";

function workspaceRow(overrides: Record<string, unknown> = {}) {
  return {
    id: UUID,
    name: "Acme",
    slug: "acme",
    logo: null,
    logo_asset: null,
    logo_url: null,
    owner: UUID,
    organization_size: "2-10",
    timezone: "UTC",
    background_color: "#ffffff",
    total_members: 3,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    ...overrides,
  };
}

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

interface StubOptions {
  patchStatus?: number;
  deleteStatus?: number;
  assetDeleteStatus?: number;
  grantStatus?: number;
  patchRow?: (body: Record<string, unknown>) => Record<string, unknown>;
}

function stubApi(options: StubOptions = {}) {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ method, url, body });
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url === `https://api.test${DETAILS_PATH}` && method === "PATCH") {
      const status = options.patchStatus ?? 200;
      if (status !== 200) return jsonResponse(url, status, { detail: "no" });
      const sent = body as Record<string, unknown>;
      return jsonResponse(url, 200, options.patchRow?.(sent) ?? workspaceRow(sent));
    }
    if (url === `https://api.test${DETAILS_PATH}` && method === "DELETE") {
      return jsonResponse(url, options.deleteStatus ?? 204, {});
    }
    if (url === `https://api.test${ASSET_PATH}` && method === "POST") {
      if ((options.grantStatus ?? 200) !== 200) return jsonResponse(url, options.grantStatus ?? 500, {});
      return jsonResponse(url, 200, {
        asset_id: "asset-1",
        asset_url: "/api/assets/v2/static/asset-1/",
        upload_data: { url: "https://uploads.test/signed", fields: { key: "logos/asset-1.png" } },
      });
    }
    if (url.startsWith(`https://api.test${ASSET_PATH}`) && method === "PATCH") {
      return jsonResponse(url, 204, {});
    }
    if (url.startsWith(`https://api.test${ASSET_PATH}`) && method === "DELETE") {
      return jsonResponse(url, options.assetDeleteStatus ?? 204, {});
    }
    if (url.includes("/api/workspaces/file-assets/") && method === "DELETE") {
      return jsonResponse(url, options.assetDeleteStatus ?? 204, {});
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { calls };
}

function testClient() {
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

describe("detailsUpdateInput", () => {
  it("sends exactly the three editable fields", () => {
    expect(detailsUpdateInput({ name: "Acme", organizationSize: "11-50", timezone: "Pacific/Auckland" })).toEqual({
      name: "Acme",
      organization_size: "11-50",
      timezone: "Pacific/Auckland",
    });
  });
});

describe("applyLogoToDetails", () => {
  it("leaves a missing row missing", () => {
    expect(applyLogoToDetails(undefined, "x")).toBeUndefined();
  });

  it("stores or clears the logo legs", () => {
    const rows = workspaceRow({ logo: "old", logo_asset: UUID, logo_url: "/api/assets/v2/static/old/" });
    const stored = applyLogoToDetails(rows, "/api/assets/v2/static/new/");
    expect(stored).toMatchObject({ logo: "/api/assets/v2/static/new/", logo_url: "/api/assets/v2/static/new/" });
    expect(applyLogoToDetails(rows, null)).toMatchObject({ logo: null, logo_asset: null, logo_url: null });
  });
});

describe("useSaveWorkspaceDetails", () => {
  it("PATCHes the form and re-seeds the shared row", async () => {
    const { calls } = stubApi();
    const queryClient = testClient();
    const hook = renderHook(() => useSaveWorkspaceDetails("acme"), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate({ name: "Renamed", organization_size: "11-50", timezone: "Pacific/Auckland" });
    });

    await waitFor(() => {
      expect(hook.result.current.isSuccess).toBe(true);
    });
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toBe(`https://api.test${DETAILS_PATH}`);
    expect(patch?.body).toEqual({ name: "Renamed", organization_size: "11-50", timezone: "Pacific/Auckland" });
    expect(queryClient.getQueryData(workspaceDetailsKey("acme"))).toMatchObject({ name: "Renamed" });
    expect(notifySuccess).toHaveBeenCalledWith(DETAILS_SAVED_TITLE, DETAILS_SAVED_MESSAGE);
  });

  it("stays quiet on a failed save and leaves the cache alone", async () => {
    stubApi({ patchStatus: 500 });
    const queryClient = testClient();
    queryClient.setQueryData(workspaceDetailsKey("acme"), workspaceRow());
    const hook = renderHook(() => useSaveWorkspaceDetails("acme"), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate({ name: "Renamed" });
    });

    await waitFor(() => {
      expect(hook.result.current.isError).toBe(true);
    });
    expect(queryClient.getQueryData(workspaceDetailsKey("acme"))).toMatchObject({ name: "Acme" });
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });
});

describe("useUploadWorkspaceLogo", () => {
  it("runs grant, bytes, confirm and the logo PATCH, then closes quietly", async () => {
    const { calls } = stubApi();
    const queryClient = testClient();
    const uploads: Array<{ url: string }> = [];
    const hook = renderHook(() => useUploadWorkspaceLogo("acme"), { wrapper: wrapper(queryClient) });
    const file = new File(["bytes"], "logo.png", { type: "image/png" });

    act(() => {
      hook.result.current.mutate({
        workspaceId: UUID,
        file,
        uploadBytes: async (url) => {
          uploads.push({ url });
          return { ok: true, status: 204 };
        },
      });
    });

    await waitFor(() => {
      expect(hook.result.current.isSuccess).toBe(true);
    });
    expect(calls.find((call) => call.method === "POST")?.body).toMatchObject({
      entity_type: "WORKSPACE_LOGO",
      entity_identifier: UUID,
    });
    expect(calls.find((call) => call.url.endsWith("/asset-1/") && call.method === "PATCH")).toBeDefined();
    const patch = calls.find((call) => call.method === "PATCH" && call.url === `https://api.test${DETAILS_PATH}`);
    expect(patch?.body).toEqual({ logo: "/api/assets/v2/static/asset-1/" });
    expect(uploads).toEqual([{ url: "https://uploads.test/signed" }]);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("toasts an upload failure and stores nothing", async () => {
    const { calls } = stubApi({ grantStatus: 500 });
    const queryClient = testClient();
    const hook = renderHook(() => useUploadWorkspaceLogo("acme"), { wrapper: wrapper(queryClient) });
    const file = new File(["bytes"], "logo.png", { type: "image/png" });

    act(() => {
      hook.result.current.mutate({ workspaceId: UUID, file, uploadBytes: async () => ({ ok: true, status: 204 }) });
    });

    await waitFor(() => {
      expect(hook.result.current.isError).toBe(true);
    });
    expect(notifyError).toHaveBeenCalledWith(LOGO_UPLOAD_FAILED_TITLE, expect.any(String));
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });
});

describe("useRemoveWorkspaceLogo", () => {
  it("deletes the current asset by its key then clears the logo", async () => {
    const { calls } = stubApi();
    const queryClient = testClient();
    queryClient.setQueryData(
      workspaceDetailsKey("acme"),
      workspaceRow({ logo_asset: UUID, logo_url: "/api/assets/v2/static/asset-9/" })
    );
    const hook = renderHook(() => useRemoveWorkspaceLogo("acme"), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate({
        workspaceId: UUID,
        logoUrl: "/api/assets/v2/static/asset-9/",
        legacy: false,
      });
    });

    await waitFor(() => {
      expect(hook.result.current.isSuccess).toBe(true);
    });
    expect(calls.find((call) => call.method === "DELETE")?.url).toBe(
      "https://api.test/api/assets/v2/workspaces/acme/asset-9/"
    );
    expect(calls.find((call) => call.url === `https://api.test${DETAILS_PATH}`)?.body).toEqual({ logo: "" });
    expect(queryClient.getQueryData(workspaceDetailsKey("acme"))).toMatchObject({ logo_url: null, logo_asset: null });
    expect(notifySuccess).toHaveBeenCalledWith(LOGO_REMOVED_TITLE, LOGO_REMOVED_MESSAGE);
  });

  it("uses the legacy file-asset path for a full-URL logo", async () => {
    const { calls } = stubApi();
    const queryClient = testClient();
    const hook = renderHook(() => useRemoveWorkspaceLogo("acme"), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate({ workspaceId: UUID, logoUrl: "https://cdn.test/old/logo.png", legacy: true });
    });

    await waitFor(() => {
      expect(hook.result.current.isSuccess).toBe(true);
    });
    expect(calls.find((call) => call.method === "DELETE")?.url).toBe(
      `https://api.test/api/workspaces/file-assets/${UUID}/logo.png/`
    );
  });

  it("toasts a failed workspace write and keeps the stored logo", async () => {
    stubApi({ patchStatus: 500 });
    const queryClient = testClient();
    queryClient.setQueryData(
      workspaceDetailsKey("acme"),
      workspaceRow({ logo_asset: UUID, logo_url: "/api/assets/v2/static/asset-9/" })
    );
    const hook = renderHook(() => useRemoveWorkspaceLogo("acme"), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate({ workspaceId: UUID, logoUrl: "/api/assets/v2/static/asset-9/", legacy: false });
    });

    await waitFor(() => {
      expect(hook.result.current.isError).toBe(true);
    });
    expect(notifyError).toHaveBeenCalledWith(LOGO_REMOVE_FAILED_TITLE, expect.any(String));
    expect(queryClient.getQueryData(workspaceDetailsKey("acme"))).toMatchObject({
      logo_url: "/api/assets/v2/static/asset-9/",
    });
  });
});

describe("useDeleteWorkspace", () => {
  it("DELETEs, drops the workspace cache and refreshes the landing reads", async () => {
    const { calls } = stubApi();
    const queryClient = testClient();
    queryClient.setQueryData(workspaceDetailsKey("acme"), workspaceRow());
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const hook = renderHook(() => useDeleteWorkspace("acme"), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate();
    });

    await waitFor(() => {
      expect(hook.result.current.isSuccess).toBe(true);
    });
    expect(calls.find((call) => call.method === "DELETE")?.url).toBe(`https://api.test${DETAILS_PATH}`);
    expect(queryClient.getQueryData(workspaceDetailsKey("acme"))).toBeUndefined();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: sessionKeys.settings });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: sessionKeys.workspaces });
    expect(WORKSPACE_DELETED_TITLE).toBe("Workspace deleted.");
  });

  it("toasts a failed delete and keeps the cache", async () => {
    stubApi({ deleteStatus: 403 });
    const queryClient = testClient();
    queryClient.setQueryData(workspaceDetailsKey("acme"), workspaceRow());
    const hook = renderHook(() => useDeleteWorkspace("acme"), { wrapper: wrapper(queryClient) });

    act(() => {
      hook.result.current.mutate();
    });

    await waitFor(() => {
      expect(hook.result.current.isError).toBe(true);
    });
    expect(notifyError).toHaveBeenCalledWith(WORKSPACE_DELETE_FAILED_TITLE, WORKSPACE_DELETE_FAILED_MESSAGE);
    expect(queryClient.getQueryData(workspaceDetailsKey("acme"))).toBeDefined();
  });
});

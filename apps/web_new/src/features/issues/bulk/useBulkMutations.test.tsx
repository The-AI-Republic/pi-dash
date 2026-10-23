// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import { bulkFailureDescription, useBulkMutations } from "./useBulkMutations.js";

const { notifySuccess, notifyError } = vi.hoisted(() => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifySuccess, notifyError };
});

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ID1 = "123e4567-e89b-12d3-a456-426614174000";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const MODULE = "423e4567-e89b-12d3-a456-426614174003";

let calls: Array<{ url: string; method: string; body: string }> = [];
let fail = false;
let emptyMessage = false;

function response(status: number, body: unknown) {
  return {
    status,
    url: "https://api.test/",
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  };
}

function setup() {
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/csrf/")) {
      return response(200, { csrf_token: "csrf" });
    }
    if (fail) {
      return response(400, emptyMessage ? { message: "" } : { detail: "Bulk action refused" });
    }
    return response(200, { message: "ok", archived_at: "2026-05-01" });
  }) as never);
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false }, queries: { retry: false } } });
  const wrapper = function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return React.createElement(QueryClientProvider, { client }, children as React.ReactNode);
  };
  return { client, ...renderHook(() => useBulkMutations({ workspaceSlug: SLUG, projectId: PROJECT }), { wrapper }) };
}

beforeEach(() => {
  calls = [];
  fail = false;
  emptyMessage = false;
  notifySuccess.mockClear();
  notifyError.mockClear();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetAppClient();
});

describe("useBulkMutations", () => {
  it("posts a bulk update with the selected ids and properties", async () => {
    const { result } = setup();
    await act(async () => {
      await result.current.update.mutateAsync({ issueIds: [ID1], properties: { priority: "high" } });
    });
    const call = calls.find((entry) => entry.url.includes("/bulk-operation-issues/"));
    expect(call?.method).toBe("POST");
    expect(JSON.parse(call?.body ?? "{}")).toEqual({ issue_ids: [ID1], properties: { priority: "high" } });
    expect(notifySuccess).toHaveBeenCalledWith("Work items updated");
  });

  it("deletes, archives and subscribes in one request each", async () => {
    const { result } = setup();
    await act(async () => {
      await result.current.remove.mutateAsync([ID1]);
      await result.current.archive.mutateAsync([ID1]);
      await result.current.subscribe.mutateAsync([ID1]);
    });
    expect(calls.find((entry) => entry.url.includes("/bulk-delete-issues/"))?.method).toBe("DELETE");
    expect(calls.find((entry) => entry.url.includes("/bulk-archive-issues/"))?.method).toBe("POST");
    expect(calls.find((entry) => entry.url.includes("/bulk-subscribe-issues/"))?.method).toBe("POST");
    expect(notifySuccess).toHaveBeenCalledWith("Work items deleted");
    expect(notifySuccess).toHaveBeenCalledWith("Work items archived", "Find them again under Archives.");
    expect(notifySuccess).toHaveBeenCalledWith("Subscribed to work items");
  });

  it("invalidates every list under the workspace after a write", async () => {
    const { result, client } = setup();
    const projectKey = issueKeys.lists(SLUG, PROJECT);
    const cycleKey = issueKeys.lists(SLUG, "cycle", PROJECT, CYCLE);
    const moduleKey = issueKeys.lists(SLUG, "module", PROJECT, MODULE);
    for (const queryKey of [projectKey, cycleKey, moduleKey]) {
      client.setQueryData(queryKey, { pages: [], pageParams: [] });
    }
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await act(async () => {
      await result.current.remove.mutateAsync([ID1]);
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.lists(SLUG) });
    for (const queryKey of [projectKey, cycleKey, moduleKey]) {
      expect(client.getQueryCache().find({ queryKey })?.state.isInvalidated).toBe(true);
    }
  });

  it("marks a cycle list stale after a bulk archive", async () => {
    const { result, client } = setup();
    const cycleKey = issueKeys.lists(SLUG, "cycle", PROJECT, CYCLE);
    client.setQueryData(cycleKey, { pages: [], pageParams: [] });
    await act(async () => {
      await result.current.archive.mutateAsync([ID1]);
    });
    expect(client.getQueryCache().find({ queryKey: cycleKey })?.state.isInvalidated).toBe(true);
  });

  it("reports the server detail and keeps the selection on failure", async () => {
    fail = true;
    const { result } = setup();
    await act(async () => {
      await expect(result.current.remove.mutateAsync([ID1])).rejects.toBeTruthy();
    });
    expect(notifyError).toHaveBeenCalledWith("Couldn't apply the bulk action", "Bulk action refused");
  });

  it("falls back to retry copy when an API error carries no message", () => {
    expect(bulkFailureDescription(new ApiError({ code: "http", message: "" }))).toBe(
      "Nothing changed — check your connection and try again."
    );
    expect(bulkFailureDescription(new Error("socket closed"))).toBe(
      "Nothing changed — check your connection and try again."
    );
  });
});

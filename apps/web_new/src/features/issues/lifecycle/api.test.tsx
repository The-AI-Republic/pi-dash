// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lifecycle invalidation (ARCH-002): archiving or deleting a work item
// moves the archived total, so the hooks invalidate the archives badge
// query next to the issue reads. The badge refetches while the archives
// shell is mounted and re-reads on its next visit otherwise.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { archivesKeys } from "../../archives/index.js";
import { useArchiveWorkItem, useDeleteWorkItem, type WorkItemScope } from "./api.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-42661417400a";
const SCOPE: WorkItemScope = { workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE };

function setup() {
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (init.method === "POST" && url.endsWith("/archive/")) return json(200, { archived_at: "2026-03-01" });
    if (init.method === "DELETE") return json(204, {});
    return json(200, {});
  }) as never);
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false }, queries: { retry: false } } });
  client.setQueryData(archivesKeys.count(SLUG, PROJECT), 11);
  const wrapper = function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return React.createElement(QueryClientProvider, { client }, children as React.ReactNode);
  };
  return { client, wrapper };
}

afterEach(() => {
  vi.restoreAllMocks();
  resetAppClient();
});

describe("lifecycle archives invalidation", () => {
  it("marks the archived count stale after an archive (ARCH-002)", async () => {
    const { client, wrapper } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useArchiveWorkItem(SCOPE), { wrapper });

    await act(async () => {
      await result.current.mutateAsync();
    });

    expect(invalidate).toHaveBeenCalledWith({ queryKey: archivesKeys.all(SLUG, PROJECT) });
    expect(client.getQueryCache().find({ queryKey: archivesKeys.count(SLUG, PROJECT) })?.state.isInvalidated).toBe(
      true
    );
  });

  it("marks the archived count stale after a delete (ARCH-002)", async () => {
    const { client, wrapper } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useDeleteWorkItem(SCOPE), { wrapper });

    await act(async () => {
      await result.current.mutateAsync();
    });

    expect(invalidate).toHaveBeenCalledWith({ queryKey: archivesKeys.all(SLUG, PROJECT) });
    expect(client.getQueryCache().find({ queryKey: archivesKeys.count(SLUG, PROJECT) })?.state.isInvalidated).toBe(
      true
    );
  });
});

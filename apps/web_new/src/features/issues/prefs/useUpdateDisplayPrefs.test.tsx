// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { IssuesLoaderProvider, useIssuesLoader } from "../loaders/loaderContext.js";
import type { IssuesEntity } from "./entities.js";
import { selectDisplayPrefs, useEntityPrefs, useUpdateDisplayPrefs, type DisplayPrefs } from "./displayPrefs.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { entityListsKeyPart } from "./entities.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const ENTITY: IssuesEntity = { scope: "project", projectId: UUID2 };
const LISTS_KEY = [...issueKeys.lists("acme", ...entityListsKeyPart(ENTITY)), "probe"] as const;

function prefsPayload(displayFilters: Record<string, unknown>, displayProperties: Record<string, unknown>) {
  return {
    id: UUID,
    workspace: UUID,
    project: UUID2,
    user: UUID,
    filters: {},
    display_filters: displayFilters,
    display_properties: displayProperties,
    rich_filters: {},
    preferences: {},
    sort_order: 1,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

interface StubOptions {
  patchStatus?: number;
  holdPatch?: { release: () => void } | null;
  holdLists?: { release: () => void } | null;
}

function stubClient(options: StubOptions = {}) {
  let serverFilters: Record<string, unknown> = {
    layout: "list",
    group_by: null,
    order_by: "sort_order",
    future_key: "kept",
  };
  let serverProperties: Record<string, unknown> = {};
  const calls: Array<{ url: string; method: string; body: string }> = [];
  const client = createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
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
      if (options.holdPatch) await new Promise<void>((resolve) => (options.holdPatch!.release = resolve));
      const status = options.patchStatus ?? 201;
      if (status !== 201 && status !== 200) {
        return { status, url, headers: { get: () => null }, text: async () => "{}" };
      }
      const sent = JSON.parse(init.body ?? "{}") as {
        display_filters?: Record<string, unknown>;
        display_properties?: Record<string, unknown>;
      };
      if (sent.display_filters !== undefined) serverFilters = sent.display_filters;
      if (sent.display_properties !== undefined) serverProperties = sent.display_properties;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(prefsPayload(serverFilters, serverProperties)),
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(prefsPayload(serverFilters, serverProperties)),
    };
  }) as never);
  return { client, calls };
}

describe("useUpdateDisplayPrefs", () => {
  function renderHandle(options: StubOptions = {}) {
    const { calls } = stubClient(options);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    let listFetches = 0;
    let listRelease: (() => void) | null = null;
    let update: ((partial: Partial<DisplayPrefs>) => void) | null = null;
    function Probe(): React.ReactElement {
      const prefs = useEntityPrefs("acme", ENTITY);
      const writer = useUpdateDisplayPrefs("acme", ENTITY);
      const loader = useIssuesLoader();
      // An active lists subscriber, standing in for the rendered layout.
      useQuery({
        queryKey: LISTS_KEY,
        queryFn: async () => {
          listFetches += 1;
          if (options.holdLists) await new Promise<void>((resolve) => (listRelease = resolve));
          return { probe: true };
        },
      });
      update = writer.updateDisplayPrefs;
      const layout = prefs.data ? selectDisplayPrefs(prefs.data).layout : "loading";
      return <p>{`layout:${layout}|saving:${writer.isSaving}|loader:${loader.isMutationLoading}`}</p>;
    }
    render(
      <QueryClientProvider client={queryClient}>
        <IssuesLoaderProvider>
          <Probe />
        </IssuesLoaderProvider>
      </QueryClientProvider>
    );
    return {
      calls,
      queryClient,
      listFetchCount: () => listFetches,
      releaseLists: () => listRelease?.(),
      text: () => screen.getByText(/^layout:/).textContent ?? "",
      update: (partial: Partial<DisplayPrefs>) => update?.(partial),
    };
  }

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetAppClient();
  });

  it("reads the stored layout on mount through the entity route", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe("layout:list|saving:false|loader:false"));
    const get = h.calls.find((call) => call.method === "GET" && call.url.includes("user-properties"));
    expect(get?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID2}/user-properties/`);
  });

  it("persists a layout switch with complete key objects", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe("layout:list|saving:false|loader:false"));
    h.update({ layout: "kanban" });
    await waitFor(() => expect(h.text()).toBe("layout:kanban|saving:false|loader:false"));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/api/workspaces/acme/projects/${UUID2}/user-properties/`);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({
      display_filters: {
        layout: "kanban",
        group_by: null,
        order_by: "sort_order",
        sub_group_by: null,
        show_empty_groups: false,
        sub_issue: true,
        calendar: { layout: "month", show_weekends: true },
        future_key: "kept",
      },
      display_properties: {},
    });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("refetches the entity lists once the write lands", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe("layout:list|saving:false|loader:false"));
    expect(h.listFetchCount()).toBe(1);
    h.update({ layout: "calendar" });
    await waitFor(() => expect(h.text()).toBe("layout:calendar|saving:false|loader:false"));
    expect(h.listFetchCount()).toBe(2);
  });

  it("rolls back and toasts when the save fails", async () => {
    const h = renderHandle({ patchStatus: 500 });
    await waitFor(() => expect(h.text()).toBe("layout:list|saving:false|loader:false"));
    h.update({ layout: "kanban" });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't save display preferences", expect.any(String))
    );
    await waitFor(() => expect(h.text()).toBe("layout:list|saving:false|loader:false"));
  });

  it("holds the mutation loader across the write and the list refetch", async () => {
    const holdPatch: { release: () => void } = { release: () => {} };
    const holdLists: { release: () => void } = { release: () => {} };
    const h = renderHandle({ holdPatch, holdLists });
    await waitFor(() => expect(h.text()).toBe("layout:list|saving:false|loader:false"));
    h.update({ layout: "kanban" });
    // The loader rises while the PATCH pends and stays up through the refetch.
    await waitFor(() => expect(h.text()).toContain("loader:true"));
    h.releaseLists();
    holdPatch.release();
    await waitFor(() => expect(h.listFetchCount()).toBe(2));
    expect(h.text()).toContain("loader:true");
    h.releaseLists();
    await waitFor(() => expect(h.text()).toBe("layout:kanban|saving:false|loader:false"));
  });
});
